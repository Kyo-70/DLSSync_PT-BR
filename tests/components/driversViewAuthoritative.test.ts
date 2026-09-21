import { describe, it, expect, vi, beforeEach } from "vitest";
import { tick } from "svelte";
import { render } from "@testing-library/svelte";
import Drivers from "@/views/Drivers.svelte";
import {
  driverReports,
  driverRebootPending,
  driverInstall,
  systemDriverGroups,
  systemDriverInstall,
} from "@/lib/stores";
import type {
  DriverStatusReport,
  DriverUpdateStatus,
  GpuVendor,
  SystemDeviceGroup,
} from "@/lib/api";

vi.mock("@/lib/stores", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/stores")>();
  return {
    ...actual,
    loadDriverUpdates: vi.fn(async () => undefined),
    loadSystemDrivers: vi.fn(async () => undefined),
  };
});

function report(
  vendor: GpuVendor,
  status: DriverUpdateStatus,
  opts: { download?: string; notes?: string | null } = {},
): DriverStatusReport {
  return {
    device: { class: "gpu", vendor, pci_vendor_id: 0, pci_device_id: 0, model: `${vendor} adapter` },
    installed: { packed: 1, display: "1.0", raw: "1.0" },
    latest:
      status === "update_available"
        ? {
            vendor,
            version: { packed: 2, display: "2.0", raw: "2.0" },
            channel: "stable",
            display_version: null,
            is_beta: false,
            download_url: opts.download ?? "",
            size_bytes: 0,
            signature_subject: "x",
            released_at: null,
            release_notes_url: opts.notes === undefined ? "https://vendor.test/notes" : opts.notes,
            changelog: null,
          }
        : null,
    status,
    health: status === "update_available" ? "outdated" : status === "up_to_date" ? "current" : status,
    action: status !== "update_available" ? { kind: "none", help_url: null }
      : opts.download ? { kind: "install", download_url: opts.download, size_bytes: 0 }
      : (opts.notes === undefined ? "https://vendor.test/notes" : opts.notes)
        ? { kind: "open_page", url: opts.notes ?? "https://vendor.test/notes" }
        : { kind: "none", help_url: null },
    reboot_pending: null,
  } as DriverStatusReport;
}

const UPDATE_ID = "1111-2222:3";

function systemGroup(): SystemDeviceGroup {
  return {
    class: "audio",
    label: "Audio",
    updates: [
      {
        update_id: UPDATE_ID,
        title: "Realtek Audio Driver Update (1.2.3)",
        class: "audio",
        provider: "Realtek",
        driver_version: "1.2.3",
        driver_date: "2026-01-01",
        hardware_id: null,
        size_bytes: 0,
        target_device: "Realtek Audio",
        current_version: "1.2.2",
        target_inf: null,
        target_hardware_id: null,
        support_url: null,
      },
    ],
  } as SystemDeviceGroup;
}

beforeEach(() => {
  driverReports.set([]);
  driverRebootPending.set({});
  driverInstall.set({ vendor: null, stage: null, message: "", fraction: null });
  systemDriverGroups.set([]);
  systemDriverInstall.set({ updateId: null, stage: null, message: "", fraction: null });
});

describe("10.07 - the adapter card offers the action the published release supports", () => {
  it("offers an in-app install when the vendor source published a download URL", async () => {
    const { container } = render(Drivers);
    driverReports.set([report("nvidia", "update_available", { download: "https://nv.test/d.exe" })]);
    await tick();
    expect(container.querySelector('[data-testid="gpu-install-action"]')).not.toBeNull();
    expect(container.querySelector('[data-testid="gpu-open-page-action"]')).toBeNull();
  });

  it("opens the observed official page for AMD, which publishes no download URL", async () => {
    const { container } = render(Drivers);
    driverReports.set([
      report("amd", "update_available", { download: "", notes: "https://amd.test/drivers" }),
    ]);
    await tick();
    expect(container.querySelector('[data-testid="gpu-open-page-action"]')).not.toBeNull();
    expect(container.querySelector('[data-testid="gpu-install-action"]')).toBeNull();
  });

  it("offers no action at all when neither a download nor a page was published", async () => {
    const { container } = render(Drivers);
    driverReports.set([report("intel", "update_available", { download: "", notes: null })]);
    await tick();
    expect(container.querySelector('[data-testid="gpu-install-action"]')).toBeNull();
    expect(container.querySelector('[data-testid="gpu-open-page-action"]')).toBeNull();
    expect(container.querySelector('[data-testid="gpu-status-only"]')).not.toBeNull();
  });
});

describe("10.06 - health and pending restart follow the backend", () => {
  it("preserves unsupported health on the adapter card", async () => {
    const { container } = render(Drivers);
    driverReports.set([report("other", "unsupported")]);
    await tick();
    const chip = container.querySelector('.driver-card[data-health="unsupported"]');
    expect(chip).not.toBeNull();
    expect(chip?.textContent).toContain('Not supported');
    expect(container.querySelector('.health-chip.is-device .state-dot[data-state="beta"]')).toBeNull();
  });

  it("keeps the pending restart visible from the recorded outcome alone", async () => {
    const { container } = render(Drivers);
    driverReports.set([report("nvidia", "up_to_date")]);
    driverRebootPending.set({ nvidia: "590.00" });
    driverReports.update((reports) => reports.map((report) => ({ ...report, reboot_pending: "590.00" })));
    await tick();
    expect(container.querySelector('[data-testid="gpu-reboot-pending"]')).not.toBeNull();
  });
});

describe("10.08 - a failed install renders as a failure, with no fabricated bar", () => {
  it("renders no progress bar and no 100% width for a failed system-driver install", async () => {
    const { container } = render(Drivers);
    systemDriverGroups.set([systemGroup()]);
    systemDriverInstall.set({
      updateId: UPDATE_ID,
      stage: "failed",
      message: "Install failed",
      fraction: 1,
    });
    await tick();
    const live = container.querySelector('[data-testid="sys-install-live"]');
    expect(live).not.toBeNull();
    expect(live?.getAttribute("data-progress")).toBe("failed");
    expect(container.querySelector('[data-testid="sys-install-bar"]')).toBeNull();
    expect(container.querySelector('[data-testid="sys-install-failed"]')).not.toBeNull();
    expect(container.innerHTML).not.toContain("width: 100%");
  });

  it("still renders a determinate bar while an install is progressing", async () => {
    const { container } = render(Drivers);
    systemDriverGroups.set([systemGroup()]);
    systemDriverInstall.set({
      updateId: UPDATE_ID,
      stage: "downloading",
      message: "Downloading",
      fraction: 0.5,
    });
    await tick();
    const bar = container.querySelector('[data-testid="sys-install-bar"]');
    expect(bar).not.toBeNull();
    expect(bar?.querySelector(".install-fill")?.getAttribute("style")).toContain("50%");
  });
});
