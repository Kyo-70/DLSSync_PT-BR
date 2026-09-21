import { describe, expect, it } from "vitest";
import {
  classifyUpdateCheck as classifyInAbout,
  updateBannerFor,
  type AboutUpdateAvailability,
} from "@/views/About.svelte";
import { classifyUpdateCheck as classifyInSettings } from "@/views/Settings.svelte";
import { translate } from "@/lib/i18n/index";

/** 10.12 in the About surface. The classifier now lives once in `lib/appUpdateStatus.ts`, shared by
 *  the two code-split views; this suite keeps checking that both surfaces classify identically, and
 *  `tests/unit/appUpdateStatus.test.ts` checks that they resolve to that one implementation. */
describe("About — app update availability", () => {
  const samples: unknown[] = [
    null,
    undefined,
    {},
    { available: true },
    { available: true, version: "1.8.0" },
    { available: false },
    { available: false, version: "1.8.0" },
    { version: "1.8.0" },
    { version: "  " },
    { available: "yes" },
    "no-update",
    0,
  ];

  it("classifies every sample exactly like the Settings surface", () => {
    for (const sample of samples) {
      expect(classifyInAbout(sample)).toEqual(classifyInSettings(sample));
    }
  });

  it("shows no banner before a check has run", () => {
    expect(updateBannerFor("en", { kind: "unchecked" }, "1.7.0")).toBeNull();
  });

  it("separates the four outcomes by tone and by wording", () => {
    const outcomes: AboutUpdateAvailability[] = [
      { kind: "available", version: "1.8.0" },
      { kind: "upToDate" },
      { kind: "indeterminate", reason: "no-availability-field" },
      { kind: "error", error: "endpoint unreachable" },
    ];
    const banners = outcomes.map((status) => updateBannerFor("en", status, "1.7.0")!);
    expect(banners.map((b) => b.tone)).toEqual(["info", "success", "warning", "danger"]);
    expect(new Set(banners.map((b) => b.text)).size).toBe(4);

    const upToDateText = banners[1].text;
    expect(banners[2].text).not.toBe(upToDateText);
    expect(banners[3].text).not.toBe(upToDateText);
    expect(banners[3].text).toContain("endpoint unreachable");
  });

  it("uses a translated word when an available release carries no version", () => {
    const banner = updateBannerFor("es", { kind: "available", version: null }, "1.7.0")!;
    expect(banner.text).toContain(translate("es", "status.unknown"));
    expect(banner.text).not.toContain("unknown");
  });

  it("resolves the indeterminate wording in every shipped locale", () => {
    for (const loc of ["en", "es", "pt-BR", "de", "fr", "ja", "ru", "zh-CN"] as const) {
      const banner = updateBannerFor(loc, { kind: "indeterminate", reason: "unexpected-shape" }, "1.7.0")!;
      expect(banner.text).not.toContain("view.about.update.status");
      expect(banner.text.trim()).not.toBe("");
      // Its own key now ships in the eight catalogs, so the banner no longer falls back to the
      // weaker "Unknown" wording.
      expect(banner.text).not.toBe(translate(loc, "status.unknown"));
    }
  });
});
