import type { DriverHealth, DriverStatusReport, DriverUpdateAction, DriverUpdateStatus } from "./api";

export type DriverTone = "success" | "accent" | "muted" | "warning";

const STATUS_LABEL: Record<DriverUpdateStatus, string> = {
  update_available: "Update available",
  up_to_date: "Up to date",
  unknown: "Unknown",
  unsupported: "Not supported",
};

const STATUS_TONE: Record<DriverUpdateStatus, DriverTone> = {
  update_available: "accent",
  up_to_date: "success",
  unknown: "warning",
  unsupported: "muted",
};

export function driverStatusLabel(status: DriverUpdateStatus): string {
  return STATUS_LABEL[status] ?? STATUS_LABEL.unknown;
}

export function driverStatusTone(status: DriverUpdateStatus): DriverTone {
  return STATUS_TONE[status] ?? "muted";
}

export function hasDriverUpdate(report: DriverStatusReport): boolean {
  return driverHealth(report) === "outdated";
}

export function driverHealth(report: DriverStatusReport): DriverHealth {
  const health = report.health;
  if (health === "current" || health === "outdated" || health === "unsupported" || health === "unknown") return health;
  return "unknown";
}

export function driverPrimaryAction(report: DriverStatusReport): DriverUpdateAction {
  return report.action ?? { kind: "none", help_url: null };
}

export type DriverCardState =
  | { kind: "installing" }
  | { kind: "reboot_pending"; version: string }
  | { kind: "action"; action: DriverUpdateAction };

export function driverCardState(
  report: DriverStatusReport,
  operation: { installing: boolean; rebootPendingVersion?: string },
): DriverCardState {
  if (operation.installing) return { kind: "installing" };
  const rebootPending = report.reboot_pending === undefined ? operation.rebootPendingVersion : report.reboot_pending;
  if (rebootPending) {
    return { kind: "reboot_pending", version: rebootPending };
  }
  return { kind: "action", action: driverPrimaryAction(report) };
}

/** How an install operation is presented.
 *
 *  A failed stage is a failure, not a finished bar: it carries no fraction, so no progress width is
 *  fabricated from it. An operation without a published fraction is indeterminate. */
export type InstallProgressView =
  | { kind: "failed"; message: string }
  | { kind: "indeterminate"; message: string }
  | { kind: "determinate"; percent: number; message: string };

export function installProgressView(
  stage: string | null,
  fraction: number | null | undefined,
  message: string,
): InstallProgressView {
  if (stage === "failed" || stage === "cancelled") return { kind: "failed", message };
  if (fraction === null || fraction === undefined || Number.isNaN(fraction)) {
    return { kind: "indeterminate", message };
  }
  const percent = Math.round(Math.min(1, Math.max(0, fraction)) * 100);
  return { kind: "determinate", percent, message };
}

/** Page to open for a driver: the device's own release-notes page, falling back
 *  to a changelog notes PDF. Used both for "Release notes" and AMD's
 *  "Open download page" action. */
export function driverPageUrl(report: DriverStatusReport): string | null {
  return report.latest?.release_notes_url ?? report.latest?.changelog?.notes_page_url ?? null;
}

/** An update is in-app installable only when the vendor gives a direct download
 *  URL (NVIDIA, Intel). */
export function canInstall(report: DriverStatusReport): boolean {
  return driverPrimaryAction(report).kind === "install";
}

/** An update exists but there is no safe direct download (AMD: the real `.exe`
 *  is EULA-gated and its filename is unstable) — route the user to the official
 *  page instead of fabricating a link. */
export function isOpenPageOnly(report: DriverStatusReport): boolean {
  return driverPrimaryAction(report).kind === "open_page";
}

export function vendorHelpUrl(vendor: DriverStatusReport["device"]["vendor"]): string {
  return vendor === "intel"
    ? "https://www.intel.com/content/www/us/en/support/detect.html"
    : vendor === "nvidia"
      ? "https://www.nvidia.com/Download/index.aspx"
      : vendor === "amd"
        ? "https://www.amd.com/en/support/download/drivers.html"
        : "https://support.microsoft.com/windows/update-drivers-in-windows-ec62f46c-ff14-c91d-eead-d7126dc1f7b6";
}

export function driverUpdateCount(reports: DriverStatusReport[]): number {
  return reports.reduce((total, report) => (hasDriverUpdate(report) ? total + 1 : total), 0);
}

export function sortDriverReports(reports: DriverStatusReport[]): DriverStatusReport[] {
  const rank: Record<DriverHealth, number> = {
    outdated: 0,
    unknown: 1,
    current: 2,
    unsupported: 3,
  };
  return [...reports].sort((a, b) => {
    const healthRank = (rank[driverHealth(a)] ?? 9) - (rank[driverHealth(b)] ?? 9);
    return healthRank !== 0 ? healthRank : a.device.model.localeCompare(b.device.model);
  });
}
