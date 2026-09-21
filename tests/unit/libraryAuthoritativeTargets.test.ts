import { describe, expect, it } from "vitest";
import type { GameSnapshot, OperationSnapshot, StateCounts } from "@/lib/api";
import type { ArtifactDescriptor, ComponentState } from "@/generated/bindings";
import {
  applyObservationRevision,
  authoritativeApplyTargets,
  presentedGameStatus,
  projectedProtectedGames,
  upToDateGameCount,
} from "@/views/Library.svelte";

function candidate(over: Partial<ArtifactDescriptor> = {}): ArtifactDescriptor {
  return {
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
    ...over,
  };
}

function component(over: Partial<ComponentState> = {}): ComponentState {
  return {
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
    candidate: candidate(),
    status: "update_available",
    compatibility: { status: "compatible", reason_code: "catalog_match", evidence: [] },
    support: "official",
    applicability: "applicable",
    owner: null,
    checked_at: "2026-09-17T00:00:00Z",
    revision: "7",
    ...over,
  };
}

function game(over: Partial<GameSnapshot> = {}): GameSnapshot {
  return {
    id: "g1",
    name: "Cyberpunk 2077",
    install_dir: "C:\\Games\\Cyberpunk 2077",
    launcher: "steam",
    status: "update_available",
    revision: "7",
    components: [component()],
    ...over,
  };
}

function operation(over: Partial<OperationSnapshot> = {}): OperationSnapshot {
  return {
    id: "op-1",
    plan_id: "plan-1",
    actor: "gui",
    game_ids: ["g1"],
    sequence: "12",
    stage: "completed",
    progress: {
      files_verified: 1,
      files_total: 1,
      measurement_basis: "verified_files",
    } as OperationSnapshot["progress"],
    results: [],
    cancel_requested: false,
    error: null,
    state_revision: "9",
    updated_at: "2026-09-17T00:00:10Z",
    ...over,
  };
}

const NO_HIDDEN: ReadonlySet<string> = new Set<string>();

describe("Library apply-all membership comes from the authoritative snapshot", () => {
  it("takes the target version from the published candidate, without comparing versions", () => {
    const targets = authoritativeApplyTargets([game()], NO_HIDDEN);
    expect(targets).toHaveLength(1);
    expect(targets[0].target_version).toBe("310.4.0");
    expect(targets[0].catalog_family).toBe("dlss_sr");
    expect(targets[0].record.path).toBe("C:\\Games\\Cyberpunk 2077\\bin\\nvngx_dlss.dll");
    expect(targets[0].record.current_version).toBe("310.1.0");
    expect(targets[0].record.sha256).toBe("b".repeat(64));
    expect(targets[0].game_label).toBe("Steam - Cyberpunk 2077");
  });

  it("keeps a candidate Rust published as the update even when it sorts below the observed version", () => {
    // A view that recomputed the relation would drop this target. Rust decided it is the update to
    // install, so the view carries that decision unchanged.
    const targets = authoritativeApplyTargets(
      [
        game({
          components: [
            component({ observed_version: "310.9.0", candidate: candidate({ package_version: "310.2.1" }) }),
          ],
        }),
      ],
      NO_HIDDEN,
    );
    expect(targets.map((target) => target.target_version)).toEqual(["310.2.1"]);
  });

  it("excludes a component Rust reports as current even when a newer candidate is attached", () => {
    const targets = authoritativeApplyTargets(
      [
        game({
          components: [
            component({ status: "current", observed_version: "310.4.0", candidate: candidate() }),
          ],
        }),
      ],
      NO_HIDDEN,
    );
    expect(targets).toEqual([]);
  });

  it("excludes a component without a published candidate", () => {
    const targets = authoritativeApplyTargets([game({ components: [component({ candidate: null })] })], NO_HIDDEN);
    expect(targets).toEqual([]);
  });

  it("never builds a target from unchecked, unknown, no_components or non_actionable games", () => {
    const snapshots = (["unchecked", "unknown", "no_components", "non_actionable"] as const).map(
      (status, index) => game({ id: `g${index}`, status }),
    );
    expect(authoritativeApplyTargets(snapshots, NO_HIDDEN)).toEqual([]);
  });

  it("drops games the user hides locally, which stays a presentation preference", () => {
    const targets = authoritativeApplyTargets([game(), game({ id: "g2" })], new Set(["g2"]));
    expect(targets.map((target) => target.game_id)).toEqual(["g1"]);
  });
});

describe("Library game status presentation", () => {
  it("maps only update_available to the updatable presentation", () => {
    expect(presentedGameStatus("update_available")).toBe("outdated");
    expect(presentedGameStatus("current")).toBe("up_to_date");
    expect(presentedGameStatus("no_components")).toBe("no_dlls");
    expect(presentedGameStatus("unchecked")).toBe("unknown");
    expect(presentedGameStatus("unknown")).toBe("unknown");
    expect(presentedGameStatus("non_actionable")).toBe("unknown");
    expect(presentedGameStatus(undefined)).toBe("unknown");
  });

  it("counts only the games Rust reports as current, minus locally hidden ones", () => {
    const snapshots = [
      game({ id: "a", status: "current" }),
      game({ id: "b", status: "current" }),
      game({ id: "c", status: "unchecked" }),
      game({ id: "d", status: "non_actionable" }),
    ];
    expect(upToDateGameCount(snapshots, NO_HIDDEN)).toBe(2);
    expect(upToDateGameCount(snapshots, new Set(["b"]))).toBe(1);
  });
});

describe("Protected games are projected, never inferred", () => {
  it("reports unknown while Rust publishes no projection", () => {
    const counts: StateCounts = {
      actionable_games: 1,
      actionable_components: 1,
      eligible_restorable_backups: 4,
      active_operations: 0,
    };
    expect(projectedProtectedGames(null)).toBeNull();
    // Restorable backups are a different quantity and must not stand in for protected games.
    expect(projectedProtectedGames(counts)).toBeNull();
  });

  it("reports the projected count once Rust publishes it", () => {
    const counts = {
      actionable_games: 1,
      actionable_components: 1,
      eligible_restorable_backups: 4,
      active_operations: 0,
      protected_games: 2,
    } as StateCounts;
    expect(projectedProtectedGames(counts)).toBe(2);
  });
});

describe("Post-apply observation revision", () => {
  it("reads the highest revision published for the games the operation touched", () => {
    expect(
      applyObservationRevision([game({ revision: "11" })], [operation()], new Set(["g1"])),
    ).toBe("11");
  });

  it("uses the operation revision when it is ahead of the installed game revision", () => {
    expect(
      applyObservationRevision([game({ revision: "7" })], [operation({ state_revision: "9" })], new Set(["g1"])),
    ).toBe("9");
  });

  it("ignores operations and games outside the applied set", () => {
    expect(
      applyObservationRevision(
        [game({ id: "other", revision: "99" })],
        [operation({ id: "op-2", game_ids: ["other"], state_revision: "98" })],
        new Set(["g1"]),
      ),
    ).toBeNull();
  });

  it("returns null before any apply has been dispatched", () => {
    expect(applyObservationRevision([game()], [operation()], new Set<string>())).toBeNull();
  });

  it("compares revisions over the full u64 range", () => {
    const high = "18446744073709551615";
    expect(
      applyObservationRevision([game({ revision: high })], [operation({ state_revision: "9" })], new Set(["g1"])),
    ).toBe(high);
  });
});
