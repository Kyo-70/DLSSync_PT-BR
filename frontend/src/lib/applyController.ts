import { get } from "svelte/store";
import { classifyApplyError } from "./applyErrorClass";
import {
  applyUpdateBatch,
  previewUpdatePlan,
  applyStreamlineSet,
  applyDllSet,
  cancelApply as cancelApplyApi,
  cancelAllApplies as cancelAllAppliesApi,
  type ApplyRequest,
  type ApplyBatchResult,
  type StreamlineSetResult,
  type DllRecord,
  type UpdatePlan,
} from "./api";
import { familyMeta } from "./familyMeta";
import {
  activeApplies,
  games,
  downloadProgressByGroup,
  formatError,
  showToast,
  type ApplyTracker,
  type Toast,
} from "./stores";
import { familyVendor, familyCatalogKey } from "./labels";
import { translate, locale } from "./i18n/index";
import { notifyApplySuccess } from "./community";
import type { LauncherKind } from "./api";
import { getCatalogStatus } from "./api";
import type { ReviewedUpdatePlan } from "../features/update-plan/model";

export interface ApplyTarget {
  game_id: string;
  game_label: string;
  record: DllRecord;
  target_version: string;
  /** Exact catalog family returned by a verified Rust plan. */
  catalog_family?: string;
}

export interface DispatchOptions {
  actor?: "gui" | "background";
  /** Ended attempts to replace only after a validated preview is ready to start. */
  supersedes?: ApplyTracker[];
  showModal?: () => void;
  toast?: (kind: Toast["kind"], message: string) => void;
  reviewPlan?: (targets: ApplyTarget[]) => Promise<ReviewedUpdatePlan | null>;
}

const DEFAULT_TOAST = (kind: Toast["kind"], message: string): void => showToast(kind, message);
const ENDED_TRACKER_TTL_MS = 5 * 60 * 1000;
let preparingDispatch = false;

/// True when any tracker in the store is still running (no `ended_at`). Mirrors the
/// `isApplyInflight` guard used by the background daemon so a second dispatch never
/// clobbers an in-flight batch's trackers.
export function isApplyInflight(): boolean {
  return preparingDispatch || Object.values(get(activeApplies)).some((t) => t.ended_at === null);
}

/// Drops trackers that finished more than `ENDED_TRACKER_TTL_MS` ago plus any
/// download-progress entry whose group no longer backs a live tracker. Runs at
/// every new dispatch so a long tray session with daily auto-applies cannot
/// accrete state forever.
export function pruneEndedApplyState(now = Date.now()): void {
  const cutoff = now - ENDED_TRACKER_TTL_MS;
  activeApplies.update((m) => {
    const next: Record<string, ApplyTracker> = {};
    for (const [id, tracker] of Object.entries(m)) {
      if (tracker.ended_at === null || tracker.ended_at > cutoff) next[id] = tracker;
    }
    return next;
  });
  const liveGroups = new Set(
    Object.values(get(activeApplies))
      .map((t) => t.group_id)
      .filter((g) => g !== ""),
  );
  downloadProgressByGroup.update((m) => {
    const next: typeof m = {};
    for (const [group, state] of Object.entries(m)) {
      if (liveGroups.has(group)) next[group] = state;
    }
    return next;
  });
}

