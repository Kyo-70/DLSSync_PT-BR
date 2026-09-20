import { beforeEach, describe, expect, it, vi } from "vitest";
import { tick } from "svelte";
import { writable } from "svelte/store";
import { render, fireEvent } from "@testing-library/svelte";
import type { AuthoritativeState } from "@/lib/stateSync";
import type { AppSettings, DetectedGame, GameSnapshot } from "@/lib/api";

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

vi.mock("@/lib/stores", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/stores")>();
  return {
    ...actual,
    // Any call to these would be a speculative refresh from the view; the test asserts on them.
    rescanGame: vi.fn(async () => undefined),
    scanGames: vi.fn(async () => undefined),
    loadBackups: vi.fn(async () => undefined),
  };
});

const stores = await import("@/lib/stores");
const Library = (await import("@/views/Library.svelte")).default;

function detected(id: string, name: string): DetectedGame {
  return {
    id,
    name,
    install_dir: `C:\\Games\\${name}`,
    launcher: "steam",
  } as DetectedGame;
}

function snapshot(over: Partial<GameSnapshot> = {}): GameSnapshot {
  return {
    id: "g1",
    name: "Cyberpunk 2077",
    install_dir: "C:\\Games\\Cyberpunk 2077",
    launcher: "steam",
    status: "update_available",
    revision: "7",
    components: [
      {
        identity: {
          game_id: "g1",
          relative_path: "bin/nvngx_dlss.dll",
          family: "dlss_sr",
          filename: "nvngx_dlss.dll",
          architecture: "x64",
          graphics_api: null,
        },
        observed_version: "310.1.0",
        observed_hash: { algorithm: "sha256", digest: "b".repeat(64) },
        candidate: {
          id: "nvidia:dlss_sr:310.4.0",
          family: "dlss_sr",
          filename: "nvngx_dlss.dll",
          file_version: "310.4.0.0",
          package_version: "310.4.0",
          package_id: "nvidia-dlss-310.4.0",
          compatibility_line: "dlss_sr",
          architecture: "x64",
          hash: { algorithm: "sha256", digest: "a".repeat(64) },
          size_bytes: "1024",
          source_url: "https://example.invalid/nvngx_dlss.dll",
          archive_entry: null,
          expected_publisher: "NVIDIA Corporation",
          observed_publisher: "NVIDIA Corporation",
          signature_status: "verified",
          dependencies: [],
          checked_at: "2026-09-17T00:00:00Z",
        },
        status: "update_available",
        compatibility: { status: "compatible", reason_code: "catalog_match", evidence: [] },
        support: "official",
        applicability: "applicable",
        owner: null,
        checked_at: "2026-09-17T00:00:00Z",
        revision: "7",
      },
    ],
    ...over,
  };
}

function settingsFixture(): AppSettings {
  return {
    blacklist: [],
    game_preferences: {},
    update_prefs: null,
    launcher_overrides: { custom: [] },
    ui_prefs: { favorite_game_ids: [], library_view_mode: "grid", library_density: "compact", library_sort: "default" },
  } as unknown as AppSettings;
}

function heroCounts(container: HTMLElement): { total: string; kpis: string[] } {
  return {
    total: container.querySelector(".library-summary")?.getAttribute("data-update-count") ?? "",
    kpis: [...container.querySelectorAll(".hero-kpi-num")].map((node) => node.textContent?.trim() ?? ""),
  };
}

beforeEach(() => {
  dispatchApply.mockClear();
  vi.mocked(stores.rescanGame).mockClear();
  authoritative.set(EMPTY_STATE);
  stores.settings.set(settingsFixture());
  stores.systemInfo.set({ gpus: [{ vendor: "nvidia" }] } as unknown as import("@/lib/api").SystemInfo);
  stores.driverReports.set([]);
  stores.games.set([detected("g1", "Cyberpunk 2077")]);
  stores.gameDlls.set({});
  stores.gameDllErrors.set({});
  stores.statusFilter.set("all");
  stores.searchQuery.set("");
  stores.launcherFilter.set("all");
});

