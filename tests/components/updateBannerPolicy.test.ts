import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { render, cleanup } from "@testing-library/svelte";
import { tick } from "svelte";
import { updaterMock } from "../setup";

const runtimeMode = vi.fn();
vi.mock("@/generated/bindings", async (original) => ({
  ...await original<typeof import("@/generated/bindings")>(),
  invokeCommand: (command: string) => command === "runtime_mode" ? runtimeMode() : Promise.resolve([]),
}));

const Banner = (await import("@/components/UpdateBanner.svelte")).default;
async function settle() {
  for (let i = 0; i < 8; i++) {
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}
beforeEach(() => updaterMock.reset());
afterEach(() => cleanup());

it("does not check for updates when runtime mode is unknown", async () => {
  runtimeMode.mockRejectedValue(new Error("IPC unavailable"));
  render(Banner);
  await settle();
  expect(updaterMock.check).not.toHaveBeenCalled();
});

it("does not check for updates in portable mode", async () => {
  runtimeMode.mockResolvedValue({ portable: true, release_url: "https://example.test" });
  render(Banner);
  await settle();
  expect(updaterMock.check).not.toHaveBeenCalled();
});

it("checks for updates after installed mode is observed", async () => {
  runtimeMode.mockResolvedValue({ portable: false, release_url: "https://example.test" });
  render(Banner);
  await settle();
  expect(updaterMock.check).toHaveBeenCalledTimes(1);
});