export async function dispatchApply(
  targets: ApplyTarget[],
  opts: DispatchOptions = {},
): Promise<ApplyBatchResult | null> {
  const loc = get(locale);
  if (targets.length === 0) {
    (opts.toast ?? DEFAULT_TOAST)("warning", translate(loc, "toast.nothingSelected"));
    return null;
  }
  if (isApplyInflight()) {
    (opts.toast ?? DEFAULT_TOAST)("warning", translate(loc, "toast.applyInProgress"));
    return null;
  }
  targets = [...new Map(targets.map((target) => [targetIdentity(target.game_id, target.record.path), target])).values()];
  let reviewed: ReviewedUpdatePlan | null;
  preparingDispatch = true;
  try {
    if (opts.reviewPlan) {
      reviewed = await opts.reviewPlan(targets);
    } else {
      const plan = await previewUpdatePlan(buildApplyRequests(targets));
      reviewed = { targets, catalogGeneratedAt: plan.catalog_generated_at, plan };
    }
    if (!reviewed) return null;
    if (reviewed.plan) reviewed.targets = targetsFromPlan(reviewed.plan);
    const currentCatalog = await getCatalogStatus();
    if (currentCatalog.provenance.generated_at !== reviewed.catalogGeneratedAt) {
      (opts.toast ?? DEFAULT_TOAST)("warning", translate(loc, "toast.updatePlanStale"));
      return null;
    }
  } catch (error) {
    (opts.toast ?? DEFAULT_TOAST)("danger", friendlyApplyError(error));
    return null;
  } finally {
    preparingDispatch = false;
  }
  targets = reviewed.targets;
  pruneEndedApplyState();
  const { trackers, requests } = prepareApply(targets);
  activeApplies.update((m) => {
    const next = { ...m };
    const started = new Set(targets.map((t) => targetIdentity(t.game_id, t.record.path)));
    const retried = new Set((opts.supersedes ?? []).map((t) => targetIdentity(t.game_id, t.dll_path)));
    for (const [id, tracker] of Object.entries(next)) {
      const identity = targetIdentity(tracker.game_id, tracker.dll_path);
      if (tracker.ended_at !== null && started.has(identity) && retried.has(identity) &&
          (tracker.stage === "failed" || tracker.stage === "cancelled")) delete next[id];
    }
    return { ...next, ...trackers };
  });
  opts.showModal?.();
  const toast = opts.toast ?? DEFAULT_TOAST;
  const uniqueGames = new Set(targets.map((t) => t.game_id)).size;
  toast(
    "info",
    translate(loc, "toast.queuedAcross", {
      count: targets.length,
      updates: translate(loc, "toast.queuedUpdates", { count: targets.length }),
      games: translate(loc, "toast.queuedGames", { count: uniqueGames }),
    }),
  );
  try {
    const result = await applyUpdateBatch({ items: requests, plan: reviewed.plan, actor: opts.actor });
    annotateOutcomes(result);
    notifyApplySuccess(result.outcomes.filter((o) => o.success).length);
    return result;
  } catch (err: unknown) {
    const msg = friendlyApplyError(err);
    failAllTrackers(trackers, formatError(err));
    toast("danger", translate(loc, "toast.batchApplyFailed", { msg }));
    return null;
  }
}

function friendlyApplyError(error: unknown): string {
  const message = formatError(error);
  if (/plan.*stale|changed since review|catalog revision changed|installed bytes changed/i.test(message)) {
    return translate(get(locale), "toast.updatePlanStale");
  }
  return message;
}

/** Rust supplies the complete set, including required members the user did not select. */
function targetsFromPlan(plan: UpdatePlan): ApplyTarget[] {
  return plan.items.filter((item) => item.selected).map((item) => {
    if (!familyMeta(item.family)) throw new Error(`Unknown component family: ${item.family}`);
    return {
      game_id: item.game_id,
      game_label: item.game_name,
      target_version: item.target_version,
      catalog_family: item.family,
      record: {
        path: item.dll_path,
        family: item.family as DllRecord["family"],
        current_version: item.current_version,
        sha256: item.trust.observed_sha256,
        file_description: null,
      },
    };
  });
}

async function prepareSetTargets(targets: ApplyTarget[], toast: typeof DEFAULT_TOAST): Promise<ApplyTarget[] | null> {
  preparingDispatch = true;
  try {
    return targetsFromPlan(await previewUpdatePlan(buildApplyRequests(targets)));
  } catch (error) {
    toast("danger", friendlyApplyError(error));
    return null;
  } finally {
    preparingDispatch = false;
  }
}

export function buildApplyRequests(targets: ApplyTarget[]): ApplyRequest[] {
  return targets.map((t) => ({
      apply_id: crypto.randomUUID(),
      game_id: t.game_id,
      install_dir: get(games).find((game) => game.id === t.game_id)?.install_dir ?? null,
      game_label: t.game_label,
      dll_path: t.record.path,
      vendor: familyVendor(t.record.family),
      family: t.catalog_family ?? familyCatalogKey(t.record.family),
      target_version: t.target_version,
      observed_sha256: t.record.sha256 ?? null,
  }));
}

function prepareApply(targets: ApplyTarget[]): {
  trackers: Record<string, ApplyTracker>;
  requests: ApplyRequest[];
} {
  const loc = get(locale);
  const trackers: Record<string, ApplyTracker> = {};
  const requests = buildApplyRequests(targets);
  for (const [index, t] of targets.entries()) {
    const apply_id = requests[index].apply_id;
    trackers[apply_id] = {
      apply_id,
      group_id: "",
      game_id: t.game_id,
      game_label: t.game_label,
      dll_path: t.record.path,
      family: t.record.family,
      target_version: t.target_version,
      stage: "download",
      failed_at_stage: null,
      message: translate(loc, "toast.trackerQueued"),
      progress: null,
      error: null,
      error_class: null,
      attempt: null,
      bytes_downloaded: 0,
      bytes_total: null,
      bytes_per_sec: 0,
      started_at: Date.now(),
      ended_at: null,
    };

  }
  return { trackers, requests };
}

