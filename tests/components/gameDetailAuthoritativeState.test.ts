/** Cohort C1 — the game detail surface decides from the authoritative component state.
 *
 *  It used to call `dllRelation` with the catalog stores to decide what was outdated and which
 *  version to install. Both now come from the `ComponentState` Rust published. The catalog stores
 *  stay empty in every test, so a surface that still resolved a target locally would offer
 *  nothing. Statuses that must not be offered keep their own presentation and never read as
 *  "Up to date". */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { tick } from "svelte";
import { writable } from "svelte/store";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve, dirname } from "node:path";
import { render, fireEvent } from "@testing-library/svelte";
import type { AuthoritativeState } from "@/lib/stateSync";
import type { AppSettings, DetectedGame, DllRecord, GameSnapshot } from "@/lib/api";
import type { ComponentState, ComponentStatus } from "@/generated/bindings";

const here = dirname(fileURLToPath(import.meta.url));
const drawerSource = readFileSync(
  resolve(here, "../../frontend/src/components/GameDetailDrawer.svelte"),
  "utf8",
);

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

const authoritative = writable<AuthoritativeState>(EMPTY_STATE);
const dispatchApply = vi.fn(async () => null);

vi.mock("@/lib/stateSync", () => ({
  authoritativeState: authoritative,
  startStateSync: vi.fn(async () => undefined),
  stopStateSync: vi.fn(async () => undefined),
}));

vi.mock("@/lib/applyController", () => ({
  dispatchApply,
  dispatchStreamlineSet: vi.fn(async () => null),
  dispatchDllSet: vi.fn(async () => null),
  isApplyInflight: vi.fn(() => false),
  pruneEndedApplyState: vi.fn(),
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    findGameExecutable: vi.fn(async () => null),
    detectAnticheat: vi.fn(async () => null),
    readDlssOverrideConfig: vi.fn(async () => {
      throw new Error("not available in this test");
    }),
  };
});

vi.mock("@/lib/stores", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/stores")>();
  return {
    ...actual,
    rescanGame: vi.fn(async () => undefined),
    ensureSystemInfo: vi.fn(async () => null),
  };
});

const stores = await import("@/lib/stores");
const GameDetailDrawer = (await import("@/components/GameDetailDrawer.svelte")).default;

const INSTALL_DIR = "C:\\Games\\Cyberpunk 2077";

function detected(): DetectedGame {
  return { id: "g1", name: "Cyberpunk 2077", install_dir: INSTALL_DIR, launcher: "steam" } as DetectedGame;
}

function record(family: string, filename: string, version: string | null): DllRecord {
  return {
    family: family as DllRecord["family"],
    path: `${INSTALL_DIR}\\bin\\${filename}`,
    current_version: version,
    file_description: null,
    sha256: null,
  };
}

function component(
  family: string,
  filename: string,
  status: ComponentStatus,
  over: Partial<ComponentState> = {},
): ComponentState {
  return {
    identity: {
      game_id: "g1",
      relative_path: `bin/${filename}`,
      family,
      filename,
      architecture: "x64",
      graphics_api: null,
    },
    observed_version: "310.1.0",
    observed_hash: null,
    candidate: {
      id: `nvidia:${family}:310.4.0`,
      family,
      filename,
      file_version: "310.4.0.0",
      package_version: "310.4.0",
      package_id: `nvidia-${family}-310.4.0`,
      compatibility_line: family,
      architecture: "x64",
      hash: { algorithm: "sha256", digest: "a".repeat(64) },
      size_bytes: "1024",
      source_url: "https://example.invalid/artifact",
      archive_entry: null,
      expected_publisher: "NVIDIA Corporation",
      observed_publisher: "NVIDIA Corporation",
      signature_status: "verified",
      dependencies: [],
      checked_at: "2026-09-18T00:00:00Z",
    },
    status,
    compatibility: { status: "compatible", reason_code: "catalog_candidate_available", evidence: [] },
    support: "official",
    applicability: "applicable",
    owner: null,
    checked_at: "2026-09-18T00:00:00Z",
    revision: "7",
    ...over,
  } as ComponentState;
}