describe("Library hero reads the authoritative apply set", () => {
  it("does not include another vendor in the default batch", async () => {
    const original = snapshot();
    const nvidia = original.components![0];
    const amd = { ...nvidia, identity: { ...nvidia.identity, family: "fsr_upscaler", filename: "amd_fidelityfx_upscaler.dll", relative_path: "bin/amd_fidelityfx_upscaler.dll" }, candidate: { ...nvidia.candidate!, family: "fsr_upscaler", package_version: "2.0.0" } };
    authoritative.set({ ...EMPTY_STATE, emitterId: "emitter-a", games: [snapshot({ components: [nvidia, amd] })] });
    const { container } = render(Library);
    await tick();
    await fireEvent.click(container.querySelector('[data-testid="library-update-all"]')!);
    const targets = dispatchApply.mock.calls[0][0] as { record: { family: string } }[];
    expect(targets.map(target => target.record.family)).toEqual(["dlss_sr"]);
  });

  it("keeps unknown hardware browsable without selecting a default batch", async () => {
    stores.systemInfo.set(null);
    authoritative.set({ ...EMPTY_STATE, emitterId: "emitter-a", games: [snapshot()] });
    const { container } = render(Library);
    await tick();
    expect(container.querySelector('[data-testid="library-update-all"]')).toBeNull();
    expect(container.querySelector(".game-card")).not.toBeNull();
    expect(dispatchApply).not.toHaveBeenCalled();
  });
  it("dispatches the published candidate version and never recomputes it in the view", async () => {
    authoritative.set({ ...EMPTY_STATE, emitterId: "emitter-a", games: [snapshot()] });
    const { container } = render(Library);
    await tick();

    expect(heroCounts(container).total).toBe("1");
    await fireEvent.click(container.querySelector('[data-testid="library-update-all"]')!);

    expect(dispatchApply).toHaveBeenCalledTimes(1);
    const targets = dispatchApply.mock.calls[0][0] as {
      game_id: string;
      target_version: string;
      record: { path: string };
    }[];
    expect(targets).toHaveLength(1);
    expect(targets[0].game_id).toBe("g1");
    expect(targets[0].target_version).toBe("310.4.0");
    expect(targets[0].record.path).toBe("C:\\Games\\Cyberpunk 2077\\bin\\nvngx_dlss.dll");
    // The catalog stores and the per-game DLL records are empty in this test: a view that resolved
    // the target itself would have found no version at all and dispatched nothing.
  });

  it("does not rescan any game after the apply response", async () => {
    authoritative.set({ ...EMPTY_STATE, emitterId: "emitter-a", games: [snapshot()] });
    const { container } = render(Library);
    await tick();

    await fireEvent.click(container.querySelector('[data-testid="library-update-all"]')!);
    await tick();

    expect(stores.rescanGame).not.toHaveBeenCalled();
  });

  it("updates from the observation revision Rust publishes for the applied games", async () => {
    authoritative.set({ ...EMPTY_STATE, emitterId: "emitter-a", games: [snapshot()] });
    const { container } = render(Library);
    await tick();
    await fireEvent.click(container.querySelector('[data-testid="library-update-all"]')!);
    await tick();

    authoritative.set({
      ...EMPTY_STATE,
      emitterId: "emitter-a",
      games: [snapshot({ status: "current", revision: "11", components: [] })],
      operations: [
        {
          id: "op-1",
          plan_id: "plan-1",
          actor: "gui",
          game_ids: ["g1"],
          sequence: "12",
          stage: "completed",
          progress: { files_verified: 1, files_total: 1, measurement_basis: "verified_files" },
          results: [],
          cancel_requested: false,
          error: null,
          state_revision: "11",
          updated_at: "2026-09-17T00:00:10Z",
        } as unknown as AuthoritativeState["operations"][number],
      ],
    });
    await tick();

    expect(heroCounts(container).total).toBe("0");
    expect(container.querySelectorAll(".game-card.status-up_to_date")).toHaveLength(1);
    expect(container.querySelector(".library-summary")?.getAttribute("data-observation-revision")).toBe("11");
    expect(stores.rescanGame).not.toHaveBeenCalled();
  });

  it("keeps unchecked, unknown, no_components and non_actionable out of the updatable presentation", async () => {
    stores.games.set([
      detected("g1", "Alpha"),
      detected("g2", "Beta"),
      detected("g3", "Gamma"),
      detected("g4", "Delta"),
    ]);
    authoritative.set({
      ...EMPTY_STATE,
      emitterId: "emitter-a",
      games: [
        snapshot({ id: "g1", name: "Alpha", status: "unchecked" }),
        snapshot({ id: "g2", name: "Beta", status: "unknown" }),
        snapshot({ id: "g3", name: "Gamma", status: "no_components" }),
        snapshot({ id: "g4", name: "Delta", status: "non_actionable" }),
      ],
    });
    const { container } = render(Library);
    await tick();

    expect(heroCounts(container).total).toBe("0");
    // None of the four states is presented as up to date either; they keep their own status.
    expect(container.querySelectorAll(".game-card.status-up_to_date")).toHaveLength(0);
    expect(container.querySelector('[data-testid="library-update-all"]')).toBeNull();
    expect(container.querySelector(".game-card.status-outdated")).toBeNull();
  });

  it("does not reintroduce a protection KPI in the update action summary", async () => {
    authoritative.set({
      ...EMPTY_STATE,
      emitterId: "emitter-a",
      games: [snapshot()],
      counts: {
        actionable_games: 1,
        actionable_components: 1,
        eligible_restorable_backups: 4,
        active_operations: 0,
      },
    });
    const { container } = render(Library);
    await tick();
    // The action summary does not infer protected games from backup entries.
    expect(heroCounts(container).kpis).toHaveLength(0);

    authoritative.update((state) => ({
      ...state,
      counts: { ...state.counts!, protected_games: 2 } as AuthoritativeState["counts"],
    }));
    await tick();
    expect(heroCounts(container).kpis).toHaveLength(0);
  });
});