function failAllTrackers(trackers: Record<string, ApplyTracker>, msg: string): void {
  activeApplies.update((m) => {
    const next = { ...m };
    for (const id of Object.keys(trackers)) {
      const cur = next[id];
      if (!cur) continue;
      next[id] = {
        ...cur,
        stage: "failed",
        failed_at_stage: cur.failed_at_stage ?? cur.stage,
        error: msg,
        message: msg,
        ended_at: Date.now(),
      };
    }
    return next;
  });
}

/// Apply an NVIDIA Streamline plugin set as one atomic transaction (all-or-nothing
/// in the backend). Reuses the per-member tracker/modal plumbing so the progress
/// modal shows each file, but the whole set succeeds or rolls back together.
export async function dispatchStreamlineSet(
  targets: ApplyTarget[],
  opts: DispatchOptions = {},
): Promise<StreamlineSetResult | null> {
  const toast = opts.toast ?? DEFAULT_TOAST;
  const loc = get(locale);
  if (targets.length === 0) {
    toast("warning", translate(loc, "toast.streamlineNoUpdates"));
    return null;
  }
  if (isApplyInflight()) {
    toast("warning", translate(loc, "toast.applyInProgress"));
    return null;
  }
  const completeTargets = await prepareSetTargets(targets, toast);
  if (!completeTargets) return null;
  targets = completeTargets;
  pruneEndedApplyState();
  const { trackers, requests } = prepareApply(targets);
  activeApplies.update((m) => ({ ...m, ...trackers }));
  opts.showModal?.();
  const count = targets.length;
  toast("info", translate(loc, "toast.streamlineUpdating", { count }));
  try {
    const result = await applyStreamlineSet(requests);
    if (result.success) {
      annotateOutcomes({ outcomes: result.applied });
      toast("success", translate(loc, "toast.streamlineUpdated", { count }));
      notifyApplySuccess(result.applied.length);
    } else {
      const rolledBack = result.rolled_back ? translate(loc, "toast.streamlineRolledBack") : "";
      const reason = result.error ?? translate(loc, "toast.streamlineSetFailed");
      failAllTrackers(trackers, `${reason}${rolledBack}`);
      toast(
        "danger",
        translate(loc, "toast.streamlineUpdateFailed", {
          error: result.error ?? translate(loc, "toast.unknownError"),
          rolledBack,
        }),
      );
    }
    return result;
  } catch (err: unknown) {
    const msg = formatError(err);
    failAllTrackers(trackers, msg);
    toast("danger", translate(loc, "toast.streamlineApplyFailed", { msg }));
    return null;
  }
}

/// Apply a coherent multi-DLL vendor set (FSR SDK / XeSS SDK) as one atomic
/// transaction. Mirrors `dispatchStreamlineSet` but routes through the
/// generalized `apply_dll_set` command, whose backend guard enforces set
/// coherence plus the FSR4 hardware gate (fail-closed).
export async function dispatchDllSet(
  targets: ApplyTarget[],
  setLabel: string,
  opts: DispatchOptions = {},
): Promise<StreamlineSetResult | null> {
  const toast = opts.toast ?? DEFAULT_TOAST;
  const loc = get(locale);
  if (targets.length === 0) {
    toast("warning", translate(loc, "toast.setNoUpdates", { label: setLabel }));
    return null;
  }
  if (isApplyInflight()) {
    toast("warning", translate(loc, "toast.applyInProgress"));
    return null;
  }
  const completeTargets = await prepareSetTargets(targets, toast);
  if (!completeTargets) return null;
  targets = completeTargets;
  pruneEndedApplyState();
  const { trackers, requests } = prepareApply(targets);
  activeApplies.update((m) => ({ ...m, ...trackers }));
  opts.showModal?.();
  const count = targets.length;
  toast("info", translate(loc, "toast.setUpdating", { label: setLabel, count }));
  try {
    const result = await applyDllSet(requests);
    if (result.success) {
      annotateOutcomes({ outcomes: result.applied });
      toast("success", translate(loc, "toast.setUpdated", { label: setLabel, count }));
      notifyApplySuccess(result.applied.length);
    } else {
      const rolledBack = result.rolled_back ? translate(loc, "toast.streamlineRolledBack") : "";
      const reason = result.error ?? translate(loc, "toast.setFailed", { label: setLabel });
      failAllTrackers(trackers, `${reason}${rolledBack}`);
      toast(
        "danger",
        translate(loc, "toast.setUpdateFailed", {
          label: setLabel,
          error: result.error ?? translate(loc, "toast.unknownError"),
          rolledBack,
        }),
      );
    }
    return result;
  } catch (err: unknown) {
    const msg = formatError(err);
    failAllTrackers(trackers, msg);
    toast("danger", translate(loc, "toast.setApplyFailed", { label: setLabel, msg }));
    return null;
  }
}