function snapshot(components: ComponentState[], over: Partial<GameSnapshot> = {}): GameSnapshot {
  return {
    id: "g1",
    name: "Cyberpunk 2077",
    install_dir: INSTALL_DIR,
    launcher: "steam",
    status: "update_available",
    revision: "7",
    components,
    ...over,
  } as GameSnapshot;
}

function settingsFixture(): AppSettings {
  return {
    blacklist: [],
    game_preferences: {},
    update_prefs: null,
    launcher_overrides: { custom: [] },
    ui_prefs: {
      favorite_game_ids: [],
      library_view_mode: "grid",
      library_density: "compact",
      library_sort: "default",
    },
  } as unknown as AppSettings;
}

function drawerProps(): never {
  return {
    gameId: "g1",
    onClose: (): void => undefined,
    onApplyStart: (): void => undefined,
  } as never;
}

function featureChips(container: HTMLElement): string[] {
  return [...container.querySelectorAll(".feature-head .chip")].map(
    (node) => node.textContent?.trim() ?? "",
  );
}

beforeEach(() => {
  dispatchApply.mockClear();
  authoritative.set(EMPTY_STATE);
  stores.settings.set(settingsFixture());
  stores.games.set([detected()]);
  stores.gameDlls.set({ g1: [record("dlss_sr", "nvngx_dlss.dll", "310.1.0")] });
  stores.gameDllsLoading.set({});
  stores.gameDllErrors.set({});
  stores.gameDlssEnabler.set({});
  stores.driverReports.set([]);
  stores.systemInfo.set({ gpus: [{ vendor: "nvidia" }] } as unknown as import("@/lib/api").SystemInfo);
  // Deliberately empty: the drawer may not resolve a target version from the catalog.
  stores.catalogLatestByKey.set({});
  stores.catalogLatestShas.set({});
});

