import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, cleanup } from "@testing-library/svelte";
import Catalog from "@/views/Catalog.svelte";
import DrawerFeatureList from "@/components/DrawerFeatureList.svelte";
import VersionPickerPopover from "@/components/VersionPickerPopover.svelte";
import { catalogVendors } from "@/lib/stores";

vi.mock("@/lib/stores", async (original) => ({
  ...await original<typeof import("@/lib/stores")>(),
  bootstrapCatalog: vi.fn(), loadDriverUpdates: vi.fn(),
}));
vi.mock("@/lib/api", async (original) => ({
  ...await original<typeof import("@/lib/api")>(),
  getCatalogStatus: vi.fn(async () => null), listReleases: vi.fn(async () => []),
}));
beforeEach(() => { cleanup(); catalogVendors.set([]); });

describe("artifact version presentation", () => {
  it("catalog keeps NGX and Streamline latest versions in separate artifact rows", () => {
    catalogVendors.set([{ vendor: "nvidia", label: "NVIDIA", families: [
      { family: "dlss_fg", label: "DLSS Frame Generation", latest: "310.3.0", releaseCount: 1 },
      { family: "sl_dlss_fg", label: "DLSS Frame Generation (Streamline plug-in)", latest: "2.11.1", releaseCount: 2 },
    ] }]);
    const { container } = render(Catalog);
    const rows = [...container.querySelectorAll(".catalog-family")];
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain("310.3.0");
    expect(rows[0].textContent).not.toContain("2.11.1");
    expect(rows[1].textContent).toContain("Streamline");
    expect(rows[1].textContent).toContain("2.11.1");
    expect(rows[1].textContent).not.toContain("310.3.0");
  });

  it("picker names the Streamline artifact rather than only its shared feature", () => {
    // The picker renders through the `portal` action, so its nodes live at `document.body` instead
    // of inside the render container. That is the fix for pointer interception by a transformed
    // ancestor, so the query follows the node to its real mount point.
    render(VersionPickerPopover, { family: "sl_dlss_fg", filename: "sl.dlss_g.dll",
      currentVersion: "2.11.1", latestVersion: "2.11.1", pickedVersion: null, onPick: vi.fn(), onClose: vi.fn() });
    expect(document.body.querySelector(".picker-head h2")?.textContent).toContain("Streamline");
  });

  it.each([false, true])("renders no equal-version update arrow with files expanded=%s and names the summary artifact", (expanded) => {
    const record = { family: "dlss_fg" as const, path: "C:/Game/nvngx_dlssg.dll", current_version: "310.3.0", sha256: null, file_description: null };
    const noop = vi.fn();
    const { container } = render(DrawerFeatureList, {
      hasRecords: true, recordCount: 1, outdatedCount: 0, selectedCount: 0,
      featureBuckets: [{ feature: "dlss_fg", records: [record], primary: record, title: "DLSS Frame Generation", blurb: "", iconId: "frame_gen", accent: "green", anyOutdated: false, anyAhead: false, allUpToDate: true, allDisabled: false, statusLabel: "Current", statusTone: "success" }],
      advancedRows: [{ family: "dlss_fg", label: "DLSS FG", records: [record], primary: record, anyOutdated: false }],
      selected: {}, disabledFamilies: [], pinnedVersions: {}, expandedFeatures: { dlss_fg: expanded }, advancedExpanded: true,
      pickerOpenFor: null, dlssExpanded: false, dlssExe: null, dlssExeResolving: false, dlssDriverPacked: 0,
      rowKey: (r) => r.path, relation: () => "same", targetFor: () => "310.3.0", latestFor: () => "310.3.0", featureSelectionState: () => "none",
      onSelectAllOutdated: noop, onClearSelection: noop, onToggleFeatureSelection: noop, onToggleFileSelection: noop,
      onToggleFeatureDisabled: noop, onSetPin: noop, onSetPickerOpen: noop, onToggleFeatureExpanded: noop,
      onToggleAdvanced: noop, onToggleDlss: noop, onRowContextMenu: noop, onRowMenuAnchor: noop,
    });
    expect(container.querySelectorAll("svg.arrow")).toHaveLength(0);
    expect(container.querySelector(".feature-versions")?.getAttribute("title")).toContain("nvngx_dlssg.dll");
  });
});
