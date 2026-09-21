import { describe, expect, it, vi } from "vitest";
import { createUiPreferenceWriter } from "@/lib/uiPreferences";
import type { AppSettings } from "@/lib/api";

function fixture(mode = "grid"): AppSettings {
  return { ui_prefs: { library_view_mode: mode, library_density: "comfy" }, advanced: { verbose_logs: false } } as unknown as AppSettings;
}

describe("UI preference persistence", () => {
  it("changes presentation before IPC finishes and never lets an older acknowledgement replace it", async () => {
    let local = fixture();
    let persisted = fixture();
    let release!: () => void;
    const firstSave = new Promise<void>(resolve => { release = resolve; });
    const writes: AppSettings[] = [];
    const writer = createUiPreferenceWriter({
      current: () => local,
      publish: next => { local = next; },
      read: async () => persisted,
      save: async next => { writes.push(next); if (writes.length === 1) await firstSave; persisted = next; },
      failed: vi.fn(),
    });
    const first = writer.update({ library_view_mode: "list" });
    expect(local.ui_prefs.library_view_mode).toBe("list");
    await vi.waitFor(() => expect(writes).toHaveLength(1));
    const second = writer.update({ library_view_mode: "grid", library_density: "compact" });
    expect(local.ui_prefs.library_view_mode).toBe("grid");
    release();
    await first;
    expect(local.ui_prefs.library_view_mode).toBe("grid");
    await second;
    expect(persisted.ui_prefs.library_density).toBe("compact");
    expect(persisted.ui_prefs.library_view_mode).toBe("grid");
  });

  it("preserves fresh non-UI backend settings and overlays pending choices on a concurrent read", async () => {
    let local = fixture();
    const fresh = { ...fixture(), advanced: { ...fixture().advanced, verbose_logs: true } };
    const save = vi.fn(async (_next: AppSettings) => undefined);
    const writer = createUiPreferenceWriter({ current: () => local, publish: next => { local = next; }, read: async () => fresh, save, failed: vi.fn() });
    const pending = writer.update({ library_view_mode: "list" });
    expect(writer.project(fresh).ui_prefs.library_view_mode).toBe("list");
    await pending;
    expect(save.mock.calls[0][0].advanced.verbose_logs).toBe(true);
  });

  it("restores the durable choice after a failed write and reports the failure", async () => {
    let local = fixture();
    const failed = vi.fn();
    const writer = createUiPreferenceWriter({ current: () => local, publish: next => { local = next; }, read: async () => fixture(), save: async () => { throw Error("disk unavailable"); }, failed });
    await writer.update({ library_view_mode: "list" });
    expect(local.ui_prefs.library_view_mode).toBe("grid");
    expect(failed).toHaveBeenCalledOnce();
  });
});