describe("GameDetailDrawer — decisions read the authoritative component state", () => {
  it("selects matching hardware when its observation arrives, leaving other technologies unselected", async () => {
    stores.systemInfo.set(null);
    stores.gameDlls.set({ g1: [record("dlss_sr", "nvngx_dlss.dll", "310.1.0"), record("fsr_upscaler", "amd_fidelityfx_upscaler.dll", "1.0.0")] });
    authoritative.set({ ...EMPTY_STATE, emitterId: "emitter-a", games: [snapshot([component("dlss_sr", "nvngx_dlss.dll", "update_available"), component("fsr_upscaler", "amd_fidelityfx_upscaler.dll", "update_available")])] });
    const { container } = render(GameDetailDrawer, drawerProps());
    await tick();
    expect((container.querySelector(".foot-apply") as HTMLButtonElement).disabled).toBe(true);
    stores.systemInfo.set({ gpus: [{ vendor: "nvidia" }] } as unknown as import("@/lib/api").SystemInfo);
    await tick();
    const other = container.querySelector(".other-feature-disclosure") as HTMLDetailsElement;
    expect(other.open).toBe(false);
    expect((other.querySelector(".feature-check input") as HTMLInputElement).checked).toBe(false);
    expect((container.querySelector(".feature-list .feature-check input") as HTMLInputElement).checked).toBe(true);
  });

  it("allows an explicit selection from the other-technology disclosure", async () => {
    stores.gameDlls.set({ g1: [record("dlss_sr", "nvngx_dlss.dll", "310.1.0"), record("fsr_upscaler", "amd_fidelityfx_upscaler.dll", "1.0.0")] });
    authoritative.set({ ...EMPTY_STATE, emitterId: "emitter-a", games: [snapshot([component("dlss_sr", "nvngx_dlss.dll", "update_available"), component("fsr_upscaler", "amd_fidelityfx_upscaler.dll", "update_available")])] });
    const { container } = render(GameDetailDrawer, drawerProps());
    await tick();
    const other = container.querySelector(".other-feature-disclosure") as HTMLDetailsElement;
    await fireEvent.click(other.querySelector("summary")!);
    const checkbox = other.querySelector(".feature-check input") as HTMLInputElement;
    await fireEvent.change(checkbox, { target: { checked: true } });
    await fireEvent.click(container.querySelector(".foot-apply")!);
    const targets = dispatchApply.mock.calls[0][0] as { record: { family: string } }[];
    expect(targets.map(target => target.record.family).sort()).toEqual(["dlss_sr", "fsr_upscaler"]);
  });
  it("offers and applies the candidate version Rust published", async () => {
    authoritative.set({
      ...EMPTY_STATE,
      emitterId: "emitter-a",
      games: [snapshot([component("dlss_sr", "nvngx_dlss.dll", "update_available")])],
    });
    const { container } = render(GameDetailDrawer, drawerProps());
    await tick();

    expect(featureChips(container)).toContain("Update ready");
    expect(container.querySelector(".target-btn .target")?.textContent?.trim()).toBe("v310.4.0");

    const apply = container.querySelector(".foot-apply") as HTMLButtonElement;
    expect(apply.disabled).toBe(false);
    await fireEvent.click(apply);

    expect(dispatchApply).toHaveBeenCalledTimes(1);
    const targets = dispatchApply.mock.calls[0][0] as {
      game_id: string;
      target_version: string;
      record: { path: string };
    }[];
    expect(targets).toHaveLength(1);
    expect(targets[0].game_id).toBe("g1");
    expect(targets[0].target_version).toBe("310.4.0");
    expect(targets[0].record.path).toBe(`${INSTALL_DIR}\\bin\\nvngx_dlss.dll`);
  });

  it("presents a current component as up to date and selects nothing", async () => {
    authoritative.set({
      ...EMPTY_STATE,
      emitterId: "emitter-a",
      games: [
        snapshot([component("dlss_sr", "nvngx_dlss.dll", "current", { candidate: null })], {
          status: "current",
        }),
      ],
    });
    const { container } = render(GameDetailDrawer, drawerProps());
    await tick();

    expect(featureChips(container)).toContain("Up to date");
    expect((container.querySelector(".foot-apply") as HTMLButtonElement).disabled).toBe(true);
    expect(dispatchApply).not.toHaveBeenCalled();
  });

  it("never offers an externally managed component and never calls it up to date", async () => {
    authoritative.set({
      ...EMPTY_STATE,
      emitterId: "emitter-a",
      games: [
        snapshot([component("dlss_sr", "nvngx_dlss.dll", "externally_managed")], {
          status: "non_actionable",
        }),
      ],
    });
    const { container } = render(GameDetailDrawer, drawerProps());
    await tick();

    const chips = featureChips(container);
    expect(chips).not.toContain("Up to date");
    expect(chips).not.toContain("Update ready");
    expect((container.querySelector(".foot-apply") as HTMLButtonElement).disabled).toBe(true);
  });

  it("does not offer an available update whose applicability Rust marked not applicable", async () => {
    authoritative.set({
      ...EMPTY_STATE,
      emitterId: "emitter-a",
      games: [
        snapshot([
          component("dlss_sr", "nvngx_dlss.dll", "update_available", { applicability: "not_applicable" }),
        ]),
      ],
    });
    const { container } = render(GameDetailDrawer, drawerProps());
    await tick();

    expect(featureChips(container)).not.toContain("Update ready");
    expect((container.querySelector(".foot-apply") as HTMLButtonElement).disabled).toBe(true);
  });

  it("keeps a scanned file with no published component unknown, not outdated and not current", async () => {
    const { container } = render(GameDetailDrawer, drawerProps());
    await tick();

    const chips = featureChips(container);
    expect(chips).toContain("Unknown");
    expect(chips).not.toContain("Up to date");
    expect((container.querySelector(".foot-apply") as HTMLButtonElement).disabled).toBe(true);
    expect(container.querySelector(".target-btn .target")).toBeNull();
  });

  it("no longer decides with the local relation helper", () => {
    expect(drawerSource).not.toMatch(/dllRelation/);
    expect(drawerSource).not.toMatch(/relationContext/);
    expect(drawerSource).not.toMatch(/catalogLatestByKey/);
    expect(drawerSource).toMatch(/from "\.\.\/lib\/stateSync"/);
  });
});
