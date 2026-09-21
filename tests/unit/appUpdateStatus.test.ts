/** Cohort C3 — one app-update classifier, shared by Settings and About without merging chunks.
 *
 *  The classifier used to exist twice, once per view, because the two views are code split. This
 *  suite pins the outcome of the extraction: a single implementation in `lib/appUpdateStatus.ts`,
 *  both views resolving to that same function, and no import edge that could pull one lazy view
 *  chunk into the other.
 *
 *  Cohort C5 is pinned at the bottom: no suite may reach into `frontend/node_modules` to mock a
 *  Tauri plugin. */
import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve, dirname, join, relative } from "node:path";
import {
  classifyUpdateCheck,
  updateStatusText,
  updateBannerFor,
  type AppUpdateAvailability,
} from "@/lib/appUpdateStatus";
import { classifyUpdateCheck as classifyInSettings, updateStatusText as settingsText } from "@/views/Settings.svelte";
import {
  classifyUpdateCheck as classifyInAbout,
  updateBannerFor as aboutBanner,
} from "@/views/About.svelte";
import { translate, LOCALES } from "@/lib/i18n/index";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../..");
const testsRoot = resolve(repoRoot, "tests");
const srcRoot = resolve(repoRoot, "frontend/src");

function read(relativePath: string): string {
  return readFileSync(resolve(srcRoot, relativePath), "utf8");
}

/** Module ids this source imports, statically or dynamically. Prose in a comment is not an import
 *  edge, so only the specifier of an `import` is collected. */
function importSpecifiers(source: string): string[] {
  const ids: string[] = [];
  for (const match of source.matchAll(/\bfrom\s+["']([^"']+)["']/g)) ids.push(match[1]);
  for (const match of source.matchAll(/\bimport\s*\(\s*["']([^"']+)["']/g)) ids.push(match[1]);
  return ids;
}

function testFiles(dir: string): string[] {
  const found: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      found.push(...testFiles(full));
    } else if (entry.endsWith(".test.ts")) {
      found.push(full);
    }
  }
  return found;
}

