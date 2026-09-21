import { describe, it, expect, beforeEach, vi } from "vitest";
import { get } from "svelte/store";
import type { ApplyBatchResult, ApplyRequest, DllRecord, UpdatePlan } from "@/lib/api";

let pendingResolve: ((result: ApplyBatchResult) => void) | null = null;
const previewUpdatePlan = vi.fn(async (requests: ApplyRequest[]): Promise<UpdatePlan> => ({
  id: "preview", schema_version: 1, catalog_generated_at: "2026-07-10T00:00:00Z",
  catalog_revision: "revision", fingerprint: "fingerprint", created_at: "2026-07-10T00:00:00Z",
  stale: false, changes: [], items: requests.map((request) => ({
    id: request.apply_id, game_id: request.game_id, game_name: request.game_label ?? request.game_id,
    dll_path: request.dll_path, family: request.family, current_version: "1.0.0.0",
    target_version: request.target_version, backup_path: "C:\\Backups\\component.dll", selected: true,
    trust: { source_url: "https://example.test/component.dll", expected_sha256: "a".repeat(64),
      observed_sha256: request.observed_sha256 ?? null, signature_subject: null, signature_verified: false, anti_cheat_risk: null },
  })),
}));
const applyUpdateBatch = vi.fn(
  () =>
    new Promise<ApplyBatchResult>((resolve) => {
      pendingResolve = resolve;
    }),
);

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    previewUpdatePlan: (...args: unknown[]) => (previewUpdatePlan as unknown as (...args: unknown[]) => ReturnType<typeof previewUpdatePlan>)(...args),
    getCatalogStatus: vi.fn(async () => ({ provenance: { generated_at: "2026-07-10T00:00:00Z" } })),
    applyUpdateBatch: (...a: unknown[]) =>
      (applyUpdateBatch as unknown as (...x: unknown[]) => Promise<ApplyBatchResult>)(...a),
  };
});

vi.mock("@/lib/community", () => ({ notifyApplySuccess: vi.fn() }));

import { dispatchApply, retryFailedTrackers, type ApplyTarget } from "@/lib/applyController";
import { activeApplies, toasts } from "@/lib/stores";
import { pendingUpdatePlan } from "@/features/update-plan/model";

const reviewPlan = async (targets: ApplyTarget[]) => ({
  targets,
  catalogGeneratedAt: "2026-07-10T00:00:00Z",
});

function target(gameId: string, family: DllRecord["family"] = "dlss_sr"): ApplyTarget {
  return {
    game_id: gameId,
    game_label: gameId,
    record: {
      family,
      path: `C:\\Games\\${gameId}\\nvngx_dlss.dll`,
      current_version: "1.0.0.0",
      file_description: null,
      sha256: null,
    },
    target_version: "2.0.0.0",
  };
}

beforeEach(() => {
  pendingResolve = null;
  applyUpdateBatch.mockClear();
  previewUpdatePlan.mockClear();
  pendingUpdatePlan.set(null);
  activeApplies.set({});
  toasts.set([]);
});

