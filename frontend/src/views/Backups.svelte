<script module lang="ts">
  import type { BackupEntry, BackupView, HistoryView } from "../lib/api";
  import { translate, type Locale, type TranslationVars } from "../lib/i18n/index";

  /** How long a restore attempt waits for backend-published evidence before it is reported as
   *  unverified. Waiting longer would not make the attempt succeed; it only delays the honest
   *  answer. */
  export const RESTORE_VERIFY_TIMEOUT_MS = 12_000;

  /** Kind of stored snapshot. Rust owns the classification; `unknown` stays distinct so an
   *  unrecognised value is never folded into the game-DLL listing. */
  export type BackupKind = "game_dll" | "driver_package" | "unknown";

  /** Availability of the stored bytes as published by the backend.
   *  `unverified` means nobody checked: it is neither a correct snapshot nor an absent one. */
  export type BackupAvailability = "verified_present" | "verified_absent" | "unverified";

  /** Restore verdict. The view presents it and never derives `eligible` from a local file guess. */
  export type RestoreEligibility =
    | { state: "eligible" }
    | { state: "restored" }
    | { state: "blocked"; reasonCode: string }
    | { state: "unknown" };

  /** Result of one restore attempt. A resolved command is not a result: only evidence published
   *  by Rust promotes an attempt to `verified`. */
  export type RestoreOutcome =
    | { state: "verified"; at: string }
    | { state: "failed"; detail: string }
    | { state: "unverified" };

  /** The authoritative backup projection, including the fields phase 10 asks Rust to publish
   *  (`kind`, three-state `availability`, `restore_eligibility`, `last_restore`). They are optional
   *  so this view consumes them as soon as they exist and reports `unverified` / `unknown` until
   *  then, instead of inventing a verdict. */
  export type ProjectedBackup = BackupView & {
    kind?: string;
    availability?: string;
    restore_eligibility?: { eligible: boolean; reason_code?: string | null } | null;
    last_restore?: { verified: boolean; at?: string | null; detail?: string | null } | null;
  };

  /** One presented row: the stored record plus everything the backend published about it. */
  export interface BackupRow {
    id: string;
    entry: BackupEntry;
    view: ProjectedBackup | null;
    kind: BackupKind;
    availability: BackupAvailability;
    eligibility: RestoreEligibility;
    restoredAt: string | null;
  }

  /** Evidence baseline captured when a restore is dispatched, so a record that already existed
   *  can never be mistaken for the result of this attempt. */
  export interface RestoreAttempt {
    backupId: string;
    restoredAtBefore: string | null;
    revisionBefore: string | null;
    lastRestoreBefore?: string | null;
    knownHistoryIds: ReadonlySet<string>;
  }

  function normalisedKind(raw: string | undefined): BackupKind {
    if (raw === "driver_package") return "driver_package";
    if (raw === "game_dll" || raw === "dll") return "game_dll";
    return "unknown";
  }

  /** Snapshot type. The authoritative `kind` wins. The legacy `backup_type` column is also a
   *  backend value, so it is normalised here once instead of being string-tested at each use, and
   *  an unrecognised value stays `unknown`. */
  export function backupKind(entry: BackupEntry, view: ProjectedBackup | undefined): BackupKind {
    const projected = normalisedKind(view?.kind);
    if (projected !== "unknown") return projected;
    if (entry.backup_type === "driver_package") return "driver_package";
    if (entry.backup_type !== "driver_package" && entry.backup_type !== "dll" && entry.backup_type !== undefined) {
      return "unknown";
    }
    return "game_dll";
  }

  /** Availability of the stored bytes, read from the projection only. */
  export function projectAvailability(view: ProjectedBackup | undefined): BackupAvailability {
    if (view === undefined) return "unverified";
    if (
      view.availability === "verified_present" ||
      view.availability === "verified_absent" ||
      view.availability === "unverified"
    ) {
      return view.availability;
    }
    // Two-state contract: `true` is a verified presence check. `false` cannot separate an absent
    // snapshot from one that was never checked, so it stays `unverified`.
    return view.verified_available === true ? "verified_present" : "unverified";
  }

  /** Restore verdict for a row. Without a projection the answer is `unknown`, never "eligible"
   *  and never "blocked". */
  export function projectEligibility(view: ProjectedBackup | undefined): RestoreEligibility {
    if (view === undefined) return { state: "unknown" };
    const projected = view.restore_eligibility;
    if (projected !== undefined && projected !== null) {
      return projected.eligible
        ? { state: "eligible" }
        : { state: "blocked", reasonCode: projected.reason_code ?? "unspecified" };
    }
    // Until Rust publishes the verdict, the fallback uses published facts only and mirrors the
    // backend rule behind `eligible_restorable_backups`.
    const availability = projectAvailability(view);
    if (availability === "verified_absent") return { state: "blocked", reasonCode: "snapshot_absent" };
    if (availability === "unverified") return { state: "unknown" };
    return view.restored_at !== null ? { state: "restored" } : { state: "eligible" };
  }

  /** Whether the primary restore control is offered. Blocked rows keep a disabled control with the
   *  published reason; already-restored rows use the explicit "restore again" affordance. */
  export function restoreOffered(eligibility: RestoreEligibility): boolean {
    return eligibility.state === "eligible" || eligibility.state === "unknown";
  }

  /** Join the stored rows with the authoritative projection. */
  export function projectBackupRows(
    entries: readonly BackupEntry[],
    views: readonly BackupView[],
  ): BackupRow[] {
    const byId = new Map<string, ProjectedBackup>(views.map((view) => [view.id, view as ProjectedBackup]));
    return entries.map((entry) => {
      const view = byId.get(entry.id);
      return {
        id: entry.id,
        entry,
        view: view ?? null,
        kind: backupKind(entry, view),
        availability: projectAvailability(view),
        eligibility: projectEligibility(view),
        // The projection wins when it exists; the stored row is only the pre-snapshot fallback.
        restoredAt: view !== undefined ? view.restored_at : entry.restored_at,
      };
    });
  }

  /** Backend-published evidence about one restore attempt, or `null` while nothing was published.
   *  A resolved `restore_backup` call is not evidence and never reaches this function as one. */
  export function restoreEvidence(
    attempt: RestoreAttempt,
    view: ProjectedBackup | undefined,
    history: readonly HistoryView[],
  ): RestoreOutcome | null {
    const last = view?.last_restore ?? null;
    if (last !== null && view !== undefined && view.revision !== attempt.revisionBefore
      && JSON.stringify(last) !== attempt.lastRestoreBefore) {
      return last.verified
        ? { state: "verified", at: last.at ?? view.restored_at ?? "" }
        : { state: "failed", detail: last.detail ?? "" };
    }
    for (const item of history) {
      if (attempt.knownHistoryIds.has(item.id)) continue;
      if (item.record.backup_id !== attempt.backupId) continue;
      if (item.recovery_outcome === "rollback_failed" || item.record.status === "failed") {
        return { state: "failed", detail: item.record.error ?? "" };
      }
      if (item.recovery_outcome === "rolled_back" && item.record.status === "succeeded") {
        return { state: "verified", at: item.historical_at ?? item.record.created_at };
      }
    }
    return null;
  }

  /** Wording for a blocked restore. An unlisted reason code resolves to the neutral unknown label
   *  rather than to a fabricated explanation. */
  const RESTORE_REASON_FALLBACK_KEYS: Record<string, string> = {
    snapshot_absent: "view.backups.entry.unavailableHint",
    snapshot_unavailable: "view.backups.entry.unavailableHint",
    already_restored: "view.backups.entry.restored",
    unverified: "view.catalog.trust.unverified",
  };

  export function restoreReasonLabel(code: string): { key: string; fallbackKey: string } {
    return {
      key: `view.backups.eligibility.${code}`,
      fallbackKey: RESTORE_REASON_FALLBACK_KEYS[code] ?? "status.unknown",
    };
  }

  /** Open-ended backend reason codes retain a neutral fallback for unknown values. */
  export function messageWithFallback(
    loc: Locale,
    key: string,
    fallbackKey: string,
    vars?: TranslationVars,
  ): string {
    const value = translate(loc, key, vars);
    return value === key ? translate(loc, fallbackKey, vars) : value;
  }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { fly } from "svelte/transition";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { backups, loadBackups, games, showToast, settings, persistUiPreferences, currentView } from "../lib/stores";
  import { pushNotification, makeNotificationEntry } from "../lib/notifications";
  import { restoreBackup, restoreSystemDriver, deleteBackup, openPath, revealPath, type DetectedGame, type BackupsGroupBy } from "../lib/api";
  import { authoritativeState } from "../lib/stateSync";
  import { BACKUPS_GROUP_BYS, BACKUPS_GROUP_BY_DEFAULT } from "../lib/ux";
  import { bestArtSrc } from "../lib/gameArt";
  import {
    featureTitle,
    featureFromFamily,
    familyVendor,
    launcherLabel,
  } from "../lib/labels";
  import BrandMark from "../components/BrandMark.svelte";
  import type { DllFamily } from "../lib/api";
  import { get } from "svelte/store";
  import { t, locale } from "../lib/i18n/index";

  onMount(() => {
    void loadBackups();
  });

  type GroupedBackup = {
    game_id: string;
    name: string;
    game: DetectedGame | null;
    entries: BackupRow[];
    activeCount: number;
    restoredCount: number;
    missingCount: number;
    unverifiedCount: number;
    latestAt: string;
    oldestAt: string;
    sizeBytes: number;
  };

  let query = $state("");
  let expanded = $state<Record<string, boolean>>({});
  /** Games whose cover failed to load, so the row shows the initial instead of a broken image. */
  let thumbErrored = $state<Record<string, boolean>>({});
  let restoringId = $state<string | null>(null);
  let deletingId = $state<string | null>(null);
  let openingPath = $state<string | null>(null);
  let selectedIds = $state<Set<string>>(new Set());
  let bulkRunning = $state<"restore" | "delete" | null>(null);
  /** Outcome of the last restore attempt per row. Failures and unverified attempts stay on screen
   *  until that row is restored again, so a partial result is never swallowed by a toast. */
  let restoreOutcomes = $state<Record<string, RestoreOutcome>>({});

  /** Every stored row joined with the authoritative projection. */
  let rows = $derived(projectBackupRows($backups, $authoritativeState.backups));

  function toggleEntry(id: string): void {
    const next = new Set(selectedIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selectedIds = next;
  }

  function groupSelectionState(g: GroupedBackup): "none" | "some" | "all" {
    let on = 0;
    for (const e of g.entries) if (selectedIds.has(e.id)) on += 1;
    if (on === 0) return "none";
    if (on === g.entries.length) return "all";
    return "some";
  }

  function toggleGroupSelection(g: GroupedBackup, checked: boolean): void {
    const next = new Set(selectedIds);
    for (const e of g.entries) {
      if (checked) next.add(e.id);
      else next.delete(e.id);
    }
    selectedIds = next;
  }

  function clearSelection(): void {
    selectedIds = new Set();
  }

  let gameById = $derived.by<Map<string, DetectedGame>>(() => {
    const m = new Map<string, DetectedGame>();
    for (const g of $games) m.set(g.id, g);
    return m;
  });

  let groupBy: BackupsGroupBy = $derived(
    ($settings?.ui_prefs.backups_group_by ?? BACKUPS_GROUP_BY_DEFAULT) as BackupsGroupBy,
  );

  async function setGroupBy(mode: BackupsGroupBy): Promise<void> {
    if (!$settings || !BACKUPS_GROUP_BYS.includes(mode)) return;
    await persistUiPreferences({ backups_group_by: mode });
  }

  function dateLabel(iso: string): string {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    return d.toLocaleDateString(undefined, { weekday: "short", year: "numeric", month: "short", day: "numeric" });
  }

  // Type comes from the projection through `backupKind`; an unrecognised kind is listed with the
  // game snapshots rather than hidden, but it is never presented as a driver package.
  let dllBackups = $derived(rows.filter((row) => row.kind !== "driver_package"));
  let driverBackups = $derived(rows.filter((row) => row.kind === "driver_package"));

  let driverQuery = $state("");
  let driverClassFilter = $state<string>("all");
  let restoringDriverId = $state<string | null>(null);

  type DriverClassGroup = { deviceClass: string; entries: BackupRow[] };

  let driverClasses = $derived.by<string[]>(() => {
    const set = new Set<string>();
    for (const row of driverBackups) set.add(row.entry.device_class ?? "Driver");
    return Array.from(set).sort((a, b) => a.localeCompare(b));
  });

  let driverGroups = $derived.by<DriverClassGroup[]>(() => {
    const q = driverQuery.trim().toLowerCase();
    const m = new Map<string, BackupRow[]>();
    for (const row of driverBackups) {
      const b = row.entry;
      const cls = b.device_class ?? "Driver";
      if (driverClassFilter !== "all" && cls !== driverClassFilter) continue;
      if (
        q &&
        !(
          cls.toLowerCase().includes(q) ||
          (b.driver_provider ?? "").toLowerCase().includes(q) ||
          b.dll_filename.toLowerCase().includes(q) ||
          (b.hardware_id ?? "").toLowerCase().includes(q) ||
          (b.previous_version ?? "").toLowerCase().includes(q)
        )
      )
        continue;
      (m.get(cls) ?? m.set(cls, []).get(cls)!).push(row);
    }
    return Array.from(m.entries())
      .map(([deviceClass, entries]) => ({
        deviceClass,
        entries: entries.sort((a, b) => b.entry.created_at.localeCompare(a.entry.created_at)),
      }))
      .sort((a, b) => a.deviceClass.localeCompare(b.deviceClass));
  });

  async function doDriverRestore(b: BackupEntry): Promise<void> {
    if (restoringDriverId) return;
    const loc = get(locale);
    const ok = await confirm(
      translate(loc, "view.backups.driver.confirmRollback.body", {
        provider: b.driver_provider ?? translate(loc, "view.backups.driver.thisDriver"),
        deviceClass: (b.device_class ?? translate(loc, "view.backups.driver.driverWord")).toLowerCase(),
        version: b.previous_version ?? "?",
      }),
      {
        title: translate(loc, "view.backups.driver.confirmRollback.title"),
        kind: "warning",
        okLabel: translate(loc, "view.backups.driver.rollBack"),
        cancelLabel: translate(loc, "view.backups.cancel"),
      },
    );
    if (!ok) return;
    restoringDriverId = b.id;
    try {
      const outcome = await restoreSystemDriver(b.id);
      if (outcome.success && outcome.verification === "active_version_verified") {
        showToast(
          "success",
          translate(loc, "view.backups.driver.toastRolledBack", { file: b.dll_filename }) +
            (outcome.reboot_required ? translate(loc, "view.backups.driver.rebootSuffix") : ""),
        );
        void pushNotification(
          makeNotificationEntry(
            "backup_restored",
            translate(loc, "view.backups.driver.notifTitle", {
              deviceClass: b.device_class ?? translate(loc, "view.backups.driver.driverWord"),
            }),
            `${b.driver_provider ?? ""} ${b.dll_filename}`.trim(),
          ),
        ).catch((err) => console.warn("[dlssync] push driver-rollback notification failed:", err));
        await loadBackups();
      } else {
        showToast(outcome.success ? "warning" : "danger", outcome.message);
      }
    } catch (err: unknown) {
      showToast("danger", translate(loc, "view.backups.driver.toastRollbackFailed", { error: String(err) }));
    } finally {
      restoringDriverId = null;
    }
  }

  let grouped = $derived.by<GroupedBackup[]>(() => {
    const m = new Map<string, GroupedBackup>();
    for (const b of dllBackups) {
      const e = b.entry;
      const key = groupBy === "date" ? e.created_at.slice(0, 10) : e.game_id;
      let g = m.get(key);
      if (!g) {
        const det = groupBy === "game" ? gameById.get(e.game_id) ?? null : null;
        const name = groupBy === "date" ? dateLabel(e.created_at) : det?.name ?? e.game_id;
        g = {
          game_id: key,
          name,
          game: det,
          entries: [],
          activeCount: 0,
          restoredCount: 0,
          missingCount: 0,
          unverifiedCount: 0,
          latestAt: e.created_at,
          oldestAt: e.created_at,
          sizeBytes: 0,
        };
        m.set(key, g);
      }
      g.entries.push(b);
      // Every count below reads a backend-published state, never a local file guess.
      if (b.availability === "verified_absent") g.missingCount += 1;
      if (b.availability === "unverified") g.unverifiedCount += 1;
      if (b.restoredAt) g.restoredCount += 1;
      else if (b.eligibility.state === "eligible") g.activeCount += 1;
      if (e.created_at > g.latestAt) g.latestAt = e.created_at;
      if (e.created_at < g.oldestAt) g.oldestAt = e.created_at;
      g.sizeBytes += e.size_bytes ?? 0;
    }
    for (const g of m.values()) {
      g.entries.sort((a, b) => b.entry.created_at.localeCompare(a.entry.created_at));
    }
    return Array.from(m.values()).sort((a, b) => b.latestAt.localeCompare(a.latestAt));
  });

  let filtered = $derived.by<GroupedBackup[]>(() => {
    if (!query.trim()) return grouped;
    const q = query.trim().toLowerCase();
    return grouped
      .map((g) => ({
        ...g,
        entries: g.entries.filter(
          ({ entry: e }) =>
            g.name.toLowerCase().includes(q) ||
            e.dll_filename.toLowerCase().includes(q) ||
            (featureTitle(featureFromFamily(e.dll_family)).toLowerCase().includes(q)) ||
            (e.previous_version ?? "").toLowerCase().includes(q),
        ),
      }))
      .filter((g) => g.entries.length > 0);
  });

  let totalMissing = $derived(dllBackups.filter((b) => b.availability === "verified_absent").length);
  let totalUnverified = $derived(dllBackups.filter((b) => b.availability === "unverified").length);

  /** Published facts captured at dispatch time, so evidence can be attributed to this attempt and
   *  never to an earlier restore of the same snapshot. */
  function beginAttempt(backupId: string): RestoreAttempt {
    const state = get(authoritativeState);
    const view = state.backups.find((b) => b.id === backupId) as ProjectedBackup | undefined;
    return {
      backupId,
      restoredAtBefore: view?.restored_at ?? null,
      revisionBefore: view?.revision ?? null,
      lastRestoreBefore: view?.last_restore ? JSON.stringify(view.last_restore) : null,
      knownHistoryIds: new Set(state.history.map((item) => item.id)),
    };
  }

  /** Wait for Rust to publish what happened to the snapshot. Nothing published inside the window
   *  is reported as unverified — never as a completed restoration. */
  function awaitRestoreEvidence(attempt: RestoreAttempt): Promise<RestoreOutcome> {
    return new Promise((resolve) => {
      let settled = false;
      let stop: (() => void) | null = null;
      let timer: ReturnType<typeof setTimeout> | null = null;
      const finish = (outcome: RestoreOutcome): void => {
        if (settled) return;
        settled = true;
        if (timer !== null) clearTimeout(timer);
        if (stop !== null) stop();
        resolve(outcome);
      };
      const unsubscribe = authoritativeState.subscribe((state) => {
        const view = state.backups.find((b) => b.id === attempt.backupId) as ProjectedBackup | undefined;
        const outcome = restoreEvidence(attempt, view, state.history);
        if (outcome !== null) finish(outcome);
      });
      stop = unsubscribe;
      if (settled) {
        unsubscribe();
        return;
      }
      timer = setTimeout(() => finish({ state: "unverified" }), RESTORE_VERIFY_TIMEOUT_MS);
    });
  }

  /** Dispatch one restore and keep only the outcome the backend published for it. */
  async function runRestore(row: BackupRow): Promise<RestoreOutcome> {
    const attempt = beginAttempt(row.id);
    let outcome: RestoreOutcome;
    try {
      await restoreBackup(row.id);
      outcome = await awaitRestoreEvidence(attempt);
    } catch (err: unknown) {
      outcome = { state: "failed", detail: String(err) };
    }
    restoreOutcomes = { ...restoreOutcomes, [row.id]: outcome };
    return outcome;
  }

  async function doRestore(row: BackupRow): Promise<void> {
    if (restoringId) return;
    const b = row.entry;
    const loc = get(locale);
    restoringId = b.id;
    try {
      const outcome = await runRestore(row);
      if (outcome.state === "verified") {
        showToast("success", translate(loc, "view.backups.toastRestoredFile", { file: b.dll_filename }));
        void pushNotification(
          makeNotificationEntry(
            "backup_restored",
            translate(loc, "view.backups.toastRestoredFile", { file: b.dll_filename }),
            gameById.get(b.game_id)?.name ?? b.game_id,
            { game_id: b.game_id },
          ),
        ).catch((err) => console.warn("[dlssync] push backup-restored notification failed:", err));
      } else if (outcome.state === "failed") {
        showToast("danger", translate(loc, "view.backups.toastRestoreFailed", { error: outcome.detail }));
      } else {
        // The command came back without an error. That is not a restoration, so the row keeps an
        // unverified marker and the toast says so.
        showToast(
          "warning",
          translate(loc, "view.backups.toastRestoreUnverified", {
            file: b.dll_filename,
          }),
        );
      }
      await loadBackups();
    } finally {
      restoringId = null;
    }
  }

  let selectedRows = $derived.by<BackupRow[]>(() => dllBackups.filter((row) => selectedIds.has(row.id)));
  let selectedActiveCount = $derived(selectedRows.filter((row) => restoreOffered(row.eligibility)).length);
  let selectedTotalBytes = $derived(selectedRows.reduce((n, row) => n + (row.entry.size_bytes ?? 0), 0));

  async function bulkRestore(): Promise<void> {
    if (bulkRunning) return;
    const loc = get(locale);
    const targets = selectedRows.filter((row) => restoreOffered(row.eligibility));
    if (targets.length === 0) {
      showToast("info", translate(loc, "view.backups.toastNothingToRestore"));
      return;
    }
    bulkRunning = "restore";
    let ok = 0;
    let fail = 0;
    let unverified = 0;
    for (const row of targets) {
      const outcome = await runRestore(row);
      if (outcome.state === "verified") ok += 1;
      else if (outcome.state === "failed") fail += 1;
      else unverified += 1;
    }
    bulkRunning = null;
    selectedIds = new Set();
    await loadBackups();
    if (ok > 0) {
      void pushNotification(
        makeNotificationEntry(
          "backup_restored",
          translate(loc, "view.backups.toastRestoredCount", { count: ok }),
          fail > 0
            ? translate(loc, "view.backups.toastFailedToRestore", { count: fail })
            : translate(loc, "view.backups.toastAllRestored"),
        ),
      ).catch((err) => console.warn("[dlssync] push backup-restored notification failed:", err));
    }
    // Only verified restorations count as restored; unverified attempts keep their own wording and
    // stay visible on their rows.
    if (fail === 0 && unverified === 0) {
      showToast("success", translate(loc, "view.backups.toastRestoredCount", { count: ok }));
    } else if (fail > 0 && ok === 0 && unverified === 0) {
      showToast("danger", translate(loc, "view.backups.toastRestoreFailedAll", { count: fail }));
    } else if (fail > 0) {
      showToast("warning", translate(loc, "view.backups.toastRestorePartial", { ok, fail }));
    } else {
      showToast(
        "warning",
        translate(loc, "view.backups.toastRestoreUnverifiedCount", {
          count: unverified,
        }),
      );
    }
  }

  async function bulkDelete(): Promise<void> {
    if (bulkRunning) return;
    if (selectedRows.length === 0) return;
    const loc = get(locale);
    const sizeLabel =
      selectedTotalBytes > 0
        ? translate(loc, "view.backups.confirmDeleteMany.sizeSuffix", { size: fmtBytes(selectedTotalBytes) })
        : "";
    const ok = await confirm(
      translate(loc, "view.backups.confirmDeleteMany.body", {
        count: selectedRows.length,
        sizeSuffix: sizeLabel,
      }),
      {
        title: translate(loc, "view.backups.confirmDeleteMany.title"),
        kind: "warning",
        okLabel: translate(loc, "view.backups.deleteCount", { count: selectedRows.length }),
        cancelLabel: translate(loc, "view.backups.cancel"),
      },
    );
    if (!ok) return;
    bulkRunning = "delete";
    let removed = 0;
    let fail = 0;
    for (const { entry: e } of selectedRows) {
      try {
        const outcome = await deleteBackup(e.id);
        if (outcome.file_error) fail += 1;
        else removed += 1;
      } catch {
        fail += 1;
      }
    }
    bulkRunning = null;
    selectedIds = new Set();
    await loadBackups();
    if (fail === 0) showToast("success", translate(loc, "view.backups.toastDeletedCount", { count: removed }));
    else if (removed === 0) showToast("danger", translate(loc, "view.backups.toastDeleteFailedAll", { count: fail }));
    else showToast("warning", translate(loc, "view.backups.toastDeletePartial", { removed, fail }));
  }

  async function doDelete(b: BackupEntry): Promise<void> {
    if (deletingId) return;
    const loc = get(locale);
    const sizeLabel = b.size_bytes
      ? translate(loc, "view.backups.confirmDeleteOne.sizeSuffix", { size: fmtBytes(b.size_bytes) })
      : "";
    const ok = await confirm(
      translate(loc, "view.backups.confirmDeleteOne.body", {
        file: b.dll_filename,
        sizeSuffix: sizeLabel,
      }),
      {
        title: translate(loc, "view.backups.confirmDeleteOne.title"),
        kind: "warning",
        okLabel: translate(loc, "view.backups.delete"),
        cancelLabel: translate(loc, "view.backups.cancel"),
      },
    );
    if (!ok) return;
    deletingId = b.id;
    try {
      const outcome = await deleteBackup(b.id);
      if (outcome.file_error) {
        showToast("warning", translate(loc, "view.backups.toastRowRemovedFileFailed", { error: outcome.file_error }));
      } else {
        showToast("success", translate(loc, "view.backups.toastDeletedFile", { file: b.dll_filename }));
      }
      await loadBackups();
    } catch (err: unknown) {
      showToast("danger", translate(loc, "view.backups.toastDeleteFailed", { error: String(err) }));
    } finally {
      deletingId = null;
    }
  }

  async function revealBackup(b: BackupEntry): Promise<void> {
    if (openingPath) return;
    openingPath = b.id;
    try {
      await revealPath(b.backup_path);
    } catch (err: unknown) {
      showToast("danger", translate(get(locale), "view.backups.toastRevealFailed", { error: String(err) }));
    } finally {
      openingPath = null;
    }
  }

  async function openGameFolder(g: GroupedBackup): Promise<void> {
    const loc = get(locale);
    if (!g.game) {
      showToast("warning", translate(loc, "view.backups.toastInstallPathUnavailable"));
      return;
    }
    try {
      await openPath(g.game.install_dir);
    } catch (err: unknown) {
      showToast("danger", translate(loc, "view.backups.toastOpenFolderFailed", { error: String(err) }));
    }
  }

  function fmtDate(s: string): string {
    const d = new Date(s);
    if (isNaN(d.getTime())) return "—";
    return d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
  }
  function fmtDateShort(s: string): string {
    const d = new Date(s);
    if (isNaN(d.getTime())) return "—";
    return d.toISOString().slice(0, 10);
  }
  function fmtBytes(n: number | null | undefined): string {
    if (n == null || n === 0) return "—";
    const units = ["B", "KB", "MB", "GB", "TB"];
    let i = 0;
    let v = n;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i += 1;
    }
    return `${v < 10 && i > 0 ? v.toFixed(1) : Math.round(v).toString()} ${units[i]}`;
  }

  function expandAll(): void {
    const next: Record<string, boolean> = {};
    for (const g of filtered) next[g.game_id] = true;
    expanded = next;
  }
  function collapseAll(): void {
    expanded = {};
  }
