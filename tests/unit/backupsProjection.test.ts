import { describe, expect, it } from "vitest";
import {
  backupKind,
  projectAvailability,
  projectBackupRows,
  projectEligibility,
  restoreEvidence,
  restoreOffered,
  type ProjectedBackup,
  type RestoreAttempt,
} from "@/views/Backups.svelte";
import type { BackupEntry, BackupView, HistoryView } from "@/lib/api";

function entry(id: string, overrides: Partial<BackupEntry> = {}): BackupEntry {
  return {
    id,
    game_id: "game-1",
    dll_family: "dlss",
    dll_filename: "nvngx_dlss.dll",
    original_path: `C:/games/game-1/${id}.dll`,
    backup_path: `C:/backups/${id}.dll`,
    previous_version: "3.7.0",
    previous_sha256: "a".repeat(64),
    created_at: "2026-09-16T10:00:00Z",
    restored_at: null,
    ...overrides,
  } as BackupEntry;
}

function view(id: string, overrides: Partial<ProjectedBackup> = {}): ProjectedBackup {
  return {
    id,
    game_id: "game-1",
    component_id: "dlss",
    operation_id: null,
    original_path: `C:/games/game-1/${id}.dll`,
    backup_path: `C:/backups/${id}.dll`,
    sha256: "a".repeat(64),
    verified_available: true,
    restored_at: null,
    revision: "7",
    ...overrides,
  };
}

function history(id: string, overrides: Partial<HistoryView> = {}): HistoryView {
  return {
    id,
    source_store_id: "journal",
    source_record_id: `record-${id}`,
    historical_at: "2026-09-17T12:00:00Z",
    operation_id: "op-1",
    game_id: "game-1",
    component_id: "dlss",
    recovery_outcome: null,
    record: {
      id: `record-${id}`,
      created_at: "2026-09-17T12:00:00Z",
      actor: "gui",
      kind: "rollback",
      status: "succeeded",
      target: "nvngx_dlss.dll",
      summary: "Restored nvngx_dlss.dll",
      details: {},
      duration_ms: 120,
      backup_id: "b1",
      error: null,
    },
    ...overrides,
  } as HistoryView;
}

describe("10.09 — availability and kind come from the backend", () => {
  it("treats a missing size as unknown availability, not as an absent file", () => {
    const rows = projectBackupRows([entry("b1", { size_bytes: null })], []);
    expect(rows[0].availability).toBe("unverified");
    expect(rows[0].entry.size_bytes).toBeNull();
  });

  it("reports a verified present snapshot only when the backend verified it", () => {
    expect(projectAvailability(view("b1", { verified_available: true }))).toBe("verified_present");
  });

  it("never turns the two-state boolean `false` into a verified absence", () => {
    expect(projectAvailability(view("b1", { verified_available: false }))).toBe("unverified");
  });

  it("consumes the three-state availability once the backend publishes it", () => {
    expect(projectAvailability(view("b1", { availability: "verified_absent" }))).toBe("verified_absent");
    expect(projectAvailability(view("b1", { availability: "unverified", verified_available: true }))).toBe(
      "unverified",
    );
  });

  it("takes the snapshot kind from the authoritative projection when present", () => {
    expect(backupKind(entry("b1", { backup_type: "dll" }), view("b1", { kind: "driver_package" }))).toBe(
      "driver_package",
    );
  });

  it("normalises the backend column once and keeps an unrecognised value unknown", () => {
    expect(backupKind(entry("b1", { backup_type: "driver_package" }), undefined)).toBe("driver_package");
    expect(backupKind(entry("b1", { backup_type: "dll" }), undefined)).toBe("game_dll");
    expect(backupKind(entry("b1", { backup_type: "future_kind" }), undefined)).toBe("unknown");
  });
});

