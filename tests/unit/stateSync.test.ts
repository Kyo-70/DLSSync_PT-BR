import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import type { AuthoritativeSnapshot, StateEvent, StateWatermark } from "@/generated/bindings";

const listeners: ((message: { payload: StateEvent }) => void)[] = [];
const unlisten = vi.fn(() => undefined);

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name: string, handler: (message: { payload: StateEvent }) => void) => {
    listeners.push(handler);
    return unlisten;
  }),
  emit: vi.fn(async () => undefined),
}));

const stateSnapshot = vi.fn<[], Promise<AuthoritativeSnapshot>>();
const stateWatermark = vi.fn<[], Promise<StateWatermark>>();

vi.mock("@/lib/api", () => ({
  STATE_EVENT: "state:event",
  stateSnapshot: () => stateSnapshot(),
  stateWatermark: () => stateWatermark(),
}));

const { startStateSync, stopStateSync, authoritativeState } = await import("@/lib/stateSync");

function snapshot(sequence: string, revision: string, gameIds: string[]): AuthoritativeSnapshot {
  return {
    schema_version: 1,
    emitter_id: "emitter-a",
    sequence,
    revision,
    captured_at: "2026-09-17T00:00:00Z",
    games: gameIds.map((id) => ({ id, name: id, install_dir: `C:/games/${id}` })),
    operations: [],
    backups: [],
    history: [],
    catalog: null,
    counts: {
      actionable_games: gameIds.length,
      actionable_components: 0,
      eligible_restorable_backups: 0,
      active_operations: 0,
    },
  } as AuthoritativeSnapshot;
}

function event(options: {
  id: string;
  sequence: string;
  revision: string;
  baseRevision: string;
  emitter?: string;
  addGameId?: string;
  removeGameId?: string;
}): StateEvent {
  return {
    schema_version: 1,
    id: options.id,
    emitter_id: options.emitter ?? "emitter-a",
    sequence: options.sequence,
    operation_id: null,
    game_id: options.addGameId ?? null,
    revision: options.revision,
    emitted_at: "2026-09-17T00:00:01Z",
    delta: {
      base_revision: options.baseRevision,
      games: options.addGameId
        ? [{ id: options.addGameId, name: options.addGameId, install_dir: `C:/games/${options.addGameId}` }]
        : [],
      removed_game_ids: options.removeGameId ? [options.removeGameId] : [],
      catalog: null,
    },
  } as StateEvent;
}

function deliver(payload: StateEvent): void {
  for (const handler of listeners) handler({ payload });
}

async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

