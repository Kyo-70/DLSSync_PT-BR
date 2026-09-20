import { describe, it, expect } from "vitest";
import type { DriverHealth, DriverStatusReport, DriverUpdateAction, DriverUpdateStatus, GpuVendor } from "@/lib/api";
import {
  driverCardState,
  driverHealth,
  driverPrimaryAction,
  driverUpdateCount,
  installProgressView,
} from "@/lib/drivers";

function report(
  vendor: GpuVendor,
  status: DriverUpdateStatus,
  opts: {
    download?: string;
    notes?: string | null;
    size?: number;
    health?: DriverHealth;
    action?: DriverUpdateAction;
    rebootPending?: string | null;
  } = {},
): DriverStatusReport {
  const hasLatest = status === "update_available" || status === "up_to_date";
  return {
    device: { class: "gpu", vendor, pci_vendor_id: 0, pci_device_id: 0, model: `${vendor} GPU` },
    installed: { packed: 1, display: "1.0", raw: "1.0" },
    latest: hasLatest
      ? {
          vendor,
          version: { packed: 2, display: "2.0", raw: "2.0" },
          channel: "stable",
          display_version: null,
          is_beta: false,
          download_url: opts.download ?? "",
          size_bytes: opts.size ?? 0,
          signature_subject: "x",
          released_at: null,
          release_notes_url: opts.notes === undefined ? "https://vendor.test/notes" : opts.notes,
          changelog: null,
        }
      : null,
    status,
    health: opts.health ?? ({
      update_available: "outdated",
      up_to_date: "current",
      unsupported: "unsupported",
      unknown: "unknown",
    } as Record<DriverUpdateStatus, DriverHealth>)[status],
    action: opts.action ?? { kind: "none", help_url: null },
    reboot_pending: opts.rebootPending ?? null,
  };
}

describe("10.06 - health semantics come from the backend status enum", () => {
  it("maps every published status to its own health, with no shared bucket", () => {
    expect(driverHealth(report("nvidia", "up_to_date"))).toBe("current");
    expect(driverHealth(report("nvidia", "update_available"))).toBe("outdated");
    expect(driverHealth(report("nvidia", "unsupported"))).toBe("unsupported");
    expect(driverHealth(report("nvidia", "unknown"))).toBe("unknown");
  });

  it("keeps `unsupported` distinct from `unknown` instead of folding both into one state", () => {
    expect(driverHealth(report("nvidia", "unsupported"))).not.toBe(driverHealth(report("nvidia", "unknown")));
  });

  it("fails closed for a status the frontend does not recognise", () => {
    expect(driverHealth(report("nvidia", "unknown", { health: "future_backend_value" as DriverHealth }))).toBe("unknown");
  });

  it("counts updates from the backend status only", () => {
    const reports = [
      report("nvidia", "update_available", { download: "https://nv.test/d.exe" }),
      report("amd", "update_available"),
      report("intel", "up_to_date"),
      report("other", "unsupported"),
    ];
    expect(driverUpdateCount(reports)).toBe(2);
  });
});

describe("10.06 - pending-restart visibility comes from the recorded install outcome", () => {
  const pending = report("nvidia", "update_available", { download: "https://nv.test/d.exe", rebootPending: "590.00" });

  it("shows the pending restart whenever the recorded outcome staged one", () => {
    const state = driverCardState(pending, { installing: false });
    expect(state).toEqual({ kind: "reboot_pending", version: "590.00" });
  });

  it("does not re-derive visibility from the reported status", () => {
    const upToDate = report("nvidia", "up_to_date", { download: "https://nv.test/d.exe", rebootPending: "590.00" });
    const state = driverCardState(upToDate, { installing: false });
    expect(state).toEqual({ kind: "reboot_pending", version: "590.00" });
  });

  it("hides it when no outcome staged a restart", () => {
    const state = driverCardState({ ...pending, reboot_pending: null }, { installing: false });
    expect(state.kind).toBe("action");
  });

  it("an active install outranks both", () => {
    const state = driverCardState(pending, { installing: true });
    expect(state).toEqual({ kind: "installing" });
  });
});

describe("10.07 - install vs open-page comes from the published release", () => {
  it("offers an install only when the vendor source published a direct download URL", () => {
    const action = driverPrimaryAction(
      report("nvidia", "update_available", { action: { kind: "install", download_url: "https://nv.test/d.exe", size_bytes: 900 } }),
    );
    expect(action).toEqual({
      kind: "install",
      download_url: "https://nv.test/d.exe",
      size_bytes: 900,
    });
  });

  it("routes AMD to the observed official page, because the source publishes no download URL", () => {
    const action = driverPrimaryAction(
      report("amd", "update_available", { action: { kind: "open_page", url: "https://amd.test/drivers" } }),
    );
    expect(action).toEqual({ kind: "open_page", url: "https://amd.test/drivers" });
  });

  it("treats a blank download URL as no download, not as a link to fabricate", () => {
    const action = driverPrimaryAction(
      report("amd", "update_available", { action: { kind: "none", help_url: null } }),
    );
    expect(action.kind).toBe("none");
  });

  it("offers no action and no link when neither a download nor a page was published", () => {
    const action = driverPrimaryAction(report("intel", "update_available", { action: { kind: "none", help_url: null } }));
    expect(action).toEqual({ kind: "none", help_url: null });
  });

  it("never offers an install or a page for a status that is not an available update", () => {
    for (const status of ["up_to_date", "unknown", "unsupported"] as DriverUpdateStatus[]) {
      const action = driverPrimaryAction(
        report("nvidia", status, { download: "https://nv.test/d.exe" }),
      );
      expect(action.kind).toBe("none");
    }
  });

  it("points unknown and unsupported adapters at the vendor's own finder page", () => {
    const unknown = driverPrimaryAction(report("intel", "unknown", { action: { kind: "none", help_url: "https://intel.com/help" } }));
    const unsupported = driverPrimaryAction(report("amd", "unsupported", { action: { kind: "none", help_url: "https://amd.com/help" } }));
    expect(unknown.kind === "none" && unknown.help_url).toContain("intel.com");
    expect(unsupported.kind === "none" && unsupported.help_url).toContain("amd.com");
  });
});

describe("10.08 - a failed install is a failure, not a finished bar", () => {
  it("carries no percentage for a failed stage, whatever fraction was recorded", () => {
    const view = installProgressView("failed", 1, "Install failed");
    expect(view).toEqual({ kind: "failed", message: "Install failed" });
    expect(view).not.toHaveProperty("percent");
  });

  it("treats a cancelled stage the same way", () => {
    expect(installProgressView("cancelled", 0.5, "Cancelled").kind).toBe("failed");
  });

  it("stays indeterminate when the backend published no fraction", () => {
    expect(installProgressView("installing", null, "Installing")).toEqual({
      kind: "indeterminate",
      message: "Installing",
    });
  });

  it("projects a published fraction and clamps it to the 0-100 range", () => {
    expect(installProgressView("downloading", 0.425, "x")).toEqual({
      kind: "determinate",
      percent: 43,
      message: "x",
    });
    expect(installProgressView("downloading", 4, "x")).toMatchObject({ percent: 100 });
    expect(installProgressView("downloading", -1, "x")).toMatchObject({ percent: 0 });
  });
});
