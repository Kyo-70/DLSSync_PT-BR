import { describe, it, expect, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { applyStreamlineSet } from "../../frontend/src/lib/api";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve, dirname } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../frontend/src");
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const controller = readFileSync(resolve(root, "lib/applyController.ts"), "utf8");
// v1.6.7 drawer decomposition: the set-action button + its override-note title
// live in DrawerFooter; the enabler banner in DrawerHero; the members $derived +
// dispatchStreamlineSet routing stay in the orchestrator (GameDetailDrawer).
const drawer = readFileSync(resolve(root, "components/GameDetailDrawer.svelte"), "utf8");
const hero = readFileSync(resolve(root, "components/DrawerHero.svelte"), "utf8");
const footer = readFileSync(resolve(root, "components/DrawerFooter.svelte"), "utf8");
const enCatalog = readFileSync(resolve(root, "lib/i18n/locales/en.json"), "utf8");

describe("api — applyStreamlineSet binding", () => {
  it("preserves a failed recovery from the backend", async () => {
    const result = { success: false, applied: [], error: "restore failed", rolled_back: false };
    vi.mocked(invoke).mockResolvedValueOnce(result);
    expect(await applyStreamlineSet([])).toEqual(result);
    expect(invoke).toHaveBeenCalledWith("apply_streamline_set", { items: [] });
  });
});

describe("applyController — dispatchStreamlineSet", () => {
  it("calls applyStreamlineSet and reuses the tracker/modal plumbing", () => {
    expect(controller).toMatch(/export async function dispatchStreamlineSet/);
    expect(controller).toMatch(/await applyStreamlineSet\(requests\)/);
    expect(controller).toMatch(/prepareApply\(targets\)/);
  });

  it("surfaces rollback in the failure path", () => {
    expect(controller).toMatch(/rolled_back/);
    expect(enCatalog).toMatch(/rolled back to the previous set/);
  });
});

describe("GameDetailDrawer — Update Streamline set action", () => {
  const ux = readFileSync(resolve(root, "lib/ux.ts"), "utf8");

  it("derives the outdated sl.* set members with no enabler-managed exclusion", () => {
    expect(drawer).toMatch(/streamlineSetMembers\s*=\s*\$derived/);
    expect(drawer).toMatch(/isStreamlinePlugin\(filenameFromPath\(r\.path\)\)/);
    expect(drawer).toMatch(/&& isOutdated\(r\)\)/);
    expect(drawer).not.toMatch(/enablerManagedSl/);
  });

  it("renders the set action gated on members and routes through dispatchStreamlineSet", () => {
    expect(footer).toMatch(/\{#if streamlineSetCount > 0\}/);
    expect(enCatalog).toContain("Update Streamline set");
    expect(drawer).toMatch(/dispatchStreamlineSet\(targets/);
  });

  it("surfaces that a same-major set update is offered under an enabler (requires Streamline >= 2.11)", () => {
    expect(enCatalog).toMatch(/DLSS Enabler requires NVIDIA Streamline 2\.11 or newer/);
    expect(enCatalog).toMatch(/same major version/);
    expect(hero).toMatch(/\{#if dlssEnabler\}[\s\S]*?\$t\(["']note\.enablerManaged["']\)/);
    expect(footer).toMatch(/title=\{`[^`]*\$\{STREAMLINE_OVERRIDE_NOTE\}`\}/);
  });
});
