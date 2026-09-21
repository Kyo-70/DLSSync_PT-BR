import { describe, expect, it } from "vitest";
import {
  filterHistoryRows,
  journalStoreRows,
  projectHistoryRows,
  recoveryLabel,
} from "@/views/Journal.svelte";
import type { HistoryView, OperationRecord } from "@/lib/api";

function record(id: string, overrides: Partial<OperationRecord> = {}): OperationRecord {
  return {
    id,
    created_at: "2026-09-17T10:00:00Z",
    actor: "gui",
    kind: "dll_apply",
    status: "succeeded",
    target: "nvngx_dlss.dll",
    summary: `operation ${id}`,
    details: {},
    duration_ms: 42,
    backup_id: null,
    error: null,
    ...overrides,
  } as OperationRecord;
}

function historyView(id: string, overrides: Partial<HistoryView> = {}): HistoryView {
  return {
    id,
    source_store_id: "journal",
    source_record_id: `record-${id}`,
    historical_at: "2026-09-17T11:00:00Z",
    operation_id: "op-1",
    game_id: "game-1",
    component_id: "dlss",
    recovery_outcome: null,
    record: record(`record-${id}`),
    ...overrides,
  } as HistoryView;
}

describe("10.15 — Activity consumes the authoritative history projection", () => {
  it("keeps the published record and never re-derives its classification", () => {
    const rows = projectHistoryRows([
      historyView("h1", { record: record("r1", { kind: "rollback", status: "failed" }) }),
    ]);
    expect(rows).toHaveLength(1);
    expect(rows[0].id).toBe("h1");
    expect(rows[0].record.kind).toBe("rollback");
    expect(rows[0].record.status).toBe("failed");
    expect(rows[0].source).toBe("authoritative");
  });

  it("preserves recovery_outcome exactly as published", () => {
    const rows = projectHistoryRows([
      historyView("h1", { recovery_outcome: "rollback_failed" }),
      historyView("h2", { recovery_outcome: "rolled_back" }),
      historyView("h3"),
    ]);
    expect(rows.map((row) => row.recoveryOutcome)).toEqual([
      "rollback_failed",
      "rolled_back",
      null,
    ]);
  });

  it("prefers the projected historical timestamp and falls back to the record", () => {
    const rows = projectHistoryRows([
      historyView("h1", { historical_at: null, record: record("r1", { created_at: "2026-09-01T00:00:00Z" }) }),
      historyView("h2", { historical_at: "2026-09-17T11:00:00Z" }),
    ]);
    expect(rows[0].id).toBe("h2");
    expect(rows[1].at).toBe("2026-09-01T00:00:00Z");
  });

  it("marks store-loaded rows as the pre-snapshot fallback with no recovery outcome", () => {
    const rows = journalStoreRows([record("r1")]);
    expect(rows[0].source).toBe("journal_store");
    expect(rows[0].recoveryOutcome).toBeNull();
  });

  it("filters on published fields only", () => {
    const rows = projectHistoryRows([
      historyView("h1", { record: record("r1", { kind: "rollback", status: "failed" }) }),
      historyView("h2", { record: record("r2", { kind: "dll_apply", status: "succeeded" }) }),
    ]);
    expect(filterHistoryRows(rows, "rollback", "").map((row) => row.id)).toEqual(["h1"]);
    expect(filterHistoryRows(rows, "", "succeeded").map((row) => row.id)).toEqual(["h2"]);
    expect(filterHistoryRows(rows, "", "").map((row) => row.id)).toEqual(["h1", "h2"]);
  });

  it("labels a recovery outcome without inventing a verdict for an unknown stage", () => {
    expect(recoveryLabel("rolled_back").key).toBe("view.journal.recovery.rolled_back");
    expect(recoveryLabel("rollback_failed").fallbackKey).toBe("view.journal.failed");
    expect(recoveryLabel("applying").fallbackKey).toBe("status.unknown");
  });
});
