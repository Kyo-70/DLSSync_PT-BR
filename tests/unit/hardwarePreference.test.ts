import { describe, expect, it } from "vitest";
import { hardwarePreferenceFor, preferredFamily, defaultUpdateFamily } from "@/lib/hardwarePreference";
import type { GpuInfo, GpuVendor } from "@/generated/bindings";

function hardware(...vendors: GpuVendor[]) {
  return hardwarePreferenceFor({ gpus: vendors.map(vendor => ({ vendor }) as GpuInfo) });
}

describe("hardware preference, separate from compatibility", () => {
  it("prioritizes NVIDIA and shared Windows libraries without declaring AMD or Intel incompatible", () => {
    const preference = hardware("nvidia");
    expect(preferredFamily("dlss_sr", preference)).toBe(true);
    expect(preferredFamily("sl_dlss_fg", preference)).toBe(true);
    expect(preferredFamily("direct_storage", preference)).toBe(true);
    expect(preferredFamily("fsr_upscaler", preference)).toBe(false);
    expect(preferredFamily("xess_sr", preference)).toBe(false);
  });
  it("includes both integrated Intel and discrete NVIDIA adapters", () => {
    const preference = hardware("intel", "nvidia", "nvidia");
    expect([...preference.vendors]).toEqual(["intel", "nvidia"]);
    expect(preferredFamily("xess_fg", preference)).toBe(true);
    expect(preferredFamily("dlss_fg", preference)).toBe(true);
    expect(preferredFamily("fsr_fg", preference)).toBe(false);
  });
  it("handles AMD-only and AMD/NVIDIA hybrid systems", () => {
    expect(preferredFamily("fsr_loader", hardware("amd"))).toBe(true);
    expect(preferredFamily("dlss_sr", hardware("amd"))).toBe(false);
    expect(preferredFamily("fsr_fg", hardware("amd", "nvidia"))).toBe(true);
  });
  it("does not invent a hardware exclusion when inventory is absent or unidentified", () => {
    for (const preference of [hardwarePreferenceFor(null), hardware(), hardware("other")]) {
      expect(preference.known).toBe(false);
      expect(preferredFamily("dlss_sr", preference)).toBe(true);
      expect(preferredFamily("fsr_upscaler", preference)).toBe(true);
      expect(defaultUpdateFamily("dlss_sr", preference)).toBe(false);
      expect(defaultUpdateFamily("direct_storage", preference)).toBe(false);
    }
  });
});
