import { beforeEach, describe, expect, it, vi } from "vitest";
import { writable } from "svelte/store";
import { render } from "@testing-library/svelte";
import { tick } from "svelte";
import type { AuthoritativeState } from "@/lib/stateSync";
import type { HistoryView, OperationRecord } from "@/lib/api";

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
const listJournalMock = vi.fn(async () => storeRecords);

let storeRecords: OperationRecord[] = [];

vi.mock("@/lib/stateSync", () => ({
  authoritativeState: authoritative,
  startStateSync: vi.fn(async () => undefined),
  stopStateSync: vi.fn(async () => undefined),
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    listJournal: listJournalMock,
    exportJournal: vi.fn(async () => "[]"),
  };
});

const { default: Journal } = await import("@/views/Journal.svelte");

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

beforeEach(() => {
  authoritative.set(EMPTY);
  storeRecords = [];
  listJournalMock.mockClear();
});

describe("Journal 10.15 — the authoritative history projection drives the view", () => {
  it("renders the projected rows and keeps recovery_outcome on the entry", async () => {
    authoritative.set({
      ...EMPTY,
      emitterId: "emitter-a",
      history: [
        historyView("h1", {
          recovery_outcome: "rollback_failed",
          record: record("r1", { kind: "rollback", status: "failed", summary: "Restore nvngx_dlss.dll" }),
        }),
      ],
    });

    const { container } = render(Journal);
    await tick();
    await tick();

    const entries = container.querySelectorAll(".journal-entry");
    expect(entries).toHaveLength(1);
    expect(entries[0].getAttribute("data-recovery")).toBe("rollback_failed");
    expect(entries[0].getAttribute("data-source")).toBe("authoritative");
    expect(container.textContent).toContain("Restore nvngx_dlss.dll");
  });

  it("shows no recovery marker when the projection published none", async () => {
    authoritative.set({ ...EMPTY, emitterId: "emitter-a", history: [historyView("h1")] });

    const { container } = render(Journal);
    await tick();
    await tick();

    const entry = container.querySelector(".journal-entry");
    expect(entry?.hasAttribute("data-recovery")).toBe(false);
    expect(container.querySelector('[data-testid="journal-recovery"]')).toBeNull();
  });

  it("falls back to the stored journal only while the projection has no history", async () => {
    storeRecords = [record("r9", { summary: "Scanned library" })];

    const { container } = render(Journal);
    await tick();
    await tick();
    await tick();

    const entry = container.querySelector(".journal-entry");
    expect(entry?.getAttribute("data-source")).toBe("journal_store");
    expect(container.textContent).toContain("Scanned library");
  });
});
