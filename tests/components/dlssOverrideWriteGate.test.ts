import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, fireEvent } from "@testing-library/svelte";
import DlssOverridePanel from "@/components/DlssOverridePanel.svelte";
import { emptyDlssConfig, FG_PRESET_DEFAULT_RAW, PRESET_LATEST_RAW } from "@/lib/dlss";
import * as api from "@/lib/api";

const globalScope = { scope: "global" } as const;

const REGISTRY = {
  sr: {
    setting_ids: { override_id: 0x10e41df3, preset_id: 0x10e41df4 },
    options: [
      { preset: "k", raw_value: 0x0000000b, description: "SR-ONLY-COPY for preset K" },
      { preset: "latest", raw_value: PRESET_LATEST_RAW, description: "SR latest model" },
    ],
  },
  rr: {
    setting_ids: { override_id: 0x10e41df7, preset_id: 0x10e41df8 },
    options: [
      { preset: "k", raw_value: 0x0000000b, description: "RR-ONLY-COPY for preset K" },
      { preset: "latest", raw_value: PRESET_LATEST_RAW, description: "RR latest model" },
    ],
  },
  fg: {
    setting_ids: { override_id: 0x10e41df5, preset_id: 0x10e41df6 },
    options: [
      { preset: "default", raw_value: FG_PRESET_DEFAULT_RAW, description: "FG default model" },
      { preset: "latest", raw_value: PRESET_LATEST_RAW, description: "FG latest model" },
    ],
  },
  nr: {
    setting_ids: { override_id: 0x10e41df9, preset_id: 0x10e41dfa },
    options: [{ preset: "latest", raw_value: PRESET_LATEST_RAW, description: "NR latest model" }],
  },
} as unknown as api.DlssPresetRegistry;

function snapshot(writeEligible: boolean, reasons: string[] = []): api.DlssCapabilitySnapshot {
  return {
    presets: REGISTRY,
    adapters: [
      {
        adapter_model: "NVIDIA adapter",
        pci_vendor_id: 0x10de,
        pci_device_id: 0x2704,
        evidence: {
          host: { platform: "windows" },
          nvapi: { status: "version_unknown", detail: "no verified runtime version" },
          adapter: {
            class: "unknown",
            provider: { provider: "nvidia", driver_version: null },
            hardware_recognized: false,
          },
          provider_applicability: "unknown",
          game_integration: "unknown",
          runtime_stack: "unknown",
          preset_runtime_mapping: "unknown",
        },
        assessment: {
          provider_documented_namespace: true,
          write_eligible: writeEligible,
          block_reasons: reasons,
        },
      },
    ],
  } as unknown as api.DlssCapabilitySnapshot;
}

let capabilitySpy: ReturnType<typeof vi.spyOn> | null = null;
let readbackSpy: ReturnType<typeof vi.spyOn> | null = null;

function mockCapabilities(value: api.DlssCapabilitySnapshot | null): void {
  capabilitySpy = vi.spyOn(api, "dlssCapabilities");
  if (value === null) {
    capabilitySpy.mockRejectedValue(new Error("no capability answer"));
  } else {
    capabilitySpy.mockResolvedValue(value);
  }
}

function mockReadback(config: api.DlssOverrideConfig): void {
  readbackSpy = vi.spyOn(api, "readDlssOverrideConfig");
  readbackSpy.mockResolvedValue({
    config,
    source: "global",
    active_count: 1,
    observations: [],
    observation_complete: false,
  } as unknown as api.DlssOverrideReadback);
}

beforeEach(() => {
  capabilitySpy = null;
  readbackSpy = null;
});

afterEach(() => {
  capabilitySpy?.mockRestore();
  readbackSpy?.mockRestore();
});

