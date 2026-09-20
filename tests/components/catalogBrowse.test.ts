import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { tick } from "svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import Catalog from "@/views/Catalog.svelte";
import { catalogVendors } from "@/lib/stores";
import { listReleases } from "@/lib/api";

vi.mock("@/lib/stores", async (original) => ({
  ...await original<typeof import("@/lib/stores")>(),
  bootstrapCatalog: vi.fn(async () => undefined),
  loadDriverUpdates: vi.fn(async () => undefined),
}));
vi.mock("@/lib/api", async (original) => ({
  ...await original<typeof import("@/lib/api")>(),
  listReleases: vi.fn(async () => []),
}));

beforeEach(() => {
  vi.clearAllMocks();
  catalogVendors.set([
    { vendor: "amd", label: "AMD", families: [
      { family: "fsr_loader", label: "FSR Loader", latest: "2.0.0", releaseCount: 1 },
      { family: "fsr_upscaler", label: "FSR Upscaler", latest: "2.0.0", releaseCount: 10 },
    ] },
    { vendor: "microsoft", label: "Microsoft", families: [
      { family: "direct_storage_core", label: "DirectStorage Core", latest: "1.3.0", releaseCount: 12 },
    ] },
  ]);
});

describe("Catalog browsing", () => {
  it("retains the vendor's libraries when searching its name", async () => {
    render(Catalog);
    await fireEvent.input(screen.getByRole("searchbox"), { target: { value: "AMD" } });
    expect(screen.getByRole("button", { name: "View FSR Loader versions" })).not.toBeNull();
    expect(screen.getByRole("button", { name: "View FSR Upscaler versions" })).not.toBeNull();
    expect(screen.queryByRole("button", { name: "View DirectStorage Core versions" })).toBeNull();
  });

  it("opens an exact formerly grouped family and requests its backend releases", async () => {
    render(Catalog);
    await fireEvent.input(screen.getByRole("searchbox"), { target: { value: "DirectStorage Core" } });
    await fireEvent.click(screen.getByRole("button", { name: "View DirectStorage Core versions" }));
    await tick();
    expect(listReleases).toHaveBeenCalledWith("microsoft", "direct_storage_core");
  });

  it("combines the vendor filter with search and offers a complete empty-state reset", async () => {
    render(Catalog);
    const filters = screen.getByRole("group", { name: "Vendors" });
    await fireEvent.click(within(filters).getByRole("button", { name: "AMD", exact: true }));
    await fireEvent.input(screen.getByRole("searchbox"), { target: { value: "DirectStorage" } });
    expect(screen.getByText("No libraries match these filters.")).not.toBeNull();
    await fireEvent.click(within(screen.getByRole("status")).getByRole("button", { name: "Clear search" }));
    await fireEvent.click(within(screen.getByRole("region", { name: "Microsoft", exact: true })).getByText("Support libraries (1)"));
    expect(screen.getByRole("button", { name: "View DirectStorage Core versions" })).not.toBeNull();
    expect(within(filters).getByRole("button", { name: "All vendors" }).getAttribute("aria-pressed")).toBe("true");
  });
});
