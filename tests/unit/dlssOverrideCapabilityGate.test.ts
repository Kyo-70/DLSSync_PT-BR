import { describe, it, expect } from "vitest";
import type {
  CapabilityAssessment,
  DlssCapabilityReport,
  DlssCapabilitySnapshot,
  DlssPresetRegistry,
} from "@/lib/api";
import {
  FG_PRESET_DEFAULT_RAW,
  PRESET_LATEST_RAW,
  dlssWriteGate,
  featurePresetDescription,
  presetEntries,
  presetEntry,
  presetSettingIds,
  rawValueHex,
  rrLocalDescription,
} from "@/lib/dlss";

const REGISTRY = {
  sr: {
    setting_ids: { override_id: 0x10e41df3, preset_id: 0x10e41df4 },
    options: [
      { preset: "k", raw_value: 0x0000000b, description: "SR description for K" },
      { preset: "latest", raw_value: PRESET_LATEST_RAW, description: "SR latest" },
    ],
  },
  rr: {
    setting_ids: { override_id: 0x10e41df7, preset_id: 0x10e41df8 },
    options: [
      { preset: "k", raw_value: 0x0000000b, description: "RR description for K" },
      { preset: "latest", raw_value: PRESET_LATEST_RAW, description: "RR latest" },
    ],
  },
  fg: {
    setting_ids: { override_id: 0x10e41df5, preset_id: 0x10e41df6 },
    options: [
      { preset: "default", raw_value: FG_PRESET_DEFAULT_RAW, description: "FG default" },
      { preset: "latest", raw_value: PRESET_LATEST_RAW, description: "FG latest" },
    ],
  },
  nr: {
    setting_ids: { override_id: 0x10e41df9, preset_id: 0x10e41dfa },
    options: [{ preset: "latest", raw_value: PRESET_LATEST_RAW, description: "NR latest" }],
  },
} as unknown as DlssPresetRegistry;

function adapter(assessment: CapabilityAssessment, model = "NVIDIA adapter"): DlssCapabilityReport {
  return {
    adapter_model: model,
    pci_vendor_id: 0x10de,
    pci_device_id: 0x2704,
    evidence: {
      host: { platform: "windows" },
      nvapi: { status: "version_unknown", detail: "no verified runtime version" },
      adapter: { class: "unknown", provider: { provider: "nvidia", driver_version: null }, hardware_recognized: false },
      provider_applicability: "unknown",
      game_integration: "unknown",
      runtime_stack: "unknown",
      preset_runtime_mapping: "unknown",
    },
    assessment,
  } as unknown as DlssCapabilityReport;
}

function snapshot(adapters: DlssCapabilityReport[]): DlssCapabilitySnapshot {
  return { presets: REGISTRY, adapters } as DlssCapabilitySnapshot;
}

describe("write_eligible is the only gate that authorises a DRS write", () => {
  it("denies the write when no capability answer was obtained", () => {
    expect(dlssWriteGate(null)).toEqual({ eligible: false, capabilityObserved: false, reasons: [] });
    expect(dlssWriteGate(undefined)).toEqual({
      eligible: false,
      capabilityObserved: false,
      reasons: [],
    });
  });

  it("denies the write when the backend reported no adapter", () => {
    expect(dlssWriteGate(snapshot([]))).toMatchObject({ eligible: false, capabilityObserved: false });
  });

  it("does not promote a documented provider namespace into a write authorisation", () => {
    const gate = dlssWriteGate(
      snapshot([
        adapter({
          provider_documented_namespace: true,
          write_eligible: false,
          block_reasons: ["nvapi_version_unknown", "adapter_class_unknown"],
        }),
      ]),
    );
    expect(gate.eligible).toBe(false);
    expect(gate).toMatchObject({
      capabilityObserved: true,
      reasons: ["nvapi_version_unknown", "adapter_class_unknown"],
    });
  });

  it("merges the reasons of every ineligible adapter without repeating one", () => {
    const gate = dlssWriteGate(
      snapshot([
        adapter({
          provider_documented_namespace: true,
          write_eligible: false,
          block_reasons: ["nvapi_version_unknown"],
        }),
        adapter(
          {
            provider_documented_namespace: false,
            write_eligible: false,
            block_reasons: ["nvapi_version_unknown", "adapter_not_nvidia"],
          },
          "Integrated adapter",
        ),
      ]),
    );
    expect(gate).toMatchObject({
      eligible: false,
      reasons: ["nvapi_version_unknown", "adapter_not_nvidia"],
    });
  });

  it("authorises the write only when an adapter assessment says so", () => {
    const gate = dlssWriteGate(
      snapshot([
        adapter({ provider_documented_namespace: true, write_eligible: true, block_reasons: [] }),
      ]),
    );
    expect(gate).toEqual({ eligible: true, adapterModel: "NVIDIA adapter" });
  });
});

