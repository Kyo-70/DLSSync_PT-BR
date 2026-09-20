import { describe, it, expect } from "vitest";
import type { DlssPreset } from "@/lib/api";
import {
  SR_PRESET_OPTIONS,
  RR_PRESET_OPTIONS,
  FG_MODE_OPTIONS,
  FG_COUNT_OPTIONS,
  emptyDlssConfig,
  presetLabel,
  dynamicMfgAvailable,
  dlss5Available,
  hasActiveOverride,
  DYNAMIC_MFG_MIN_DRIVER_PACKED,
  DLSS5_MIN_DRIVER_PACKED,
} from "@/lib/dlss";

describe("dlss override option tables", () => {
  it("exposes super-resolution, frame-gen mode and count options", () => {
    expect(SR_PRESET_OPTIONS.some((o) => o.value === "recommended")).toBe(true);
    expect(SR_PRESET_OPTIONS.some((o) => o.value === "k")).toBe(true);
    expect(FG_MODE_OPTIONS.map((o) => o.value)).toEqual(["app_controlled", "fixed", "dynamic"]);
    expect(FG_COUNT_OPTIONS.map((o) => o.value)).toEqual(["app_controlled", "x2", "x3", "x4"]);
  });

  it("labels the full A-O preset range so any externally-set preset is shown", () => {
    expect(presetLabel("k")).toContain("Preset K");
    expect(presetLabel("recommended")).toContain("Recommended");
    expect(presetLabel("a")).toContain("Preset A");
    expect(presetLabel("m")).toContain("Preset M");
    expect(presetLabel("o")).toContain("Preset O");
    for (const letter of ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o"] as DlssPreset[]) {
      expect(SR_PRESET_OPTIONS.some((o) => o.value === letter)).toBe(true);
    }
  });

  it("falls back to the upper-cased value for an unrecognized preset", () => {
    expect(presetLabel("z" as DlssPreset)).toBe("Z");
  });
});

describe("driver-version gating", () => {
  it("requires 595.97 for dynamic multi frame generation", () => {
    expect(DYNAMIC_MFG_MIN_DRIVER_PACKED).toBe(59597);
    expect(dynamicMfgAvailable(DYNAMIC_MFG_MIN_DRIVER_PACKED)).toBe(true);
    expect(dynamicMfgAvailable(DYNAMIC_MFG_MIN_DRIVER_PACKED - 1)).toBe(false);
    expect(dynamicMfgAvailable(57216)).toBe(false);
  });
});

describe("every option is self-explanatory with a source link", () => {
  it.each([
    ["SR presets", SR_PRESET_OPTIONS],
    ["FG modes", FG_MODE_OPTIONS],
    ["FG counts", FG_COUNT_OPTIONS],
  ])("%s carry a label, description and https source URL", (_name, options) => {
    for (const option of options) {
      expect(option.label.length).toBeGreaterThan(0);
      expect(option.description.length).toBeGreaterThan(12);
      expect(option.sourceUrl).toMatch(/^https:\/\//);
    }
  });
});

describe("config helpers", () => {
  it("empty config has no active override", () => {
    expect(hasActiveOverride(emptyDlssConfig())).toBe(false);
  });

  it("any set field marks the config active", () => {
    expect(hasActiveOverride({ ...emptyDlssConfig(), enable_sr_dll_override: true })).toBe(true);
    expect(hasActiveOverride({ ...emptyDlssConfig(), fg_mode: "dynamic" })).toBe(true);
    expect(hasActiveOverride({ ...emptyDlssConfig(), fg_dynamic_target_fps: 240 })).toBe(true);
  });

  it("RR fields mark the config active (issue #32)", () => {
    expect(hasActiveOverride({ ...emptyDlssConfig(), enable_rr_dll_override: true })).toBe(true);
    expect(hasActiveOverride({ ...emptyDlssConfig(), rr_preset: "k" })).toBe(true);
    expect(hasActiveOverride({ ...emptyDlssConfig(), rr_preset: "recommended" })).toBe(true);
  });
});

describe("ray reconstruction options (issue #32)", () => {
  it("exposes an RR preset table with recommended + K + full A-O range", () => {
    expect(RR_PRESET_OPTIONS.some((o) => o.value === "recommended")).toBe(true);
    expect(RR_PRESET_OPTIONS.some((o) => o.value === "default")).toBe(true);
    expect(RR_PRESET_OPTIONS.some((o) => o.value === "k")).toBe(true);
    for (const letter of ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o"] as DlssPreset[]) {
      expect(RR_PRESET_OPTIONS.some((o) => o.value === letter)).toBe(true);
    }
  });

  it("RR options carry labels, descriptions and https source URLs", () => {
    for (const option of RR_PRESET_OPTIONS) {
      expect(option.label.length).toBeGreaterThan(0);
      expect(option.description.length).toBeGreaterThan(12);
      expect(option.sourceUrl).toMatch(/^https:\/\//);
    }
  });
});

describe("DLSS5 gating", () => {
  it("requires 610.47 for DLSS5 features", () => {
    expect(DLSS5_MIN_DRIVER_PACKED).toBe(61047);
    expect(dlss5Available(DLSS5_MIN_DRIVER_PACKED)).toBe(true);
    expect(dlss5Available(DLSS5_MIN_DRIVER_PACKED - 1)).toBe(false);
  });
});
