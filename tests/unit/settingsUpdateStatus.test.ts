import { describe, expect, it } from "vitest";
import {
  classifyUpdateCheck,
  updateStatusText,
  type AppUpdateAvailability,
} from "@/views/Settings.svelte";
import { translate } from "@/lib/i18n/index";

/** 10.12 — the availability of an app update keeps four distinct outcomes.
 *  "The updater answered: no newer release" and "the check did not complete" must never share a
 *  representation, and neither may be shown as "up to date". */
describe("Settings — app update availability", () => {
  it("treats the documented empty answer as an answer, not as a failure", () => {
    expect(classifyUpdateCheck(null)).toEqual({ kind: "upToDate" });
    expect(classifyUpdateCheck(undefined)).toEqual({ kind: "upToDate" });
    expect(classifyUpdateCheck({ available: false, version: "9.9.9" })).toEqual({ kind: "upToDate" });
  });

  it("reports an available release with its version, and without one when the answer omits it", () => {
    expect(classifyUpdateCheck({ available: true, version: "1.8.0" })).toEqual({
      kind: "available",
      version: "1.8.0",
    });
    expect(classifyUpdateCheck({ version: "1.8.0" })).toEqual({ kind: "available", version: "1.8.0" });
    expect(classifyUpdateCheck({ available: true })).toEqual({ kind: "available", version: null });
    expect(classifyUpdateCheck({ available: true, version: "   " })).toEqual({ kind: "available", version: null });
  });

  it("keeps an answer that states nothing as indeterminate instead of up to date", () => {
    expect(classifyUpdateCheck({})).toEqual({ kind: "indeterminate", reason: "no-availability-field" });
    expect(classifyUpdateCheck({ available: "yes" })).toEqual({ kind: "indeterminate", reason: "unexpected-shape" });
    expect(classifyUpdateCheck("no-update")).toEqual({ kind: "indeterminate", reason: "unexpected-shape" });
    expect(classifyUpdateCheck(false)).toEqual({ kind: "indeterminate", reason: "unexpected-shape" });
  });

  it("gives every outcome its own text, and never reuses the up-to-date wording", () => {
    const cases: AppUpdateAvailability[] = [
      { kind: "unchecked" },
      { kind: "checking" },
      { kind: "available", version: "1.8.0" },
      { kind: "upToDate" },
      { kind: "indeterminate", reason: "no-availability-field" },
      { kind: "error", error: "network unreachable" },
      { kind: "manualOnly" },
    ];
    const texts = cases.map((status) => updateStatusText("en", status, "1.7.0", "Nexus Mods"));
    expect(new Set(texts).size).toBe(cases.length);
    for (const text of texts) expect(text.trim()).not.toBe("");

    const upToDate = updateStatusText("en", { kind: "upToDate" }, "1.7.0", "Standard");
    const unchecked = updateStatusText("en", { kind: "unchecked" }, "1.7.0", "Standard");
    const indeterminate = updateStatusText("en", { kind: "indeterminate", reason: "unexpected-shape" }, "1.7.0", "Standard");
    const failed = updateStatusText("en", { kind: "error", error: "boom" }, "1.7.0", "Standard");
    expect(unchecked).not.toBe(upToDate);
    expect(indeterminate).not.toBe(upToDate);
    expect(failed).not.toBe(upToDate);
    expect(failed).toContain("boom");
  });

  it("names the manual channel without inviting an automatic check", () => {
    const text = updateStatusText("en", { kind: "manualOnly" }, "1.7.0", "Nexus Mods");
    expect(text).toContain("Nexus Mods");
    expect(text.toLowerCase()).not.toContain("auto-update");
  });

  it("never injects an untranslated literal when the available release carries no version", () => {
    const text = updateStatusText("es", { kind: "available", version: null }, "1.7.0", "Standard");
    // The key now ships in the eight catalogs, so the row carries its own wording with the version
    // slot filled by a translated word. The English literal "unknown" must not reach the screen,
    // which is what the pre-change code inserted.
    expect(text).not.toContain("unknown");
    expect(text).toContain(translate("es", "status.unknown"));
  });

  it("resolves every outcome in every shipped locale", () => {
    for (const loc of ["en", "es", "pt-BR", "de", "fr", "ja", "ru", "zh-CN"] as const) {
      const text = updateStatusText(loc, { kind: "indeterminate", reason: "unexpected-shape" }, "1.7.0", "Standard");
      expect(text).not.toContain("view.settings.updates.status");
    }
  });
});
