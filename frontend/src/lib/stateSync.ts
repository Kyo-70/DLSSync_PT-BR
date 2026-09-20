/** Authoritative state consumer.
 *
 *  Rust owns observed state. This module keeps one root-lifetime store in step with it and never
 *  invents a value the backend did not publish.
 *
 *  Contract enforced here, from `plans/phase3-continuation-20260913/phase4-spec.md`:
 *
 *  1. Subscribe to `STATE_EVENT` BEFORE requesting the snapshot, buffering what arrives meanwhile.
 *  2. Accept a delta only when the emitter matches, `sequence == current + 1` and
 *     `delta.base_revision == current.revision`. Anything else pauses deltas and repairs by
 *     snapshot exactly once per detection.
 *  3. Fence listener callbacks and snapshot responses with a frontend connection generation. The
 *     Rust emitter epoch is a different thing and cannot be reused for this.
 *  4. Poll the watermark every 5 s and on window focus; a higher sequence means a missed event.
 *  5. Install each accepted delta as a single store write, so a slower command response can never
 *     overwrite newer state.
 *
 *  `Counter` values are decimal strings covering the full u64 range, so every comparison uses
 *  `BigInt`. Parsing failures are treated as a desynchronised stream, not as zero. */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { readonly, writable, type Readable } from "svelte/store";
import {
  STATE_EVENT,
  stateSnapshot,
  stateWatermark,
  type AuthoritativeSnapshot,
  type BackupView,
  type CatalogState,
  type Counter,
  type GameSnapshot,
  type HistoryView,
  type OperationSnapshot,
  type StateCounts,
  type StateDelta,
  type StateEvent,
} from "./api";

/** Hard bound on events held while the first snapshot is in flight. Beyond it the buffer is
 *  dropped and the stream is repaired by snapshot, which is cheaper than unbounded memory. */
const MAX_BUFFERED_EVENTS = 256;
const WATERMARK_POLL_MS = 5_000;

export interface AuthoritativeState {
  /** Rust emitter identity of the installed data. `null` until the first snapshot lands. */
  emitterId: string | null;
  /** Inclusive watermark of what is installed, as the backend's decimal strings. */
  sequence: Counter | null;
  revision: Counter | null;
  capturedAt: string | null;
  games: GameSnapshot[];
  operations: OperationSnapshot[];
  backups: BackupView[];
  history: HistoryView[];
  historySyncPending: boolean;
  catalog: CatalogState | null;
  counts: StateCounts | null;
  /** True while a gap, emitter change or revision mismatch is being repaired by snapshot. */
  repairing: boolean;
  /** Last repair reason, kept for diagnostics. Not a user-facing message. */
  lastRepairReason: string | null;
}

const EMPTY_STATE: AuthoritativeState = {
  emitterId: null,
  sequence: null,
  revision: null,
  capturedAt: null,
  games: [],
  operations: [],
  backups: [],
  history: [],
  historySyncPending: false,
  catalog: null,
  counts: null,
  repairing: false,
  lastRepairReason: null,
};

const store = writable<AuthoritativeState>(EMPTY_STATE);

/** Read-only authoritative state. Views and derived stores consume this; nothing else writes it. */
export const authoritativeState: Readable<AuthoritativeState> = readonly(store);

function counterValue(raw: Counter | null | undefined): bigint | null {
  if (raw === null || raw === undefined || !/^(0|[1-9][0-9]*)$/.test(raw)) return null;
  try {
    const value = BigInt(raw);
    return value <= 18_446_744_073_709_551_615n ? value : null;
  } catch {
    return null;
  }
}

function mergeById<T extends { id: string }>(
  current: T[],
  incoming: T[] | undefined,
  removed: string[] | undefined,
): T[] {
  if ((incoming === undefined || incoming.length === 0) && (removed === undefined || removed.length === 0)) {
    return current;
  }
  const byId = new Map(current.map((entry) => [entry.id, entry]));
  for (const entry of incoming ?? []) byId.set(entry.id, entry);
  for (const id of removed ?? []) byId.delete(id);
  return [...byId.values()];
}