describe("the four preset registries stay separated by function", () => {
  it("projects each feature from its own registry branch", () => {
    expect(presetEntries(REGISTRY, "sr").map((e) => e.id)).toEqual(["k", "latest"]);
    expect(presetEntries(REGISTRY, "rr").map((e) => e.id)).toEqual(["k", "latest"]);
    expect(presetEntries(REGISTRY, "fg").map((e) => e.id)).toEqual(["default", "latest"]);
    expect(presetEntries(REGISTRY, "nr").map((e) => e.id)).toEqual(["latest"]);
    expect(presetEntries(null, "sr")).toEqual([]);
  });

  it("keeps Frame Generation `default` distinct from `latest`", () => {
    const fgDefault = presetEntry(REGISTRY, "fg", "default");
    const fgLatest = presetEntry(REGISTRY, "fg", "latest");
    expect(fgDefault?.rawValue).toBe(FG_PRESET_DEFAULT_RAW);
    expect(fgLatest?.rawValue).toBe(PRESET_LATEST_RAW);
    expect(fgDefault?.rawValue).not.toBe(fgLatest?.rawValue);
    expect(rawValueHex(FG_PRESET_DEFAULT_RAW)).toBe("0x00FFFFFE");
    expect(rawValueHex(PRESET_LATEST_RAW)).toBe("0x00FFFFFF");
  });

  it("does not publish a Frame Generation `default` in the other namespaces", () => {
    expect(presetEntry(REGISTRY, "sr", "default")).toBeNull();
    expect(presetEntry(REGISTRY, "rr", "default")).toBeNull();
  });

  it("never shares a description between Super Resolution and Ray Reconstruction", () => {
    const sr = featurePresetDescription(REGISTRY, "sr", "k");
    const rr = featurePresetDescription(REGISTRY, "rr", "k");
    expect(sr).toBe("SR description for K");
    expect(rr).toBe("RR description for K");
    expect(sr).not.toBe(rr);
  });

  it("returns no description rather than borrowing another feature's text", () => {
    expect(featurePresetDescription(REGISTRY, "rr", "recommended")).toBeNull();
    expect(featurePresetDescription(null, "sr", "k")).toBeNull();
  });

  it("falls back only to Ray-Reconstruction-specific local copy", () => {
    expect(rrLocalDescription("recommended")).toContain("Ray Reconstruction");
    expect(rrLocalDescription("l")).toBeNull();
    expect(rrLocalDescription(null)).toBeNull();
  });

  it("exposes each namespace's own setting ids", () => {
    expect(presetSettingIds(REGISTRY, "sr")).toEqual({ overrideId: 0x10e41df3, presetId: 0x10e41df4 });
    expect(presetSettingIds(REGISTRY, "rr")).toEqual({ overrideId: 0x10e41df7, presetId: 0x10e41df8 });
    expect(presetSettingIds(null, "fg")).toBeNull();
  });
});
