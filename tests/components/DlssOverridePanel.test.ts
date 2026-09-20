import { describe, it, expect, vi } from "vitest";
import { render } from "@testing-library/svelte";
import DlssOverridePanel from "@/components/DlssOverridePanel.svelte";
import { emptyDlssConfig } from "@/lib/dlss";
import * as api from "@/lib/api";

const globalScope = { scope: "global" } as const;

describe("DlssOverridePanel", () => {
  it("renders all three feature groups and the profile persistence and in-game effect boundary", () => {
    const { getByText, container } = render(DlssOverridePanel, {
      props: { scope: globalScope, driverPacked: 61047 },
    });
    expect(getByText("Super Resolution")).toBeTruthy();
    expect(getByText("Frame Generation")).toBeTruthy();
    const text = (container.textContent ?? "").replace(/\s+/g, " ");
    expect(text).toContain("Reset removes these local overrides");
    expect(text).toContain("depend on the game and GPU");
  });

  it("renders the Ray Reconstruction group (issue #32)", () => {
    const { container } = render(DlssOverridePanel, {
      props: { scope: globalScope, driverPacked: 61047 },
    });
    const text = (container.textContent ?? "").replace(/\s+/g, " ");
    expect(text).toContain("Ray Reconstruction");
    expect(text).toContain("0x10E41DF7");
  });

  it("renders custom dropdowns + checkboxes (no native controls)", () => {
    const { container } = render(DlssOverridePanel, {
      props: { scope: globalScope, driverPacked: 61047 },
    });
    const text = container.textContent ?? "";
    expect(text).toContain("No preset override");
    expect(text).toContain("No mode override");
    expect(container.querySelectorAll(".sel-trigger").length).toBeGreaterThanOrEqual(3);
    expect(container.querySelectorAll('[role="checkbox"]').length).toBe(3);
    expect(container.querySelector("select")).toBeNull();
    expect(container.querySelector('input[type="checkbox"]')).toBeNull();
  });

  it("does not infer compatibility from a local driver threshold", () => {
    const { container } = render(DlssOverridePanel, {
      props: { scope: globalScope, driverPacked: 57000 },
    });
    expect(container.textContent).not.toContain("572.16 or newer");
    expect(container.querySelectorAll("button[disabled]").length).toBeGreaterThan(0);
  });

  it("hydrates the form from read_dlss_override_config and labels the source (forum #1)", async () => {
    const readback: api.DlssOverrideReadback = {
      config: { ...emptyDlssConfig(), enable_sr_dll_override: true, sr_preset: "k" },
      source: "global",
      active_count: 1,
      observations: [],
      observation_complete: false,
    };
    const spy = vi.spyOn(api, "readDlssOverrideConfig").mockResolvedValue(readback);
    const { findByText } = render(DlssOverridePanel, {
      props: { scope: globalScope, driverPacked: 61047 },
    });
    expect(await findByText(/Preset K/)).toBeTruthy();
    expect(await findByText("Set in NVIDIA driver")).toBeTruthy();
    spy.mockRestore();
  });

  it("hydrates the RR preset from read_dlss_override_config (issue #32)", async () => {
    const readback: api.DlssOverrideReadback = {
      config: { ...emptyDlssConfig(), enable_rr_dll_override: true, rr_preset: "k" },
      source: "global",
      active_count: 2,
      observations: [],
      observation_complete: false,
    };
    const spy = vi.spyOn(api, "readDlssOverrideConfig").mockResolvedValue(readback);
    const { findAllByText } = render(DlssOverridePanel, {
      props: { scope: globalScope, driverPacked: 61047 },
    });
    const hits = await findAllByText(/Preset K/);
    expect(hits.length).toBeGreaterThanOrEqual(1);
    spy.mockRestore();
  });
});