function installSnapshot(snapshot: AuthoritativeSnapshot): void {
  store.set({
    emitterId: snapshot.emitter_id,
    sequence: snapshot.sequence ?? "0",
    revision: snapshot.revision ?? "0",
    capturedAt: snapshot.captured_at,
    games: snapshot.games ?? [],
    operations: snapshot.operations ?? [],
    backups: snapshot.backups ?? [],
    history: snapshot.history ?? [],
    historySyncPending: snapshot.history_sync_pending ?? false,
    catalog: snapshot.catalog ?? null,
    counts: snapshot.counts ?? null,
    repairing: false,
    lastRepairReason: null,
  });
}

function installDelta(event: StateEvent): void {
  const delta: StateDelta = event.delta;
  store.update((current) => ({
    ...current,
    emitterId: event.emitter_id,
    sequence: event.sequence,
    revision: event.revision,
    games: mergeById(current.games, delta.games, delta.removed_game_ids),
    operations: mergeById(current.operations, delta.operations, delta.removed_operation_ids),
    backups: mergeById(current.backups, delta.backups, delta.removed_backup_ids),
    history: mergeById(current.history, delta.history, delta.removed_history_ids),
    // A null catalog in a delta means "unchanged", not "cleared".
    catalog: delta.catalog ?? current.catalog,
    counts: delta.counts ?? current.counts,
    repairing: false,
    lastRepairReason: null,
  }));
}

/** Why an accepted-event check failed, or `null` when the event is the next one in order. */
function rejectionReason(current: AuthoritativeState, event: StateEvent): string | null {
  if (current.emitterId === null) return "no installed snapshot";
  if (event.emitter_id !== current.emitterId) return "emitter changed";
  const installedSequence = counterValue(current.sequence);
  const eventSequence = counterValue(event.sequence);
  if (installedSequence === null || eventSequence === null) return "unreadable sequence";
  // A duplicate or older event is not a desynchronisation: it is dropped without repair.
  if (eventSequence <= installedSequence) return "already installed";
  if (eventSequence !== installedSequence + 1n) return "sequence gap";
  const installedRevision = counterValue(current.revision);
  const baseRevision = counterValue(event.delta.base_revision);
  if (installedRevision === null || baseRevision === null) return "unreadable revision";
  if (baseRevision !== installedRevision) return "revision mismatch";
  return null;
}

let generation = 0;
let unlisten: UnlistenFn | null = null;
let pollTimer: ReturnType<typeof setInterval> | null = null;
let buffer: StateEvent[] = [];
let buffering = true;
let repairInFlight: Promise<void> | null = null;
let readyPromise: Promise<void> | null = null;
let initialSnapshotPending = false;
let focusHandler: (() => void) | null = null;
const seenEventIds = new Set<string>();

function isStale(localGeneration: number): boolean {
  return localGeneration !== generation;
}

function handleEvent(localGeneration: number, event: StateEvent): void {
  if (isStale(localGeneration)) return;

  if (buffering) {
    // While buffering, identity is tracked inside the buffer only. Marking an id as seen here
    // would make the replay drop its own events.
    if (buffer.some((buffered) => buffered.id === event.id)) return;
    if (buffer.length >= MAX_BUFFERED_EVENTS) {
      buffer = [];
      void repair(localGeneration, "buffer overflow");
      return;
    }
    buffer.push(event);
    return;
  }

  if (seenEventIds.has(event.id)) return;
  seenEventIds.add(event.id);
  if (seenEventIds.size > MAX_BUFFERED_EVENTS * 4) seenEventIds.clear();

  let reason: string | null = null;
  store.update((current) => {
    reason = rejectionReason(current, event);
    return current;
  });
  if (reason === "already installed") return;
  if (reason !== null) {
    void repair(localGeneration, reason);
    return;
  }
  installDelta(event);
}

function replayBuffered(localGeneration: number): void {
  const pending = [...buffer].sort((left, right) => {
    const a = counterValue(left.sequence);
    const b = counterValue(right.sequence);
    if (a === null || b === null) return 0;
    return a < b ? -1 : a > b ? 1 : 0;
  });
  buffer = [];
  buffering = false;
  for (const event of pending) {
    if (isStale(localGeneration)) return;
    handleEvent(localGeneration, event);
  }
}

