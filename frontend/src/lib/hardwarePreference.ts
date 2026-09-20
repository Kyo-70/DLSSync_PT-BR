import type { DriverStatusReport, SystemInfo } from "./api";
import { familyMeta } from "./familyMeta";

export type GraphicsVendor = "nvidia" | "amd" | "intel";
export interface HardwarePreference {
  known: boolean;
  vendors: ReadonlySet<GraphicsVendor>;
}

/** Display/default-selection affinity only. Compatibility and eligibility still come from Rust. */
export function hardwarePreferenceFor(
  info: Pick<SystemInfo, "gpus"> | null,
  reports: readonly DriverStatusReport[] = [],
): HardwarePreference {
  const vendors = new Set<GraphicsVendor>();
  for (const vendor of [...(info?.gpus ?? []).map(gpu => gpu.vendor), ...reports.map(report => report.device.vendor)]) {
    if (vendor === "nvidia" || vendor === "amd" || vendor === "intel") vendors.add(vendor);
  }
  return { known: vendors.size > 0, vendors };
}

export function preferredVendor(vendor: string, preference: HardwarePreference): boolean {
  // Missing hardware evidence cannot establish that a library is irrelevant.
  if (!preference.known) return true;
  return vendor === "microsoft" || preference.vendors.has(vendor as GraphicsVendor);
}

export function preferredFamily(family: string, preference: HardwarePreference): boolean {
  const metadata = familyMeta(family);
  return !preference.known || (metadata !== null && preferredVendor(metadata.vendor, preference));
}

/** Unknown hardware stays browsable, but never creates an automatic/default update selection. */
export function defaultUpdateFamily(family: string, preference: HardwarePreference): boolean {
  return preference.known && preferredFamily(family, preference);
}
