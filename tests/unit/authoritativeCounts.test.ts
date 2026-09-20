import { beforeEach, describe, expect, it, vi } from "vitest";
import { get, writable } from "svelte/store";
import type { AuthoritativeState } from "@/lib/stateSync";
import type { BackupEntry, DetectedGame } from "@/lib/api";

const EMPTY: AuthoritativeState = {
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

const authoritative = writable<AuthoritativeState>(EMPTY);

vi.mock("@/lib/stateSync", () => ({
  authoritativeState: authoritative,
  startStateSync: vi.fn(async () => undefined),
  stopStateSync: vi.fn(async () => undefined),
}));

const { games, backups, hiddenIds, settings, outdatedGameCount, restorableBackupCount, gameDlls, outdatedDllItems, catalogLatestByKey } =
  await import("@/lib/stores");

function game(id: string): DetectedGame {
  return { id, name: id, install_dir: `C:/games/${id}`, launcher: "steam" } as DetectedGame;
}

function backup(id: string, restoredAt: string | null): BackupEntry {
  return {
    id,
    original_path: `C:/games/${id}/nvngx_dlss.dll`,
    backup_path: `C:/backups/${id}.dll`,
    restored_at: restoredAt,
  } as BackupEntry;
}

describe("authoritative counts", () => {
  it("does not resurrect local updates after the authoritative library becomes empty", () => {
    games.set([game("g1")]);
    gameDlls.set({ g1: [{ family: "dlss", path: "C:/games/g1/nvngx_dlss.dll", current_version: "1.0", sha256: null, file_description: null }] });
    catalogLatestByKey.set({ dlss: "2.0" });
    authoritative.set({ ...EMPTY, emitterId: "emitter-a", games: [] });
    expect(get(outdatedGameCount)).toBe(0);
    expect(outdatedDllItems()).toEqual([]);
  });

  beforeEach(() => {
    authoritative.set(EMPTY);
    games.set([]);
    backups.set([]);
    gameDlls.set({});
    settings.set(null);
  });

  it("falls back to the local restore count before the first snapshot", () => {
    backups.set([backup("a", null), backup("b", "2026-09-17T00:00:00Z"), backup("c", null)]);
    expect(get(restorableBackupCount)).toBe(2);
  });

  it("prefers the Rust eligibility count once a snapshot is installed", () => {
    backups.set([backup("a", null), backup("b", null), backup("c", null)]);
    authoritative.set({
      ...EMPTY,
      emitterId: "emitter-a",
      counts: {
        actionable_games: 0,
        actionable_components: 0,
        // Rust also requires the stored bytes to be verified available, so its count is lower than
        // the three rows whose `restored_at` is null.
        eligible_restorable_backups: 1,
        active_operations: 0,
      },
    });
    expect(get(restorableBackupCount)).toBe(1);
  });

  it("counts games with an applicable update from the authoritative snapshot", () => {
    games.set([game("g1"), game("g2"), game("g3")]);
    authoritative.set({
      ...EMPTY,
      emitterId: "emitter-a",
      games: [
        { id: "g1", name: "g1", install_dir: "C:/games/g1", status: "update_available" },
        { id: "g2", name: "g2", install_dir: "C:/games/g2", status: "unknown" },
        { id: "g3", name: "g3", install_dir: "C:/games/g3", status: "update_available" },
      ],
    });
    expect(get(outdatedGameCount)).toBe(2);
  });

  it("keeps hiding a game as a local preference, not a domain decision", () => {
    games.set([game("g1"), game("g2")]);
    // `hiddenIds` reads the persisted blacklist, which is the user's own presentation preference.
    // The stub carries the fields the status derivations read, so this test exercises the count and
    // not a missing-settings crash.
    settings.set({
      blacklist: ["g1"],
      game_preferences: {},
      update_prefs: null,
      ui_prefs: { favorite_game_ids: [] },
    } as unknown as Parameters<typeof settings.set>[0]);
    authoritative.set({
      ...EMPTY,
      emitterId: "emitter-a",
      games: [
        { id: "g1", name: "g1", install_dir: "C:/games/g1", status: "update_available" },
        { id: "g2", name: "g2", install_dir: "C:/games/g2", status: "update_available" },
      ],
    });
    expect(get(hiddenIds).has("g1")).toBe(true);
    expect(get(outdatedGameCount)).toBe(1);
  });

  it("does not treat unchecked, no_components or non_actionable as an available update", () => {
    games.set([game("g1"), game("g2"), game("g3")]);
    authoritative.set({
      ...EMPTY,
      emitterId: "emitter-a",
      games: [
        { id: "g1", name: "g1", install_dir: "C:/games/g1", status: "unchecked" },
        { id: "g2", name: "g2", install_dir: "C:/games/g2", status: "no_components" },
        { id: "g3", name: "g3", install_dir: "C:/games/g3", status: "non_actionable" },
      ],
    });
    expect(get(outdatedGameCount)).toBe(0);
  });
});
