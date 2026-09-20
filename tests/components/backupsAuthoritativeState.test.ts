import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get, writable } from "svelte/store";
import { fireEvent, render } from "@testing-library/svelte";
import { tick } from "svelte";
import type { AuthoritativeState } from "@/lib/stateSync";
import type { BackupEntry, BackupView, HistoryView } from "@/lib/api";

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
const restoreBackupMock = vi.fn(async () => undefined);
const listBackupsMock = vi.fn(async () => storeRows);

let storeRows: BackupEntry[] = [];

vi.mock("@/lib/stateSync", () => ({
  authoritativeState: authoritative,
  startStateSync: vi.fn(async () => undefined),
  stopStateSync: vi.fn(async () => undefined),
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    listBackups: listBackupsMock,
    restoreBackup: restoreBackupMock,
    deleteBackup: vi.fn(async () => ({ file_error: null })),
    revealPath: vi.fn(async () => undefined),
    openPath: vi.fn(async () => undefined),
    restoreSystemDriver: vi.fn(async () => ({ success: true, reboot_required: false, message: "" })),
  };
});

vi.mock("@/lib/notifications", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/notifications")>();
  return { ...actual, pushNotification: vi.fn(async () => undefined) };
});

const { backups, games, settings, toasts } = await import("@/lib/stores");
const { default: Backups, RESTORE_VERIFY_TIMEOUT_MS } = await import("@/views/Backups.svelte");

function entry(id: string, overrides: Partial<BackupEntry> = {}): BackupEntry {
  return {
    id,
    game_id: "game-1",
    dll_family: "dlss",
    dll_filename: `${id}.dll`,
    original_path: `C:/games/game-1/${id}.dll`,
    backup_path: `C:/backups/${id}.dll`,
    previous_version: "3.7.0",
    previous_sha256: "a".repeat(64),
    created_at: "2026-09-16T10:00:00Z",
    restored_at: null,
    size_bytes: null,
    backup_type: "dll",
    ...overrides,
  } as BackupEntry;
}

function view(id: string, overrides: Record<string, unknown> = {}): BackupView {
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
  } as BackupView;
}

function rollbackHistory(id: string, backupId: string, failed: boolean): HistoryView {
  return {
    id,
    source_store_id: "journal",
    source_record_id: `record-${id}`,
    historical_at: "2026-09-17T12:00:00Z",
    operation_id: "op-1",
    game_id: "game-1",
    component_id: "dlss",
    recovery_outcome: failed ? "rollback_failed" : "rolled_back",
    record: {
      id: `record-${id}`,
      created_at: "2026-09-17T12:00:00Z",
      actor: "gui",
      kind: "rollback",
      status: failed ? "failed" : "succeeded",
      target: `${backupId}.dll`,
      summary: `Restore ${backupId}`,
      details: {},
      duration_ms: 90,
      backup_id: backupId,
      error: failed ? "target file is locked" : null,
    },
  } as HistoryView;
}

async function renderExpanded(): Promise<HTMLElement> {
  const { container } = render(Backups);
  await tick();
  const head = container.querySelector(".group-head") as HTMLButtonElement | null;
  if (head !== null) {
    await fireEvent.click(head);
    await tick();
  }
  return container;
}

beforeEach(() => {
  authoritative.set(EMPTY);
  storeRows = [];
  backups.set([]);
  games.set([]);
  settings.set(null);
  toasts.set([]);
  restoreBackupMock.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Backups 10.09 — availability comes from the backend", () => {
  it("does not call a snapshot with no recorded size missing", async () => {
    storeRows = [entry("b1", { size_bytes: null })];
    backups.set(storeRows);
    const container = await renderExpanded();

    const chip = container.querySelector('[data-testid="backup-availability"]');
    expect(chip?.getAttribute("data-availability")).toBe("unverified");
    expect(container.textContent).not.toContain("Snapshot missing");
  });

  it("shows an absent snapshot only when the backend published the absence", async () => {
    storeRows = [entry("b1", { size_bytes: 1024 })];
    backups.set(storeRows);
    authoritative.set({
      ...EMPTY,
      emitterId: "emitter-a",
      backups: [view("b1", { availability: "verified_absent", verified_available: false })],
    });
    const container = await renderExpanded();

    const chip = container.querySelector('[data-testid="backup-availability"]');
    expect(chip?.getAttribute("data-availability")).toBe("verified_absent");
    expect(container.textContent).toContain("Snapshot missing");
  });
});