describe("10.10 — restore eligibility is projected, with a reason when blocked", () => {
  it("is unknown while the backend has published nothing for the row", () => {
    expect(projectEligibility(undefined)).toEqual({ state: "unknown" });
  });

  it("uses the projected verdict and keeps its reason code", () => {
    const blocked = projectEligibility(
      view("b1", { restore_eligibility: { eligible: false, reason_code: "snapshot_absent" } }),
    );
    expect(blocked).toEqual({ state: "blocked", reasonCode: "snapshot_absent" });
    expect(
      projectEligibility(view("b1", { restore_eligibility: { eligible: true, reason_code: null } })),
    ).toEqual({ state: "eligible" });
  });

  it("falls back to published facts only: verified present and not restored is eligible", () => {
    expect(projectEligibility(view("b1"))).toEqual({ state: "eligible" });
    expect(projectEligibility(view("b1", { restored_at: "2026-09-17T09:00:00Z" }))).toEqual({
      state: "restored",
    });
    expect(projectEligibility(view("b1", { availability: "verified_absent" }))).toEqual({
      state: "blocked",
      reasonCode: "snapshot_absent",
    });
    expect(projectEligibility(view("b1", { verified_available: false }))).toEqual({ state: "unknown" });
  });

  it("offers the restore control unless the backend blocked it", () => {
    expect(restoreOffered({ state: "eligible" })).toBe(true);
    expect(restoreOffered({ state: "unknown" })).toBe(true);
    expect(restoreOffered({ state: "restored" })).toBe(false);
    expect(restoreOffered({ state: "blocked", reasonCode: "snapshot_absent" })).toBe(false);
  });

  it("prefers the authoritative restored timestamp over the store row", () => {
    const rows = projectBackupRows(
      [entry("b1", { restored_at: "2026-01-01T00:00:00Z" })],
      [view("b1") as BackupView],
    );
    expect(rows[0].restoredAt).toBeNull();
  });
});

describe("10.11 — a resolved call is not a restoration", () => {
  const attempt: RestoreAttempt = {
    backupId: "b1",
    restoredAtBefore: null,
    revisionBefore: "7",
    knownHistoryIds: new Set<string>(["h0"]),
  };

  it("returns no evidence when nothing was published after the attempt", () => {
    expect(restoreEvidence(attempt, view("b1"), [])).toBeNull();
  });

  it("ignores a rollback record that already existed before the attempt", () => {
    expect(restoreEvidence(attempt, view("b1"), [history("h0")])).toBeNull();
  });

  it("accepts a new verified rollback record as success", () => {
    const outcome = restoreEvidence(attempt, view("b1"), [
      history("h1", { recovery_outcome: "rolled_back" }),
    ]);
    expect(outcome).toEqual({ state: "verified", at: "2026-09-17T12:00:00Z" });
  });

  it("keeps a failed rollback visible as a failure", () => {
    const failed = history("h2", {
      recovery_outcome: "rollback_failed",
      record: {
        ...history("h2").record,
        status: "failed",
        error: "target file is locked",
      },
    });
    expect(restoreEvidence(attempt, view("b1"), [failed])).toEqual({
      state: "failed",
      detail: "target file is locked",
    });
  });

  it("does not treat a timestamp alone as verified restoration", () => {
    expect(restoreEvidence(attempt, view("b1", { restored_at: "2026-09-17T12:05:00Z" }), [])).toBeNull();
  });

  it("does not reuse a previous success when an unrelated revision changes", () => {
    const previous = { verified: true, at: "2026-09-17T12:00:00Z", detail: null };
    expect(restoreEvidence({ ...attempt, lastRestoreBefore: JSON.stringify(previous) },
      view("b1", { revision: "8", last_restore: previous }), [])).toBeNull();
  });

  it("reads the explicit restore verification once the revision moved", () => {
    const verified = view("b1", { revision: "8", last_restore: { verified: true, at: "2026-09-17T12:06:00Z" } });
    expect(restoreEvidence(attempt, verified, [])).toEqual({ state: "verified", at: "2026-09-17T12:06:00Z" });
    const rejected = view("b1", {
      revision: "8",
      last_restore: { verified: false, detail: "hash mismatch after copy" },
    });
    expect(restoreEvidence(attempt, rejected, [])).toEqual({
      state: "failed",
      detail: "hash mismatch after copy",
    });
  });
});