describe("dispatchApply — concurrent dispatch never wipes an in-flight batch", () => {
  it("Retry all deduplicates canonical targets on the second retry and clears superseded failures on success", async () => {
    async function finish(run: Promise<unknown>, call: number, success: boolean) {
      await vi.waitFor(() => expect(applyUpdateBatch).toHaveBeenCalledTimes(call));
      const sent = (applyUpdateBatch.mock.calls[call - 1] as unknown as [{ items: ApplyRequest[] }])[0];
      pendingResolve?.({ outcomes: sent.items.map((r) => ({
        apply_id: r.apply_id, success, new_version: success ? r.target_version : null,
        error: success ? null : "network timeout",
      })) } as ApplyBatchResult);
      await run;
    }
    await finish(dispatchApply([target("retry")]), 1, false);
    // Historical duplicate with Windows-equivalent spelling, as retained by the old controller.
    activeApplies.update((m) => {
      const old = Object.values(m)[0];
      return { ...m, historical: { ...old, apply_id: "historical", dll_path: old.dll_path.replaceAll("\\", "/").toUpperCase() } };
    });
    await finish(retryFailedTrackers(Object.values(get(activeApplies))), 2, false);
    await finish(retryFailedTrackers(Object.values(get(activeApplies))), 3, true);
    expect(previewUpdatePlan.mock.calls.slice(1).map(([requests]) => requests.length)).toEqual([1, 1]);
    expect(Object.values(get(activeApplies)).filter((r) => r.stage === "failed")).toHaveLength(0);
    expect(Object.values(get(activeApplies))).toHaveLength(1);
    expect(Object.values(get(activeApplies))[0].stage).toBe("complete");
  });
  it("tracks and counts every backend-added dependency before dispatch", async () => {
    const selected = target("coherent", "direct_storage");
    const plan: UpdatePlan = {
      id: "dependencies", schema_version: 1, catalog_generated_at: "2026-07-10T00:00:00Z",
      catalog_revision: "revision", fingerprint: "fingerprint", created_at: "2026-07-10T00:00:00Z",
      stale: false, changes: [],
      items: ["direct_storage", "direct_storage_core"].map((family, index) => ({
        id: family, game_id: selected.game_id, game_name: selected.game_label,
        dll_path: index === 0 ? selected.record.path : "C:\\Games\\coherent\\dstoragecore.dll",
        family, current_version: "1.0.0.0", target_version: "2.0.0.0",
        backup_path: `C:\\Backups\\${family}.dll`, selected: true,
        trust: { source_url: "https://example.test/sdk.zip", expected_sha256: "a".repeat(64),
          observed_sha256: "b".repeat(64), signature_subject: null, signature_verified: false, anti_cheat_risk: null },
      })),
    };
    const toast = vi.fn();
    const started = dispatchApply([selected], {
      toast, reviewPlan: async (targets) => ({ targets, plan, catalogGeneratedAt: plan.catalog_generated_at }),
    });
    await vi.waitFor(() => expect(applyUpdateBatch).toHaveBeenCalledTimes(1));
    try {
      const sent = (applyUpdateBatch.mock.calls[0] as unknown as [{ items: { apply_id: string }[] }])[0];
      expect(sent.items).toHaveLength(2);
      expect(Object.values(get(activeApplies)).map((item) => item.family)).toEqual(["direct_storage", "direct_storage_core"]);
      expect(sent.items.map((item) => item.apply_id)).toEqual(Object.keys(get(activeApplies)));
      expect(toast.mock.calls[0][1]).toContain("2");
    } finally {
      pendingResolve?.({ outcomes: [] });
      await started;
    }
  });

  it("starts a normal update without opening a separate confirmation dialog", async () => {
    const started = dispatchApply([target("one-click")]);
    await vi.waitFor(() => expect(applyUpdateBatch).toHaveBeenCalledTimes(1));
    expect(previewUpdatePlan).toHaveBeenCalledTimes(1);
    expect(get(pendingUpdatePlan)).toBeNull();
    pendingResolve?.({ outcomes: [] });
    await started;
  });
  it("a second dispatch while the first is in flight preserves the first batch's trackers", async () => {
    const first = dispatchApply([target("alpha"), target("beta")], { reviewPlan });
    await Promise.resolve();
    await Promise.resolve();
    const afterFirst = get(activeApplies);
    expect(Object.keys(afterFirst)).toHaveLength(2);
    const firstIds = Object.keys(afterFirst);

    const second = await dispatchApply([target("gamma")], { reviewPlan });
    expect(second).toBeNull();
    expect(applyUpdateBatch).toHaveBeenCalledTimes(1);

    const afterSecond = get(activeApplies);
    expect(Object.keys(afterSecond).sort()).toEqual([...firstIds].sort());
    for (const id of firstIds) {
      expect(afterSecond[id]).toEqual(afterFirst[id]);
    }
    expect(get(toasts).at(-1)?.kind).toBe("warning");

    pendingResolve?.({ outcomes: [] });
    await first;
  });

  it("merges a fresh batch into recently-finished trackers rather than replacing them", async () => {
    const done = dispatchApply([target("alpha")], { reviewPlan });
    await Promise.resolve();
    await Promise.resolve();
    pendingResolve?.({ outcomes: [] });
    await done;
    // In the real app the apply-progress event listener stamps `ended_at`; the
    // unit test has no listener, so mark the batch finished explicitly to clear
    // the in-flight guard before the next dispatch.
    activeApplies.update((m) => {
      const next = { ...m };
      for (const id of Object.keys(next)) next[id] = { ...next[id], ended_at: Date.now() };
      return next;
    });
    const resolved = get(activeApplies);
    expect(Object.keys(resolved)).toHaveLength(1);
    const oldId = Object.keys(resolved)[0];

    dispatchApply([target("beta")], { reviewPlan });
    await Promise.resolve();
    await Promise.resolve();
    const merged = get(activeApplies);
    expect(Object.keys(merged)).toHaveLength(2);
    expect(merged[oldId]).toEqual(resolved[oldId]);

    pendingResolve?.({ outcomes: [] });
  });

  it("prunes stale finished trackers when a fresh batch dispatches", async () => {
    const done = dispatchApply([target("alpha")], { reviewPlan });
    await Promise.resolve();
    await Promise.resolve();
    pendingResolve?.({ outcomes: [] });
    await done;
    activeApplies.update((m) => {
      const next = { ...m };
      for (const id of Object.keys(next)) next[id] = { ...next[id], ended_at: 1 };
      return next;
    });
    const staleId = Object.keys(get(activeApplies))[0];

    dispatchApply([target("beta")], { reviewPlan });
    await Promise.resolve();
    await Promise.resolve();
    const merged = get(activeApplies);
    expect(Object.keys(merged)).toHaveLength(1);
    expect(merged[staleId]).toBeUndefined();

    pendingResolve?.({ outcomes: [] });
  });
});