/** Windows paths are case-insensitive; normalize separators and lexical dot segments.
 * Game IDs stay opaque unless the store supplies a shared install directory.
 */
function targetIdentity(gameId: string, path: string): string {
  const normalize = (value: string): string => {
    const parts: string[] = [];
    for (const part of value.replaceAll("\\", "/").toLowerCase().split("/")) {
      if (part === ".") continue;
      if (part === ".." && parts.length > 0) parts.pop();
      else parts.push(part);
    }
    return parts.join("/").replace(/\/$/, "");
  };
  const installDir = get(games).find((game) => game.id === gameId)?.install_dir;
  return JSON.stringify([installDir ? normalize(installDir) : gameId, normalize(path)]);
}

function targetFromTracker(tracker: ApplyTracker): ApplyTarget {
  return {
    game_id: tracker.game_id,
    game_label: tracker.game_label ?? tracker.game_id,
    target_version: tracker.target_version,
    record: {
      family: tracker.family as DllRecord["family"],
      path: tracker.dll_path,
      current_version: null,
      sha256: null,
      file_description: null,
    },
  };
}

export async function retrySingleApply(tracker: ApplyTracker): Promise<void> {
  if (!classifyApplyError(tracker.error, tracker.error_class).retryable) return;
  await dispatchApply([targetFromTracker(tracker)], { supersedes: [tracker] });
}

export async function retryFailedTrackers(trackers: ApplyTracker[]): Promise<void> {
  const failed = trackers.filter((tracker) =>
    (tracker.stage === "failed" || tracker.stage === "cancelled") &&
    classifyApplyError(tracker.error, tracker.error_class).retryable,
  );
  if (failed.length === 0) return;
  await dispatchApply(failed.map(targetFromTracker), { supersedes: failed });
}
export async function cancelOne(applyId: string): Promise<void> {
  try {
    await cancelApplyApi(applyId);
  } catch (err: unknown) {
    showToast("danger", translate(get(locale), "toast.cancelFailed", { msg: formatError(err) }));
  }
}

export async function cancelAll(): Promise<void> {
  try {
    await cancelAllAppliesApi();
  } catch (err: unknown) {
    showToast("danger", translate(get(locale), "toast.cancelFailed", { msg: formatError(err) }));
  }
}

export function snapshotActive(): ApplyTracker[] {
  return Object.values(get(activeApplies));
}

export function buildTargetFromRecord(
  game: { id: string; name: string; launcher: LauncherKind | string },
  record: DllRecord,
  target_version: string,
): ApplyTarget {
  return {
    game_id: game.id,
    game_label: game.name,
    record,
    target_version,
  };
}

function annotateOutcomes(result: ApplyBatchResult): void {
  const loc = get(locale);
  activeApplies.update((m) => {
    const next = { ...m };
    for (const o of result.outcomes) {
      const cur = next[o.apply_id];
      if (!cur) continue;
      if (o.success) {
        next[o.apply_id] = {
          ...cur,
          stage: "complete",
          message: o.new_version
            ? translate(loc, "toast.trackerUpdatedTo", { version: o.new_version })
            : translate(loc, "toast.trackerUpdated"),
          progress: 1,
          ended_at: Date.now(),
        };
      } else if (!cur.error && o.error) {
        next[o.apply_id] = {
          ...cur,
          stage: classifyApplyError(o.error).kind === "cancelled" ? "cancelled" : "failed",
          failed_at_stage: cur.failed_at_stage ?? cur.stage,
          error: o.error,
          message: o.error,
          ended_at: Date.now(),
        };
      } else if (cur.stage !== "complete" && cur.stage !== "failed") {
        next[o.apply_id] = {
          ...cur,
          ended_at: Date.now(),
        };
      }
    }
    return next;
  });
}