describe("Backups 10.10 — projected eligibility with a visible reason", () => {
  it("offers restore again for a verified retained snapshot after a previous restoration", async () => {
    const restoredAt = "2026-09-17T12:00:00Z";
    storeRows = [entry("b1", { size_bytes: 2048, restored_at: restoredAt })];
    backups.set(storeRows);
    authoritative.set({
      ...EMPTY,
      emitterId: "emitter-a",
      backups: [view("b1", {
        kind: "dll", availability: "verified_present", restored_at: restoredAt,
        restore_eligibility: { eligible: true, reason_code: null },
        last_restore: { verified: true, completed_at: restoredAt },
      })],
    });
    const container = await renderExpanded();
    const button = container.querySelector('[data-testid="backup-restore"]') as HTMLButtonElement;
    expect(button.disabled).toBe(false);
    expect(button.textContent).toContain("Restore again");
  });

  it("disables restore and shows the reason published by the backend", async () => {
    storeRows = [entry("b1", { size_bytes: 2048 })];
    backups.set(storeRows);
    authoritative.set({
      ...EMPTY,
      emitterId: "emitter-a",
      backups: [
        view("b1", {
          availability: "verified_absent",
          verified_available: false,
          restore_eligibility: { eligible: false, reason_code: "snapshot_absent" },
        }),
      ],
    });
    const container = await renderExpanded();

    const reason = container.querySelector('[data-testid="backup-ineligible-reason"]');
    expect(reason).not.toBeNull();
    expect(reason?.getAttribute("data-reason")).toBe("snapshot_absent");
    expect((reason?.textContent ?? "").trim().length).toBeGreaterThan(0);
    const restore = container.querySelector('[data-testid="backup-restore"]') as HTMLButtonElement;
    expect(restore.disabled).toBe(true);
  });

  it("enables restore when the backend published an eligible verdict", async () => {
    storeRows = [entry("b1", { size_bytes: 2048 })];
    backups.set(storeRows);
    authoritative.set({
      ...EMPTY,
      emitterId: "emitter-a",
      backups: [view("b1", { restore_eligibility: { eligible: true, reason_code: null } })],
    });
    const container = await renderExpanded();

    expect(container.querySelector('[data-testid="backup-ineligible-reason"]')).toBeNull();
    const restore = container.querySelector('[data-testid="backup-restore"]') as HTMLButtonElement;
    expect(restore.disabled).toBe(false);
  });
});

describe("Backups 10.11 — success needs verified restoration", () => {
  it("does not report success when the command resolves without published evidence", async () => {
    vi.useFakeTimers();
    storeRows = [entry("b1", { size_bytes: 2048 })];
    backups.set(storeRows);
    authoritative.set({ ...EMPTY, emitterId: "emitter-a", backups: [view("b1")] });
    const container = await renderExpanded();

    const restore = container.querySelector('[data-testid="backup-restore"]') as HTMLButtonElement;
    await fireEvent.click(restore);
    await vi.advanceTimersByTimeAsync(RESTORE_VERIFY_TIMEOUT_MS + 50);
    await tick();

    expect(restoreBackupMock).toHaveBeenCalledWith("b1");
    expect(get(toasts).some((toast) => toast.kind === "success")).toBe(false);
    const outcome = container.querySelector('[data-testid="backup-restore-outcome"]');
    expect(outcome?.getAttribute("data-outcome")).toBe("unverified");
  });

  it("reports success once the backend publishes a verified rollback", async () => {
    storeRows = [entry("b1", { size_bytes: 2048 })];
    backups.set(storeRows);
    authoritative.set({ ...EMPTY, emitterId: "emitter-a", backups: [view("b1")] });
    restoreBackupMock.mockImplementationOnce(async () => {
      authoritative.set({
        ...EMPTY,
        emitterId: "emitter-a",
        backups: [view("b1", { restored_at: "2026-09-17T12:05:00Z", revision: "8" })],
        history: [rollbackHistory("h1", "b1", false)],
      });
      return undefined;
    });
    const container = await renderExpanded();

    const restore = container.querySelector('[data-testid="backup-restore"]') as HTMLButtonElement;
    await fireEvent.click(restore);
    await tick();
    await tick();

    expect(get(toasts).some((toast) => toast.kind === "success")).toBe(true);
  });

  it("keeps a failed restoration visible instead of reporting a completed one", async () => {
    storeRows = [entry("b1", { size_bytes: 2048 })];
    backups.set(storeRows);
    authoritative.set({ ...EMPTY, emitterId: "emitter-a", backups: [view("b1")] });
    restoreBackupMock.mockImplementationOnce(async () => {
      authoritative.set({
        ...EMPTY,
        emitterId: "emitter-a",
        backups: [view("b1", { revision: "8" })],
        history: [rollbackHistory("h1", "b1", true)],
      });
      return undefined;
    });
    const container = await renderExpanded();

    const restore = container.querySelector('[data-testid="backup-restore"]') as HTMLButtonElement;
    await fireEvent.click(restore);
    await tick();
    await tick();

    expect(get(toasts).some((toast) => toast.kind === "success")).toBe(false);
    expect(get(toasts).some((toast) => toast.kind === "danger")).toBe(true);
    const outcome = container.querySelector('[data-testid="backup-restore-outcome"]');
    expect(outcome?.getAttribute("data-outcome")).toBe("failed");
    expect(container.textContent).toContain("target file is locked");
  });
});