async function repair(localGeneration: number, reason: string): Promise<void> {
  if (isStale(localGeneration)) return;
  if (repairInFlight !== null) return repairInFlight;
  store.update((current) => ({ ...current, repairing: true, lastRepairReason: reason }));
  buffering = true;
  buffer = [];
  repairInFlight = (async () => {
    try {
      const snapshot = await stateSnapshot();
      if (isStale(localGeneration)) return;
      installSnapshot(snapshot);
      replayBuffered(localGeneration);
    } catch {
      // Keep deltas paused until a fresh snapshot repairs the stream. Reconciliation retries
      // once this request settles, even if the watermark has not changed.
      if (!isStale(localGeneration)) {
        store.update((current) => ({ ...current, repairing: true }));
      }
    } finally {
      if (!isStale(localGeneration)) repairInFlight = null;
    }
  })();
  return repairInFlight;
}

async function reconcileWatermark(localGeneration: number): Promise<void> {
  if (isStale(localGeneration) || initialSnapshotPending || repairInFlight !== null) return;
  if (buffering) {
    await repair(localGeneration, "snapshot retry");
    return;
  }
  let watermark;
  try {
    watermark = await stateWatermark();
  } catch {
    return;
  }
  if (isStale(localGeneration)) return;
  let reason: string | null = null;
  store.update((current) => {
    if (current.emitterId === null) {
      reason = "no installed snapshot";
    } else if (watermark.emitter_id !== current.emitterId) {
      reason = "emitter changed";
    } else {
      const installed = counterValue(current.sequence);
      const remote = counterValue(watermark.sequence);
      if (installed === null || remote === null) reason = "unreadable sequence";
      else if (remote > installed) reason = "missed event";
    }
    return current;
  });
  if (reason !== null) await repair(localGeneration, reason);
}

/** Install the listener, load the first snapshot and start reconciliation.
 *  Safe to call more than once: later calls return the same readiness promise. */
export function startStateSync(): Promise<void> {
  if (readyPromise !== null) return readyPromise;
  generation += 1;
  const localGeneration = generation;
  buffering = true;
  buffer = [];
  seenEventIds.clear();

  readyPromise = (async () => {
    const stopListener = await listen<StateEvent>(STATE_EVENT, (message) => {
      handleEvent(localGeneration, message.payload);
    });
    if (isStale(localGeneration)) { stopListener(); return; }
    unlisten = stopListener;

    pollTimer = setInterval(() => {
      void reconcileWatermark(localGeneration);
    }, WATERMARK_POLL_MS);
    if (typeof window !== "undefined") {
      focusHandler = () => {
        void reconcileWatermark(localGeneration);
      };
      window.addEventListener("focus", focusHandler);
    }

    initialSnapshotPending = true;
    try {
      const snapshot = await stateSnapshot();
      if (isStale(localGeneration)) return;
      installSnapshot(snapshot);
      replayBuffered(localGeneration);
    } catch (error) {
      if (!isStale(localGeneration)) {
        buffer = [];
        store.update((current) => ({
          ...current,
          repairing: true,
          lastRepairReason: `initial snapshot failed: ${String(error)}`,
        }));
      }
      // The listener remains installed and the watermark poll will request a fresh
      // snapshot. The initial caller still receives the IPC error for diagnostics.
      throw error;
    } finally {
      if (!isStale(localGeneration)) initialSnapshotPending = false;
    }
  })();

  return readyPromise;
}

/** Tear down listener and polling. Used by tests and by a deliberate reconnect. */
export async function stopStateSync(): Promise<void> {
  generation += 1;
  readyPromise = null;
  repairInFlight = null;
  initialSnapshotPending = false;
  buffering = true;
  buffer = [];
  seenEventIds.clear();
  if (focusHandler !== null && typeof window !== "undefined") {
    window.removeEventListener("focus", focusHandler);
    focusHandler = null;
  }
  if (pollTimer !== null) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
  if (unlisten !== null) {
    const stop = unlisten;
    unlisten = null;
    await stop();
  }
  store.set(EMPTY_STATE);
}

/** Exposed for tests: force a repair with an explicit reason. */
export function requestStateRepair(reason: string): Promise<void> {
  return repair(generation, reason);
}
