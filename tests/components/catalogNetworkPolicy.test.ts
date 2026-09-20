import { render } from "@testing-library/svelte";
import { tick } from "svelte";
import { describe, expect, it, vi } from "vitest";
import Catalog from "@/views/Catalog.svelte";
import { loadDriverUpdates } from "@/lib/stores";

vi.mock("@/lib/distribution", () => ({ isNexusBuild: true }));
vi.mock("@/lib/stores", async (original) => ({
  ...await original<typeof import("@/lib/stores")>(),
  bootstrapCatalog: vi.fn(async () => undefined),
  loadDriverUpdates: vi.fn(async () => undefined),
}));

describe("Nexus catalog rendering", () => {
  it("does not make a vendor driver request merely by opening Catalog", async () => {
    render(Catalog);
    await tick();
    expect(loadDriverUpdates).not.toHaveBeenCalled();
  });
});
