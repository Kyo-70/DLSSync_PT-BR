/** Cohort C1 — the card and the row read their component chips from the authoritative state.
 *
 *  Both surfaces used to call `dllRelation` on the scanned records. They now read the component
 *  state Rust published: which component carries an applicable update, and which version it
 *  targets. Every test keeps the catalog stores empty, so a surface that still compared versions
 *  locally would produce nothing at all. */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { tick } from "svelte";
import { writable } from "svelte/store";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve, dirname } from "node:path";
import { render } from "@testing-library/svelte";
import type { AuthoritativeState } from "@/lib/stateSync";
import type { DetectedGame, DllRecord, GameSnapshot } from "@/lib/api";
import type { ComponentState, ComponentStatus } from "@/generated/bindings";

const here = dirname(fileURLToPath(import.meta.url));
const srcRoot = resolve(here, "../../frontend/src");
const cardSource = readFileSync(resolve(srcRoot, "components/GameCard.svelte"), "utf8");
const rowSource = readFileSync(resolve(srcRoot, "components/GameListRow.svelte"), "utf8");

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

vi.mock("@/lib/stateSync", () => ({
  authoritativeState: authoritative,
  startStateSync: vi.fn(async () => undefined),
  stopStateSync: vi.fn(async () => undefined),
}));

const stores = await import("@/lib/stores");
const GameCard = (await import("@/components/GameCard.svelte")).default;
const GameListRow = (await import("@/components/GameListRow.svelte")).default;

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

const noop = (): void => undefined;

function surfaceProps(status: string | null): never {
  return {
    game: detected(),
    status,
    onApply: noop,
    onOpenFolder: noop,
    onBlacklist: noop,
    onClick: noop,
  } as never;
}

beforeEach(() => {
  authoritative.set(EMPTY_STATE);
  stores.gameDlls.set({});
  stores.gameDllsLoading.set({});
  // Deliberately empty: nothing on these surfaces may resolve a version from the catalog.
  stores.catalogLatestByKey.set({});
  stores.catalogLatestShas.set({});
});

describe("GameCard — chips read the authoritative component state", () => {
  it("marks only the component Rust reports as an available update, and counts it", async () => {
    authoritative.set({
      ...EMPTY_STATE,
      emitterId: "emitter-a",
      games: [
        snapshot([
          component("dlss_sr", "nvngx_dlss.dll", "update_available"),
          component("dlss_fg", "nvngx_dlssg.dll", "current", { candidate: null }),
        ]),
      ],
    });
    const { container } = render(GameCard, surfaceProps("outdated"));
    await tick();

    const chips = [...container.querySelectorAll(".feature-chip")];
    expect(chips).toHaveLength(2);
    expect(chips.filter((chip) => chip.classList.contains("outdated"))).toHaveLength(1);
    expect(container.querySelector(".status-count")?.textContent?.trim()).toBe("1");
    expect(container.querySelector(".card-status-text")?.textContent?.trim()).toBe("1 update");
  });

  it("never presents newer, unchecked, externally managed, incompatible or not-applicable as an update", async () => {
    authoritative.set({
      ...EMPTY_STATE,
      emitterId: "emitter-a",
      games: [
        snapshot(
          [
            component("dlss_sr", "nvngx_dlss.dll", "newer"),
            component("dlss_fg", "nvngx_dlssg.dll", "unchecked", { candidate: null }),
            component("dlss_rr", "nvngx_dlssd.dll", "externally_managed"),
            component("xess_sr", "libxess.dll", "incompatible"),
            component("fsr_upscaler", "amd_fidelityfx_dx12.dll", "update_available", {
              applicability: "not_applicable",
            }),
          ],
          { status: "non_actionable" },
        ),
      ],
    });
    const { container } = render(GameCard, surfaceProps("unknown"));
    await tick();

    expect(container.querySelectorAll(".feature-chip.outdated")).toHaveLength(0);
    expect(container.querySelector(".status-count")).toBeNull();
    expect(container.querySelector(".card-status-text")?.textContent?.trim()).toBe("Unknown");
  });

  it("keeps the chip inventory but claims no update while Rust published no component state", async () => {
    stores.gameDlls.set({ g1: [record("dlss_sr", "nvngx_dlss.dll", "310.1.0")] });
    const { container } = render(GameCard, surfaceProps("outdated"));
    await tick();

    expect(container.querySelectorAll(".feature-chip")).toHaveLength(1);
    expect(container.querySelectorAll(".feature-chip.outdated")).toHaveLength(0);
    expect(container.querySelector(".status-count")).toBeNull();
    expect(container.querySelector(".card-status-text")?.textContent?.trim()).toBe("Update available");
  });

  it("no longer decides with the local relation helper", () => {
    expect(cardSource).not.toMatch(/dllRelation/);
    expect(cardSource).not.toMatch(/relationContext/);
    expect(cardSource).toMatch(/from "\.\.\/lib\/stateSync"/);
  });
});

describe("GameListRow — chips and the version diff read the authoritative component state", () => {
  it("draws the observed and candidate versions Rust published", async () => {
    authoritative.set({
      ...EMPTY_STATE,
      emitterId: "emitter-a",
      games: [snapshot([component("dlss_sr", "nvngx_dlss.dll", "update_available")])],
    });
    const { container } = render(GameListRow, surfaceProps("outdated"));
    await tick();

    expect(container.querySelector(".ver-old")?.textContent?.trim()).toBe("v310.1.0");
    expect(container.querySelector(".ver-new")?.textContent?.trim()).toBe("v310.4.0");
    expect(container.querySelector(".chip-update")?.textContent?.trim()).toBe("1 update");
    expect(container.querySelectorAll(".chips .feature-chip")).toHaveLength(1);
  });

  it("shows no version diff and no chip for a component that must not be offered", async () => {
    authoritative.set({
      ...EMPTY_STATE,
      emitterId: "emitter-a",
      games: [
        snapshot(
          [component("dlss_sr", "nvngx_dlss.dll", "update_available", { applicability: "not_applicable" })],
          { status: "non_actionable" },
        ),
      ],
    });
    const { container } = render(GameListRow, surfaceProps("unknown"));
    await tick();

    expect(container.querySelector(".ver-empty")?.textContent?.trim()).toBe("—");
    expect(container.querySelectorAll(".chips .feature-chip")).toHaveLength(0);
  });

  it("announces an update without a count while Rust published no component state", async () => {
    stores.gameDlls.set({ g1: [record("dlss_sr", "nvngx_dlss.dll", "310.1.0")] });
    const { container } = render(GameListRow, surfaceProps("outdated"));
    await tick();

    expect(container.querySelector(".chip-update")?.textContent?.trim()).toBe("Update available");
    expect(container.querySelector(".ver-empty")?.textContent?.trim()).toBe("—");
  });

  it("no longer decides with the local relation helper", () => {
    expect(rowSource).not.toMatch(/dllRelation/);
    expect(rowSource).not.toMatch(/relationContext/);
    expect(rowSource).toMatch(/from "\.\.\/lib\/stateSync"/);
  });
});