describe("appUpdateStatus — one classifier for both surfaces", () => {
  it("is the same function on both views, not two copies", () => {
    expect(classifyInSettings).toBe(classifyUpdateCheck);
    expect(classifyInAbout).toBe(classifyUpdateCheck);
    expect(settingsText).toBe(updateStatusText);
    expect(aboutBanner).toBe(updateBannerFor);
  });

  it("leaves no classifier body behind in either view", () => {
    for (const view of ["views/Settings.svelte", "views/About.svelte"]) {
      const source = read(view);
      expect(source).not.toMatch(/function classifyUpdateCheck/);
      expect(source).not.toMatch(/record\.available === true/);
      expect(source).toMatch(/from "\.\.\/lib\/appUpdateStatus"/);
    }
  });

  it("keeps the two lazy view chunks separable: the shared module imports no view, and no view imports the other", () => {
    const libImports = importSpecifiers(read("lib/appUpdateStatus.ts"));
    expect(libImports.filter((id) => id.includes(".svelte") || id.includes("views/"))).toEqual([]);
    expect(importSpecifiers(read("views/Settings.svelte")).filter((id) => id.includes("About"))).toEqual([]);
    expect(importSpecifiers(read("views/About.svelte")).filter((id) => id.includes("Settings"))).toEqual([]);
  });

  it("still separates every outcome, and never reuses the up-to-date wording", () => {
    const cases: AppUpdateAvailability[] = [
      { kind: "unchecked" },
      { kind: "checking" },
      { kind: "available", version: "1.8.0" },
      { kind: "upToDate" },
      { kind: "indeterminate", reason: "no-availability-field" },
      { kind: "error", error: "boom" },
      { kind: "manualOnly" },
    ];
    const texts = cases.map((status) => updateStatusText("en", status, "1.7.0", "Nexus Mods"));
    expect(new Set(texts).size).toBe(cases.length);
    const upToDate = updateStatusText("en", { kind: "upToDate" }, "1.7.0", "Standard");
    for (const status of [
      { kind: "unchecked" } as const,
      { kind: "indeterminate", reason: "unexpected-shape" } as const,
      { kind: "error", error: "boom" } as const,
    ]) {
      expect(updateStatusText("en", status, "1.7.0", "Standard")).not.toBe(upToDate);
    }
  });

  it("draws no About banner for an outcome that reports no measurement", () => {
    expect(updateBannerFor("en", { kind: "unchecked" }, "1.7.0")).toBeNull();
    // A channel without a self-updater never ran a check, so there is no verdict to show.
    expect(updateBannerFor("en", { kind: "manualOnly" }, "1.7.0")).toBeNull();
  });

  it("reads its own translated key in every shipped locale, with no stand-in wording left", () => {
    const outcomes: AppUpdateAvailability[] = [
      { kind: "unchecked" },
      { kind: "checking" },
      { kind: "available", version: "1.8.0" },
      { kind: "upToDate" },
      { kind: "indeterminate", reason: "unexpected-shape" },
      { kind: "error", error: "endpoint unreachable" },
      { kind: "manualOnly" },
    ];
    for (const loc of LOCALES) {
      for (const status of outcomes) {
        const text = updateStatusText(loc, status, "1.7.0", "Nexus Mods");
        // An unresolved key renders as the key itself; a stand-in renders as a weaker key's text.
        expect(text).not.toContain("view.settings.updates.status");
        expect(text.trim()).not.toBe("");
        expect(text).not.toBe(translate(loc, "status.unknown"));
        expect(text).not.toBe(translate(loc, "view.backups.meta.unverified"));
      }
      const banner = updateBannerFor(loc, { kind: "indeterminate", reason: "unexpected-shape" }, "1.7.0")!;
      expect(banner.text).not.toContain("view.about.update.status");
      expect(banner.text).not.toBe(translate(loc, "status.unknown"));
    }
  });

  it("names the version and the raw failure text inside the translated wording", () => {
    expect(updateStatusText("es", { kind: "available", version: "1.8.0" }, "1.7.0", "Standard")).toContain("1.8.0");
    expect(updateStatusText("es", { kind: "upToDate" }, "1.7.0", "Standard")).toContain("1.7.0");
    // The updater's failure text is an open wire string: carried verbatim, never translated.
    expect(updateStatusText("es", { kind: "error", error: "endpoint unreachable" }, "1.7.0", "Standard")).toContain(
      "endpoint unreachable",
    );
    expect(updateStatusText("es", { kind: "manualOnly" }, "1.7.0", "Nexus Mods")).toContain("Nexus Mods");
  });
});

describe("plugin mocks — central, not reached through node_modules", () => {
  it("has no suite that mocks a module through a path inside the installed dependencies", () => {
    // Matches a `vi.mock` whose module id reaches into an install directory, which is what the
    // per-suite plugin mocks used to do.
    const reachIntoInstall = /vi\.mock\(\s*["'][^"']*node_modules/;
    const offenders = testFiles(testsRoot)
      .filter((file) => reachIntoInstall.test(readFileSync(file, "utf8")))
      .map((file) => relative(repoRoot, file));
    expect(offenders).toEqual([]);
  });

  it("registers both the bare specifier and its resolved file for each plugin, in one place", () => {
    const setup = readFileSync(resolve(testsRoot, "setup.ts"), "utf8");
    for (const plugin of ["plugin-updater", "plugin-autostart"]) {
      expect(setup).toContain(`vi.mock("@tauri-apps/${plugin}"`);
      expect(setup).toContain(`@tauri-apps/${plugin}/dist-js/index.js`);
    }
    expect(setup).toContain("export const updaterMock");
    expect(setup).toContain("export const autostartMock");
  });
});