describe("authoritative state sync", () => {
  beforeEach(() => {
    listeners.length = 0;
    stateSnapshot.mockReset();
    stateWatermark.mockReset();
    stateWatermark.mockResolvedValue({ emitter_id: "emitter-a", sequence: "5", revision: "9" });
  });

  afterEach(async () => {
    await stopStateSync();
    vi.useRealTimers();
  });

  it("buffers events that arrive before the snapshot and replays them in order", async () => {
    let release: (value: AuthoritativeSnapshot) => void = () => undefined;
    stateSnapshot.mockReturnValue(
      new Promise<AuthoritativeSnapshot>((resolve) => {
        release = resolve;
      }),
    );

    const ready = startStateSync();
    await settle();

    // Out of order on the wire; replay must still install 6 then 7.
    deliver(event({ id: "e7", sequence: "7", revision: "11", baseRevision: "10", addGameId: "g3" }));
    deliver(event({ id: "e6", sequence: "6", revision: "10", baseRevision: "9", addGameId: "g2" }));

    release(snapshot("5", "9", ["g1"]));
    await ready;
    await settle();

    const state = get(authoritativeState);
    expect(state.sequence).toBe("7");
    expect(state.revision).toBe("11");
    expect(state.games.map((game) => game.id).sort()).toEqual(["g1", "g2", "g3"]);
    expect(stateSnapshot).toHaveBeenCalledTimes(1);
  });

  it("repairs by snapshot exactly once when a sequence gap appears", async () => {
    stateSnapshot.mockResolvedValueOnce(snapshot("5", "9", ["g1"]));
    await startStateSync();
    await settle();

    stateSnapshot.mockResolvedValueOnce(snapshot("9", "14", ["g1", "g9"]));
    deliver(event({ id: "e8", sequence: "8", revision: "13", baseRevision: "12", addGameId: "g8" }));
    await settle();

    const state = get(authoritativeState);
    expect(stateSnapshot).toHaveBeenCalledTimes(2);
    expect(state.sequence).toBe("9");
    expect(state.games.map((game) => game.id)).not.toContain("g8");
    expect(state.repairing).toBe(false);
  });

  it("drops a duplicate or older event without repairing and without double applying", async () => {
    stateSnapshot.mockResolvedValueOnce(snapshot("5", "9", ["g1"]));
    await startStateSync();
    await settle();

    const next = event({ id: "e6", sequence: "6", revision: "10", baseRevision: "9", addGameId: "g2" });
    deliver(next);
    await settle();
    deliver(next);
    deliver(event({ id: "e5", sequence: "5", revision: "9", baseRevision: "8", addGameId: "gold" }));
    await settle();

    const state = get(authoritativeState);
    expect(stateSnapshot).toHaveBeenCalledTimes(1);
    expect(state.sequence).toBe("6");
    expect(state.games.map((game) => game.id).sort()).toEqual(["g1", "g2"]);
  });

  it("repairs when the emitter identity changes", async () => {
    stateSnapshot.mockResolvedValueOnce(snapshot("5", "9", ["g1"]));
    await startStateSync();
    await settle();

    stateSnapshot.mockResolvedValueOnce({
      ...snapshot("2", "3", ["g1"]),
      emitter_id: "emitter-b",
    } as AuthoritativeSnapshot);
    deliver(
      event({
        id: "eB",
        sequence: "6",
        revision: "10",
        baseRevision: "9",
        emitter: "emitter-b",
        addGameId: "g2",
      }),
    );
    await settle();

    expect(stateSnapshot).toHaveBeenCalledTimes(2);
    expect(get(authoritativeState).emitterId).toBe("emitter-b");
  });

  it("repairs when the polled watermark is ahead of the installed sequence", async () => {
    vi.useFakeTimers();
    stateSnapshot.mockResolvedValueOnce(snapshot("5", "9", ["g1"]));
    await startStateSync();
    await settle();

    stateWatermark.mockResolvedValue({ emitter_id: "emitter-a", sequence: "12", revision: "20" });
    stateSnapshot.mockResolvedValueOnce(snapshot("12", "20", ["g1", "g12"]));

    await vi.advanceTimersByTimeAsync(5_000);
    await settle();

    expect(stateSnapshot).toHaveBeenCalledTimes(2);
    expect(get(authoritativeState).sequence).toBe("12");
  });

  it("retries a failed repair while retaining the last good state", async () => {
    vi.useFakeTimers();
    stateSnapshot.mockResolvedValueOnce(snapshot("5", "9", ["g1"]));
    await startStateSync();
    stateSnapshot.mockRejectedValueOnce(new Error("IPC unavailable"));
    deliver(event({ id: "gap", sequence: "8", revision: "13", baseRevision: "12" }));
    await settle();
    expect(get(authoritativeState).sequence).toBe("5");
    expect(get(authoritativeState).repairing).toBe(true);
    stateSnapshot.mockResolvedValueOnce(snapshot("8", "13", ["g1", "g8"]));
    await vi.advanceTimersByTimeAsync(5_000);
    expect(stateSnapshot).toHaveBeenCalledTimes(3);
    expect(get(authoritativeState).sequence).toBe("8");
    expect(get(authoritativeState).repairing).toBe(false);
  });

  it("recovers from a rejected initial snapshot without restarting the consumer", async () => {
    vi.useFakeTimers();
    stateSnapshot.mockRejectedValueOnce(new Error("initial IPC failed"));
    await expect(startStateSync()).rejects.toThrow("initial IPC failed");
    expect(get(authoritativeState).repairing).toBe(true);
    stateSnapshot.mockResolvedValueOnce(snapshot("5", "9", ["g1"]));
    await vi.advanceTimersByTimeAsync(5_000);
    expect(stateSnapshot).toHaveBeenCalledTimes(2);
    expect(get(authoritativeState).games.map((game) => game.id)).toEqual(["g1"]);
    expect(get(authoritativeState).repairing).toBe(false);
  });

  it("ignores a late event from a superseded connection generation", async () => {
    stateSnapshot.mockResolvedValueOnce(snapshot("5", "9", ["g1"]));
    await startStateSync();
    await settle();

    const staleHandler = listeners[0];
    await stopStateSync();

    staleHandler({
      payload: event({ id: "eStale", sequence: "6", revision: "10", baseRevision: "9", addGameId: "gX" }),
    });
    await settle();

    const state = get(authoritativeState);
    expect(state.emitterId).toBeNull();
    expect(state.games).toEqual([]);
    expect(stateSnapshot).toHaveBeenCalledTimes(1);
  });
});