</script>

<header class="view-header backup-header">
  <h1 class="view-title">{$t("view.backups.title")}</h1>
  <details class="backup-actions">
    <summary class="btn btn-ghost btn-sm" aria-label={$t("component.gameDrawer.feature.moreActions")}>
      <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="2"/><circle cx="12" cy="12" r="2"/><circle cx="19" cy="12" r="2"/></svg>
    </summary>
    <div class="backup-action-menu">
      <button class="btn btn-ghost btn-sm" data-testid="nav-journal" onclick={() => currentView.set("journal")}>{$t("view.backups.activityTab")}</button>
      {#if filtered.length > 0}
        <button class="btn btn-ghost btn-sm" onclick={expandAll}>{$t("view.backups.expandAll")}</button>
        <button class="btn btn-ghost btn-sm" onclick={collapseAll}>{$t("view.backups.collapseAll")}</button>
      {/if}
    </div>
  </details>
</header>

{#if rows.length === 0}
  <div class="empty" in:fly={{ y: 8, duration: 240 }}>
    <span class="empty-icon" aria-hidden="true">
      <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="21 8 21 21 3 21 3 8"/><rect x="1" y="3" width="22" height="5" rx="0.5"/><line x1="10" y1="12" x2="14" y2="12"/></svg>
    </span>
    <h3 class="empty-title">{$t("view.backups.emptyTitle")}</h3>
    <p class="section-sub">{$t("view.backups.emptyBody")}</p>
  </div>
{:else}
  {#if dllBackups.length > 0}
    {#if totalMissing > 0 || totalUnverified > 0}
    <div class="hero-meta-strip">

      {#if totalMissing > 0}
        <span class="hero-meta-item hero-meta-warn" title={$t("view.backups.meta.missingHint")}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
          <span class="hero-meta-value">{$t("view.backups.meta.missingCount", { count: totalMissing })}</span>
        </span>
      {/if}
      {#if totalUnverified > 0}
        <span
          class="hero-meta-item"
          data-testid="backups-unverified-total"
          title={translate($locale, "view.backups.meta.unverifiedHint")}
        >
          <span class="hero-meta-label">{translate($locale, "view.backups.meta.unverified")}</span>
          <span class="hero-meta-value">{totalUnverified}</span>
        </span>
      {/if}
    </div>
    {/if}


  {#if selectedIds.size > 0}
    <div class="bulk-bar" in:fly={{ y: -4, duration: 180 }}>
      <span class="bulk-count">{$t("view.backups.bulk.selected", { count: selectedIds.size })}</span>
      {#if selectedTotalBytes > 0}
        <span class="bulk-meta">{fmtBytes(selectedTotalBytes)}</span>
      {/if}
      {#if selectedActiveCount > 0 && selectedActiveCount !== selectedIds.size}
        <span class="bulk-meta">{$t("view.backups.bulk.restorable", { count: selectedActiveCount })}</span>
      {/if}
      <div class="bulk-spacer"></div>
      <button class="btn btn-sm btn-ghost" onclick={clearSelection} disabled={bulkRunning !== null}>{$t("view.backups.bulk.clear")}</button>
      <button class="btn btn-sm btn-accent" onclick={bulkRestore} disabled={selectedActiveCount === 0 || bulkRunning !== null}>
        {#if bulkRunning === "restore"}
          <span class="spin"></span>
          {$t("view.backups.bulk.restoring", { count: selectedActiveCount })}
        {:else}
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 9-9c-2.52 0-4.85.93-6.63 2.46"/><polyline points="3 4 3 9 8 9"/></svg>
          {$t("view.backups.bulk.restore", { count: selectedActiveCount })}
        {/if}
      </button>
      <button class="btn btn-sm btn-danger-ghost" onclick={bulkDelete} disabled={bulkRunning !== null}>
        {#if bulkRunning === "delete"}
          <span class="spin"></span>
          {$t("view.backups.bulk.deleting", { count: selectedIds.size })}
        {:else}
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6M14 11v6"/><path d="M9 6V4a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v2"/></svg>
          {$t("view.backups.bulk.delete", { count: selectedIds.size })}
        {/if}
      </button>
    </div>
  {/if}

  <div class="backup-toolbar">
    <div class="backup-search">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="search-icon"><circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
      <input
        type="search"
        placeholder={$t("view.backups.searchPlaceholder")}
        bind:value={query}
      />
      {#if query}
        <button class="search-clear" onclick={() => (query = "")} aria-label={$t("view.backups.clearSearchAria")}>
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      {/if}
    </div>
    <div class="group-by-toggle" role="group" aria-label={$t("view.backups.groupByAria")}>
      <button class="seg-btn" class:active={groupBy === "game"} onclick={() => void setGroupBy("game")} aria-pressed={groupBy === "game"}>{$t("view.backups.groupBy.game")}</button>
      <button class="seg-btn" class:active={groupBy === "date"} onclick={() => void setGroupBy("date")} aria-pressed={groupBy === "date"}>{$t("view.backups.groupBy.date")}</button>
    </div>
    <span class="toolbar-summary">
      {$t("view.backups.summaryCount", { shown: filtered.reduce((a, g) => a + g.entries.length, 0), count: dllBackups.length })}{filtered.length !== grouped.length ? ` · ${groupBy === "date" ? $t("view.backups.summaryDays", { count: filtered.length }) : $t("view.backups.summaryGames", { count: filtered.length })}` : ""}
    </span>
  </div>

  {#if filtered.length === 0}
    <div class="empty small">
      <p class="section-sub">{$t("view.backups.noMatch")}</p>
      <button class="btn btn-accent" onclick={() => (query = "")}>{$t("view.backups.clearSearch")}</button>
    </div>
  {:else}
    <div class="groups">
      {#each filtered as g, i (g.game_id)}
        {@const groupSel = groupSelectionState(g)}
        {@const monthThis = groupBy === "date" ? g.latestAt.slice(0, 7) : ""}
        {@const monthPrev =
          groupBy === "date" && i > 0 ? filtered[i - 1].latestAt.slice(0, 7) : null}
        {#if groupBy === "date" && monthThis !== monthPrev}
          <h2 class="timeline-month-header">
            {new Date(g.latestAt).toLocaleDateString(undefined, {
              year: "numeric",
              month: "long",
            })}
          </h2>
        {/if}
        <section class="group" in:fly={{ y: 6, duration: 260, delay: 40 + i * 30 }}>
          <div class="group-row">
            <label class="group-check" title={groupSel === "all" ? $t("view.backups.group.deselectAll") : $t("view.backups.group.selectAll")}>
              <input
                type="checkbox"
                checked={groupSel === "all"}
                indeterminate={groupSel === "some"}
                aria-label={groupSel === "all" ? $t("view.backups.group.deselectAll") : $t("view.backups.group.selectAll")}
                onchange={(e) => toggleGroupSelection(g, (e.target as HTMLInputElement).checked)}
              />
              <span class="check-box"></span>
            </label>
          <button
            class="group-head"
            onclick={() => (expanded = { ...expanded, [g.game_id]: !expanded[g.game_id] })}
            aria-expanded={!!expanded[g.game_id]}
          >
            <div class="group-thumb" data-launcher={g.game?.launcher ?? "manual"}>
              <!-- A cover that fails to load must fall back to the initial instead of leaving a
                   broken image element in place. -->
              {#if g.game && bestArtSrc(g.game) && !thumbErrored[g.game_id]}
                <img
                  src={bestArtSrc(g.game)}
                  alt={g.name}
                  loading="lazy"
                  onerror={() => (thumbErrored = { ...thumbErrored, [g.game_id]: true })}
                />
              {:else}
                <span class="thumb-fallback" title={g.game?.art?.state === "unavailable" || g.game?.art?.state === "source_failed" ? $t("component.cover.unavailable") : undefined}>
                  <span aria-hidden={g.game?.art?.state === "unavailable" || g.game?.art?.state === "source_failed" ? "true" : undefined}>{g.name.slice(0, 1).toUpperCase()}</span>
                  {#if g.game?.art?.state === "unavailable" || g.game?.art?.state === "source_failed"}<span class="cover-unavailable-label">{$t("component.cover.unavailable")}</span>{/if}
                </span>
              {/if}
            </div>
            <div class="group-meta">
              <div class="group-name-row">
                <span class="group-name">{g.name}</span>
                {#if g.game}
                  <span class="chip chip-neutral group-launcher">{launcherLabel(g.game.launcher)}</span>
                {/if}
              </div>
              <div class="group-stats">
                <span class="stat-line">{$t("view.backups.group.snapshots", { count: g.entries.length })}</span>
                {#if g.activeCount > 0}<span class="dot"></span><span class="stat-line is-update">{$t("view.backups.group.restorable", { count: g.activeCount })}</span>{/if}
                {#if g.restoredCount > 0}<span class="dot"></span><span class="stat-line is-success">{$t("view.backups.group.restored", { count: g.restoredCount })}</span>{/if}
                {#if g.missingCount > 0}<span class="dot"></span><span class="stat-line is-missing">{$t("view.backups.group.missing", { count: g.missingCount })}</span>{/if}
                {#if g.unverifiedCount > 0}<span class="dot"></span><span class="stat-line">{g.unverifiedCount} {translate($locale, "view.backups.group.unverifiedWord")}</span>{/if}
                {#if g.sizeBytes > 0}<span class="dot"></span><span class="stat-line">{fmtBytes(g.sizeBytes)}</span>{/if}
                <span class="dot"></span><span class="stat-line">{$t("view.backups.group.latest", { date: fmtDateShort(g.latestAt) })}</span>
              </div>
            </div>
            <svg
              class="chevron"
              class:open={expanded[g.game_id]}
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
            ><polyline points="9 18 15 12 9 6"/></svg>
          </button>
          </div>
          {#if expanded[g.game_id]}
            <div class="group-actions">
              {#if g.game}
                <button class="btn btn-sm btn-ghost" onclick={() => openGameFolder(g)} title={$t("view.backups.openInstallFolder")}>
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
                  {$t("view.backups.openInstallFolder")}
                </button>
              {/if}
            </div>
            <ul class="entries">
              {#each g.entries as row (row.id)}
                {@const b = row.entry}
                {@const fSlot = featureFromFamily(b.dll_family)}
                {@const missing = row.availability === "verified_absent"}
                {@const outcome = restoreOutcomes[row.id]}
                <li class="entry" class:restored={row.restoredAt} class:missing class:is-selected={selectedIds.has(b.id)}>
                  <label class="entry-check" title={$t("view.backups.entry.selectHint")}>
                    <input
                      type="checkbox"
                      checked={selectedIds.has(b.id)}
                      aria-label={$t("view.backups.entry.selectAria", { file: b.dll_filename })}
                      onchange={() => toggleEntry(b.id)}
                    />
                    <span class="check-box"></span>
                  </label>
                  <div class="entry-glyph" aria-hidden="true">
                    <BrandMark key={familyVendor(b.dll_family as DllFamily)} fit="wordmark" size={14} showLabel={false} />
                  </div>
                  <div class="entry-main">
                    <div class="entry-head">
                      <span class="entry-title">{featureTitle(fSlot)}</span>
                      {#if row.restoredAt}
                        <span class="chip chip-success small-chip" title={$t("view.backups.entry.restoredHint", { date: fmtDate(row.restoredAt) })}>{$t("view.backups.entry.restored")}</span>
                      {/if}
                      <!-- Availability is whatever Rust published. `unverified` is its own state:
                           it is never drawn as a correct snapshot nor as an absent one. -->
                      {#if missing}
                        <span class="chip chip-danger small-chip" data-testid="backup-availability" data-availability="verified_absent" title={$t("view.backups.entry.missingHint")}>{$t("view.backups.entry.snapshotMissing")}</span>
                      {:else if row.availability === "unverified"}
                        <span class="chip chip-neutral small-chip" data-testid="backup-availability" data-availability="unverified" title={translate($locale, "view.backups.entry.unverifiedHint")}>{translate($locale, "view.backups.entry.unverified")}</span>
                      {:else if !row.restoredAt}
                        <span class="chip chip-update small-chip" data-testid="backup-availability" data-availability="verified_present" title={$t("view.backups.entry.activeHint")}>{$t("view.backups.entry.activeBackup")}</span>
                      {/if}
                      {#if row.eligibility.state === "blocked"}
                        {@const reason = restoreReasonLabel(row.eligibility.reasonCode)}
                        <span class="chip chip-neutral small-chip entry-reason" data-testid="backup-ineligible-reason" data-reason={row.eligibility.reasonCode}>{messageWithFallback($locale, reason.key, reason.fallbackKey)}</span>
                      {/if}
                    </div>
                    <div class="entry-meta">
                      <span class="file">{b.dll_filename}</span>
                      <span class="sep">·</span>
                      <span>v{b.previous_version ?? "?"}</span>
                      <span class="sep">·</span>
                      <span class:is-missing={missing}>{missing ? $t("view.backups.entry.gone") : fmtBytes(b.size_bytes)}</span>
                      <span class="sep">·</span>
                      <span title={b.created_at}>{fmtDate(b.created_at)}</span>
                    </div>
                    <details class="entry-details"><summary>{$t("view.backups.fileDetails")}</summary><code>{b.original_path}</code><code>SHA-256: {b.previous_sha256 ?? "—"}</code></details>
                    <!-- A failed or unverified attempt stays on the row until the next attempt. -->
                    {#if outcome !== undefined && outcome.state !== "verified"}
                      <div class="entry-outcome" data-testid="backup-restore-outcome" data-outcome={outcome.state}>
                        {#if outcome.state === "failed"}
                          {$t("view.backups.toastRestoreFailed", { error: outcome.detail })}
                        {:else}
                          {translate($locale, "view.backups.toastRestoreUnverified", { file: b.dll_filename })}
                        {/if}
                      </div>
                    {/if}
                  </div>
                  <div class="entry-actions">
                    <button
                      class="btn btn-sm btn-ghost"
                      onclick={() => revealBackup(b)}
                      title={missing ? $t("view.backups.entry.revealMissing") : $t("view.backups.entry.reveal")}
                      disabled={openingPath === b.id}
                    >
                      {#if openingPath === b.id}
                        <span class="spin"></span>
                      {:else}
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>
                      {/if}
                    </button>
                    <button
                      class="btn btn-sm btn-ghost btn-danger-ghost"
                      onclick={() => doDelete(b)}
                      title={$t("view.backups.entry.deleteHint")}
                      disabled={deletingId === b.id}
                    >
                      {#if deletingId === b.id}
                        <span class="spin"></span>
                      {:else}
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6M14 11v6"/><path d="M9 6V4a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v2"/></svg>
                      {/if}
                    </button>
                    {#if row.eligibility.state === "blocked"}
                      {@const reason = restoreReasonLabel(row.eligibility.reasonCode)}
                      <button
                        class="btn btn-sm btn-ghost"
                        data-testid="backup-restore"
                        disabled
                        title={messageWithFallback($locale, reason.key, reason.fallbackKey)}
                      >
                        {$t("view.backups.entry.unavailable")}
                      </button>
                    {:else if row.restoredAt}
                      <button
                        class="btn btn-sm btn-ghost"
                        data-testid="backup-restore"
                        disabled={restoringId === b.id}
                        onclick={() => doRestore(row)}
                        title={$t("view.backups.entry.restoreAgainHint")}
                      >
                        {#if restoringId === b.id}
                          <span class="spin"></span>
                          {$t("view.backups.entry.restoring")}
                        {:else}
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 9-9c-2.52 0-4.85.93-6.63 2.46"/><polyline points="3 4 3 9 8 9"/></svg>
                          {$t("view.backups.entry.restoreAgain")}
                        {/if}
                      </button>
                    {:else}
                      <button
                        class="btn btn-sm btn-accent"
                        data-testid="backup-restore"
                        disabled={restoringId === b.id}
                        onclick={() => doRestore(row)}
                        title={$t("view.backups.entry.restoreHint")}
                      >
                        {#if restoringId === b.id}
                          <span class="spin"></span>
                          {$t("view.backups.entry.restoring")}
                        {:else}
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 9-9c-2.52 0-4.85.93-6.63 2.46"/><polyline points="3 4 3 9 8 9"/></svg>
                          {$t("view.backups.entry.restore")}
                        {/if}
                      </button>
                    {/if}
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      {/each}
    </div>
  {/if}
  {/if}

  {#if driverBackups.length > 0}
    <section class="driver-section" in:fly={{ y: 6, duration: 220 }}>
      <div class="section-head driver-head">
        <h2 class="section-title">{$t("view.backups.driver.sectionTitle")}</h2>
        <span class="section-count">{driverBackups.length}</span>
      </div>
      <p class="section-sub driver-sub">
        {$t("view.backups.driver.sectionSub")}
      </p>

      <div class="driver-toolbar">
        <div class="backup-search">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="search-icon"><circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
          <input type="search" placeholder={$t("view.backups.driver.searchPlaceholder")} bind:value={driverQuery} />
          {#if driverQuery}
            <button class="search-clear" onclick={() => (driverQuery = "")} aria-label={$t("view.backups.clearSearchAria")}>
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
            </button>
          {/if}
        </div>
        {#if driverClasses.length > 1}
          <div class="pills" role="group" aria-label={$t("view.backups.driver.filterAria")}>
            <button class="pill" class:active={driverClassFilter === "all"} onclick={() => (driverClassFilter = "all")}>{$t("view.backups.driver.filterAll")}</button>
            {#each driverClasses as c (c)}
              <button class="pill" class:active={driverClassFilter === c} onclick={() => (driverClassFilter = c)}>{c}</button>
            {/each}
          </div>
        {/if}
      </div>

      {#if driverGroups.length === 0}
        <div class="empty small">
          <p class="section-sub">{$t("view.backups.driver.noMatch")}</p>
        </div>
      {:else}
        <div class="groups">
          {#each driverGroups as dg (dg.deviceClass)}
            <section class="group">
              <div class="driver-group-head">
                <span class="driver-class">{dg.deviceClass}</span>
                <span class="driver-class-count">{dg.entries.length}</span>
              </div>
              <ul class="entries">
                {#each dg.entries as row (row.id)}
                  {@const b = row.entry}
                  <li class="entry driver-entry" class:restored={row.restoredAt}>
                    <div class="entry-glyph" aria-hidden="true">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="4" width="16" height="16" rx="2"/><rect x="9" y="9" width="6" height="6"/><line x1="9" y1="1" x2="9" y2="4"/><line x1="15" y1="1" x2="15" y2="4"/><line x1="9" y1="20" x2="9" y2="23"/><line x1="15" y1="20" x2="15" y2="23"/><line x1="20" y1="9" x2="23" y2="9"/><line x1="20" y1="14" x2="23" y2="14"/><line x1="1" y1="9" x2="4" y2="9"/><line x1="1" y1="14" x2="4" y2="14"/></svg>
                    </div>
                    <div class="entry-main">
                      <div class="entry-head">
                        <span class="entry-title">{b.driver_provider ?? $t("view.backups.driver.driverWord")}</span>
                        {#if row.restoredAt}
                          <span class="chip chip-success small-chip" title={$t("view.backups.driver.rolledBackHint", { date: fmtDate(row.restoredAt) })}>{$t("view.backups.driver.rolledBack")}</span>
                        {:else}
                          <span class="chip chip-update small-chip" title={$t("view.backups.driver.snapshotHint")}>{$t("view.backups.driver.snapshot")}</span>
                        {/if}
                        {#if row.availability === "verified_absent"}
                          <span class="chip chip-danger small-chip" data-testid="backup-availability" data-availability="verified_absent" title={$t("view.backups.entry.missingHint")}>{$t("view.backups.entry.snapshotMissing")}</span>
                        {/if}
                      </div>
                      <div class="entry-meta">
                        <span class="file">{b.dll_filename}</span>
                        <span class="sep">·</span>
                        <span>v{b.previous_version ?? "?"}</span>
                        <span class="sep">·</span>
                        <span class="truncate hwid" title={b.hardware_id ?? ""}>{b.hardware_id ?? "—"}</span>
                        <span class="sep">·</span>
                        <span title={b.created_at}>{fmtDate(b.created_at)}</span>
                      </div>
                    </div>
                    <div class="entry-actions">
                      <button
                        class="btn btn-sm btn-ghost"
                        onclick={() => revealBackup(b)}
                        title={$t("view.backups.driver.revealHint")}
                        disabled={openingPath === b.id}
                      >
                        {#if openingPath === b.id}
                          <span class="spin"></span>
                        {:else}
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>
                        {/if}
                      </button>
                      <button
                        class="btn btn-sm btn-accent"
                        disabled={restoringDriverId === b.id}
                        onclick={() => doDriverRestore(b)}
                        title={$t("view.backups.driver.rollBackHint")}
                      >
                        {#if restoringDriverId === b.id}
                          <span class="spin"></span>
                          {$t("view.backups.driver.rollingBack")}
                        {:else}
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 9-9c-2.52 0-4.85.93-6.63 2.46"/><polyline points="3 4 3 9 8 9"/></svg>
                          {$t("view.backups.driver.rollBack")}
                        {/if}
                      </button>
                    </div>
                  </li>
                {/each}
              </ul>
            </section>
          {/each}
        </div>
      {/if}
    </section>
  {/if}
{/if}

<style>
  .backup-header { align-items: center; margin-bottom: 20px; }
  .backup-actions { position: relative; margin-left: auto; }
  .backup-actions > summary { list-style: none; cursor: pointer; }
  .backup-actions > summary::-webkit-details-marker { display: none; }
  .backup-action-menu { position: absolute; right: 0; top: calc(100% + 6px); z-index: 20; min-width: 170px; padding: 6px; display: grid; background: var(--bg-elevated); border: 1px solid var(--border); border-radius: var(--radius-md); box-shadow: 0 8px 24px #0002; }
  .backup-action-menu .btn { justify-content: flex-start; }

  .view-header {
    display: flex;
    align-items: end;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 20px;
    flex-wrap: wrap;
  }
  
  
  .empty {
    padding: clamp(56px, 9vh, 96px) 24px;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    color: var(--text-muted);
  }
  .empty.small {
    padding: 40px 24px;
    background: var(--bg-card);
    border: 1px dashed var(--border-strong);
    border-radius: var(--radius-lg);
  }
  .empty-icon {
    width: 64px;
    height: 64px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-2xl);
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    color: var(--text-secondary);
    margin-bottom: var(--space-2);
  }
  .empty.small :global(svg) { opacity: 0.4; margin-bottom: 8px; }
  .empty-title { font-size: var(--fs-lg); font-weight: 700; color: var(--text-primary); letter-spacing: var(--letter-tight); }
  .empty .section-sub { max-width: 440px; line-height: var(--lh-normal); }

  














  .hero-meta-strip {
    margin-top: var(--space-4);
    padding-top: var(--space-4);
    border-top: 1px solid var(--border);
    display: flex;
    align-items: stretch;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .hero-meta-item {
    display: inline-flex;
    flex-direction: column;
    gap: 3px;
    padding: 7px 14px;
    border-radius: var(--radius-md);
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    min-width: 0;
  }
  .hero-meta-label {
    font-size: var(--fs-2xs);
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: var(--letter-wider);
    font-weight: 700;
    white-space: nowrap;
  }
  .hero-meta-value {
    font-size: var(--fs-sm);
    color: var(--text-primary);
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  
  .hero-meta-warn {
    flex-direction: row;
    align-items: center;
    gap: 7px;
    background: var(--danger-dim);
    border-color: transparent;
    color: var(--danger);
    cursor: help;
  }
  .hero-meta-warn svg { display: block; flex-shrink: 0; }
  .hero-meta-warn .hero-meta-value { color: var(--danger); }

  
  
  
  
  
  
  
  

  .backup-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 14px;
    padding-bottom: 14px;
    border-bottom: 1px solid var(--border);
    flex-wrap: wrap;
  }
  .group-by-toggle {
    display: inline-flex;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 2px;
    gap: 2px;
  }
  .group-by-toggle .seg-btn {
    display: inline-flex;
    align-items: center;
    padding: 5px 12px;
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font-size: var(--fs-xs);
    font-weight: 600;
    background: transparent;
    border: none;
    transition: background var(--dur-fast) var(--ease), color var(--dur-fast) var(--ease);
  }
  .group-by-toggle .seg-btn:hover { color: var(--text-primary); background: var(--bg-elevated); }
  .group-by-toggle .seg-btn.active { background: var(--accent-dim); color: var(--accent); }
  .group-by-toggle .seg-btn:focus-visible { outline: none; box-shadow: var(--shadow-ring); }
  .backup-search { position: relative; flex: 1; max-width: 520px; display: flex; align-items: center; }
  .backup-search input {
    width: 100%;
    padding: 9px 34px 9px 34px;
    border-radius: var(--radius-full);
    font-size: var(--fs-sm);
    background: var(--bg-input);
    border: 1px solid var(--border);
  }
  .backup-search input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-dim); }
  .backup-search .search-icon { position: absolute; left: 12px; color: var(--text-muted); pointer-events: none; }
  .search-clear {
    position: absolute;
    right: 8px;
    width: 22px;
    height: 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    border-radius: var(--radius-full);
  }
  .search-clear:hover { color: var(--text-primary); background: var(--bg-elevated); }
  .toolbar-summary { font-size: var(--fs-xs); color: var(--text-muted); font-variant-numeric: tabular-nums; }

  .groups { display: flex; flex-direction: column; gap: var(--space-2); }
  .group {
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    overflow: hidden;
    transition: border-color var(--dur-fast) var(--ease), box-shadow var(--dur-fast) var(--ease);
  }
  .group:hover { border-color: var(--border-hover); box-shadow: var(--shadow-xs); }
  .group-head {
    display: grid;
    grid-template-columns: 92px 1fr auto;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-3) var(--space-4);
    background: transparent;
    border: none;
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
    transition: background var(--dur-fast) var(--ease);
  }
  .group-head:hover { background: var(--bg-card-hover); }
  .group-head:focus-visible { outline: none; box-shadow: inset 0 0 0 2px var(--accent-dim); }
  .group-thumb {
    width: 92px;
    aspect-ratio: 16 / 9;
    border-radius: var(--radius-md);
    overflow: hidden;
    background: var(--bg-art-fallback);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    position: relative;
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .group-thumb::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, rgba(0,0,0,0) 60%, rgba(0,0,0,0.45) 100%);
    pointer-events: none;
  }
  .group-thumb img { width: 100%; height: 100%; object-fit: cover; }
  .thumb-fallback {
    font-size: var(--fs-md);
    font-weight: 700;
    color: var(--launcher-accent, var(--accent));
    opacity: 0.7;
    position: relative;
  }
  .cover-unavailable-label {
    position: absolute;
    bottom: 3px;
    left: 50%;
    transform: translateX(-50%);
    max-width: calc(100% - 6px);
    padding: 1px 4px;
    border-radius: var(--radius-full);
    background: color-mix(in oklab, var(--bg-elevated) 88%, transparent);
    color: var(--text-secondary);
    font-size: 8px;
    font-weight: 600;
    line-height: 1.1;
    text-align: center;
    white-space: nowrap;
  }
  .group-meta { min-width: 0; }
  .group-name-row { display: flex; align-items: center; gap: 8px; margin-bottom: 5px; }
  .group-name {
    font-size: var(--fs-md);
    font-weight: 700;
    letter-spacing: var(--letter-tight);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .group-launcher { font-size: var(--fs-2xs); padding: 1px 7px; flex-shrink: 0; }
  .group-stats { display: flex; align-items: center; gap: 7px; font-size: var(--fs-xs); color: var(--text-muted); flex-wrap: wrap; }
  .group-stats :global(strong) { color: var(--text-secondary); font-weight: 600; }
  .stat-line.is-update { color: var(--update); }
  .stat-line.is-success { color: var(--success); }
  .stat-line.is-missing { color: var(--danger); }
  .group-stats .dot {
    width: 3px;
    height: 3px;
    border-radius: 50%;
    background: currentColor;
    opacity: 0.4;
  }
  .chevron { transition: transform 0.2s var(--ease); color: var(--text-muted); }
  .chevron.open { transform: rotate(90deg); color: var(--accent); }

  .group-actions {
    padding: 8px 16px;
    border-top: 1px solid var(--border);
    background: var(--bg-input);
    display: flex;
    gap: 8px;
  }
  .entries { list-style: none; padding: 0; margin: 0; }
  .entry {
    display: grid;
    grid-template-columns: 24px 64px minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--border);
    transition: background var(--dur-fast) var(--ease), box-shadow var(--dur-fast) var(--ease);
    position: relative;
  }
  .entry.is-selected { background: var(--accent-soft); }
  .entry.is-selected::before {
    content: "";
    position: absolute;
    inset: 0 auto 0 0;
    width: 2px;
    background: var(--accent);
  }
  .entry-check, .group-check {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    user-select: none;
  }
  .entry-check input, .group-check input { position: absolute; opacity: 0; pointer-events: none; }
  .entry-check .check-box, .group-check .check-box {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    border: 1.5px solid var(--border-strong);
    background: var(--bg-input);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    transition: background 0.12s var(--ease), border-color 0.12s var(--ease);
  }
  .entry-check input:checked + .check-box, .group-check input:checked + .check-box {
    background: var(--accent);
    border-color: var(--accent);
  }
  .entry-check input:checked + .check-box::after, .group-check input:checked + .check-box::after {
    content: "";
    width: 8px;
    height: 4px;
    border-left: 2px solid var(--accent-fg);
    border-bottom: 2px solid var(--accent-fg);
    transform: translate(0, -1px) rotate(-45deg);
  }
  .entry-check input:indeterminate + .check-box, .group-check input:indeterminate + .check-box {
    background: var(--accent);
    border-color: var(--accent);
  }
  .entry-check input:indeterminate + .check-box::after, .group-check input:indeterminate + .check-box::after {
    content: "";
    width: 8px;
    height: 2px;
    background: var(--accent-fg);
    border-radius: 1px;
  }

  .group-row {
    display: grid;
    grid-template-columns: 36px 1fr;
    align-items: stretch;
  }
  .group-check { padding-left: 14px; padding-right: 0; }

  .bulk-bar {
    position: sticky;
    top: 0;
    z-index: 10;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 16px;
    margin-bottom: 12px;
    background: var(--accent-soft);
    border: 1px solid var(--accent);
    border-radius: var(--radius-lg);
    box-shadow: 0 4px 14px rgba(0,0,0,0.25);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
  }
  @supports not ((backdrop-filter: blur(1px)) or (-webkit-backdrop-filter: blur(1px))) {
    .bulk-bar { background: var(--bg-elevated); }
  }
  @media (prefers-reduced-transparency: reduce) {
    .bulk-bar { background: var(--bg-elevated); }
  }
  .bulk-count {
    font-size: var(--fs-sm);
    font-weight: 700;
    color: var(--accent);
    font-variant-numeric: tabular-nums;
  }
  .bulk-meta { font-size: var(--fs-xs); color: var(--text-muted); font-variant-numeric: tabular-nums; }
  .bulk-spacer { flex: 1; }
  .entry:hover { background: var(--bg-card-hover); }
  .entry-glyph {
    width: 34px;
    height: 34px;
    border-radius: var(--radius-md);
    background: var(--accent-dim);
    color: var(--accent);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .entry.restored .entry-glyph { background: var(--success-dim); color: var(--success); }
  .entry.missing .entry-glyph { background: var(--danger-dim); color: var(--danger); opacity: 0.75; }
  .entry.missing .entry-title { color: var(--text-secondary); }
  .entry-meta .is-missing { color: var(--danger); font-weight: 600; }
  .entry-main { min-width: 0; }
  .entry-head { display: flex; align-items: center; gap: 8px; margin-bottom: 4px; }
  .entry-title { font-size: var(--fs-sm); font-weight: 600; color: var(--text-primary); letter-spacing: var(--letter-tight); }
  .small-chip { padding: 1px 7px; font-size: var(--fs-2xs); letter-spacing: 0.04em; }
  .entry-meta { font-size: var(--fs-xs); color: var(--text-muted); display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
  .entry-meta .file { color: var(--text-secondary); }
  .entry-meta .sep { opacity: 0.4; }

  .entry-outcome {
    margin-top: 6px;
    font-size: var(--fs-xs);
    line-height: var(--lh-snug);
    color: var(--text-secondary);
  }
  .entry-outcome[data-outcome="failed"] { color: var(--danger); }
  .entry-reason { font-weight: 600; }
  .entry.restored .entry-main { opacity: 0.7; }
  .entry-actions { display: inline-flex; gap: 6px; flex-shrink: 0; }

  .spin { width: 11px; height: 11px; border: 2px solid currentColor; border-top-color: transparent; border-radius: 50%; animation: spin 0.7s linear infinite; display: inline-block; }
  @keyframes spin { to { transform: rotate(360deg); } }

  .driver-section { margin-top: 28px; }
  .driver-head { margin-bottom: 4px; }
  .driver-sub { max-width: 640px; margin-bottom: 14px; }
  .driver-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 14px;
    flex-wrap: wrap;
  }
  .driver-group-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-input);
  }
  .driver-class {
    font-size: var(--fs-xs);
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: var(--letter-wider);
    color: var(--text-secondary);
  }
  .driver-class-count {
    font-size: var(--fs-2xs);
    font-weight: 600;
    color: var(--text-muted);
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius-full);
    padding: 0 7px;
    font-variant-numeric: tabular-nums;
  }
  .entry-meta .hwid { max-width: 320px; display: inline-block; vertical-align: bottom; }
  .entry.driver-entry { grid-template-columns: 34px 1fr auto; }

  @media (max-width: 1080px) {

  }
  @media (max-width: 720px) {


    .group-head { grid-template-columns: 64px 1fr auto; }
    .group-thumb { width: 64px; }
    .entry {
      grid-template-columns: 24px 30px 1fr;
      row-gap: var(--space-2);
    }
    .entry-glyph { width: 30px; height: 30px; }
    .entry-actions {
      grid-column: 1 / -1;
      justify-content: flex-end;
      flex-wrap: wrap;
    }
    .entry.driver-entry { grid-template-columns: 30px 1fr; }
  }

  @media (prefers-reduced-motion: reduce) {
    .group, .group-head, .entry, .chevron { transition: none; }
  }

  .timeline-month-header {
    font-size: var(--fs-xs);
    font-weight: 700;
    letter-spacing: var(--letter-wider);
    text-transform: uppercase;
    color: var(--text-muted);
    margin: var(--space-5) 0 var(--space-2);
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
    font-variant-numeric: tabular-nums;
  }
  .timeline-month-header:first-child {
    margin-top: 0;
    padding-top: 0;
    border-top: none;
  }

  .groups .entry { padding: 20px; gap: 16px; align-items: start; }
  .groups .entry-glyph { display: flex; width: 64px; min-height: 36px; align-items: center; justify-content: center; background: none; border: none; }
  .groups .entry-head { gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 8px; }
  .groups .entry-title { font-size: 15px; line-height: 1.4; }
  .groups .entry-meta { font-size: 12px; line-height: 1.6; gap: 6px 8px; }
  .groups .entry-meta .file { font-family: var(--font-mono); }
  .groups .entry-actions { padding-top: 2px; }
  .groups .small-chip { text-transform: none; letter-spacing: 0; font-size: 11px; }
  .entry-details { margin-top: 8px; font-size: 12px; color: var(--text-muted); }
  .entry-details summary { cursor: pointer; width: fit-content; }
  .entry-details code { display: block; margin-top: 8px; font-size: 11px; line-height: 1.5; white-space: normal; overflow-wrap: anywhere; }
  .groups .entry:not(.driver-entry) > .entry-main { grid-column: 3; min-width: 0; }
  .groups .entry:not(.driver-entry) > .entry-actions { grid-column: 4; }
  .groups .entry-meta { display: flex; flex-wrap: wrap; }
  .groups .entry { padding: 18px 20px; }
  .entry-title { overflow-wrap: anywhere; }
  .entry-meta > span { white-space: normal; overflow-wrap: anywhere; }
  @container workspace (max-width: 760px) {
    .groups .entry:not(.driver-entry) { grid-template-columns: 24px 64px minmax(0, 1fr); }
    .groups .entry:not(.driver-entry) > .entry-actions { grid-column: 3; justify-content: flex-start; flex-wrap: wrap; }
  }

  
</style>