async function settle(): Promise<void> {
  for (let i = 0; i < 6; i += 1) await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe("phase 5 - no write is offered unless the backend authorises it", () => {
  it("disables every write control and names the blocking reasons", async () => {
    mockCapabilities(snapshot(false, ["nvapi_version_unknown", "adapter_class_unknown"]));
    const { container } = render(DlssOverridePanel, {
      props: { scope: globalScope, driverPacked: 61047 },
    });
    await settle();

    expect(container.querySelector('[data-testid="dlss-write-blocked"]')).not.toBeNull();
    const reasons = [...container.querySelectorAll('[data-testid="dlss-block-reason"]')].map((el) =>
      el.getAttribute("data-reason"),
    );
    expect(reasons).toEqual(["nvapi_version_unknown", "adapter_class_unknown"]);

    expect(container.querySelector('[data-testid="dlss-apply"]')?.hasAttribute("disabled")).toBe(true);
    expect(container.querySelector('[data-testid="dlss-reset"]')?.hasAttribute("disabled")).toBe(true);
    for (const trigger of container.querySelectorAll(".sel-trigger")) {
      expect(trigger.hasAttribute("disabled")).toBe(true);
    }
    for (const box of container.querySelectorAll('[role="checkbox"]')) {
      expect(box.hasAttribute("disabled")).toBe(true);
    }
  });

  it("never calls apply or reset while the write is not authorised", async () => {
    mockCapabilities(snapshot(false, ["nvapi_unavailable"]));
    const applySpy = vi.spyOn(api, "applyDlssOverride").mockResolvedValue(
      {} as unknown as api.DlssApplyOutcome,
    );
    const resetSpy = vi.spyOn(api, "resetDlssOverride").mockResolvedValue(undefined);
    const { container } = render(DlssOverridePanel, {
      props: { scope: globalScope, driverPacked: 61047 },
    });
    await settle();

    await fireEvent.click(container.querySelector('[data-testid="dlss-apply"]') as Element);
    await fireEvent.click(container.querySelector('[data-testid="dlss-reset"]') as Element);
    await settle();

    expect(applySpy).not.toHaveBeenCalled();
    expect(resetSpy).not.toHaveBeenCalled();
    applySpy.mockRestore();
    resetSpy.mockRestore();
  });

  it("fails closed when no capability answer is obtained at all", async () => {
    mockCapabilities(null);
    const { container } = render(DlssOverridePanel, {
      props: { scope: globalScope, driverPacked: 61047 },
    });
    await settle();
    expect(container.querySelector('[data-testid="dlss-write-blocked"]')).not.toBeNull();
    expect(container.querySelectorAll('[data-testid="dlss-block-reason"]').length).toBe(0);
    expect(container.querySelector('[data-testid="dlss-apply"]')?.hasAttribute("disabled")).toBe(true);
  });

  it("enables the controls when an adapter assessment reports write_eligible", async () => {
    mockCapabilities(snapshot(true));
    mockReadback(emptyDlssConfig());
    const { container } = render(DlssOverridePanel, {
      props: { scope: globalScope, driverPacked: 61047 },
    });
    await settle();
    expect(container.querySelector('[data-testid="dlss-write-blocked"]')).toBeNull();
    expect(container.querySelector('[data-testid="dlss-reset"]')?.hasAttribute("disabled")).toBe(false);
    expect(container.querySelector('[data-testid="dlss-apply"]')?.hasAttribute("disabled")).toBe(true);
  });
});

describe("phase 5 - the four preset sets are presented per function", () => {
  it("lists each registry separately, with FG default distinct from FG latest", async () => {
    mockCapabilities(snapshot(true));
    const { container } = render(DlssOverridePanel, {
      props: { scope: globalScope, driverPacked: 61047 },
    });
    await settle();
    await fireEvent.click(
      container.querySelector('[data-testid="dlss-preset-reference"] button') as Element,
    );
    await settle();

    for (const feature of ["sr", "rr", "fg", "nr"]) {
      expect(container.querySelector(`[data-testid="preset-registry-${feature}"]`)).not.toBeNull();
    }
    const fgDefault = container.querySelector('[data-testid="preset-fg-default"]');
    const fgLatest = container.querySelector('[data-testid="preset-fg-latest"]');
    expect(fgDefault?.getAttribute("data-raw")).toBe("0x00FFFFFE");
    expect(fgLatest?.getAttribute("data-raw")).toBe("0x00FFFFFF");
    expect(fgDefault?.getAttribute("data-raw")).not.toBe(fgLatest?.getAttribute("data-raw"));
    expect(container.querySelector('[data-testid="preset-sr-default"]')).toBeNull();
    expect(container.querySelector('[data-testid="preset-rr-default"]')).toBeNull();
  });

  it("shows Ray Reconstruction copy in the RR group and Super Resolution copy in the SR group", async () => {
    mockCapabilities(snapshot(true));
    mockReadback({ ...emptyDlssConfig(), sr_preset: "k", rr_preset: "k" });
    const { container } = render(DlssOverridePanel, {
      props: { scope: globalScope, driverPacked: 61047 },
    });
    await settle();

    const sr = container.querySelector('[data-testid="sr-preset-desc"]')?.textContent ?? "";
    const rr = container.querySelector('[data-testid="rr-preset-desc"]')?.textContent ?? "";
    expect(sr).toContain("SR-ONLY-COPY");
    expect(sr).not.toContain("RR-ONLY-COPY");
    expect(rr).toContain("RR-ONLY-COPY");
    expect(rr).not.toContain("SR-ONLY-COPY");
  });
});
