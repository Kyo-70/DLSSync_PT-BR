import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { DetectedGame } from "@/lib/api";

vi.mock("@/lib/api", async (original) => ({
  ...await original<typeof import("@/lib/api")>(),
  scanLibraries: vi.fn(async () => [{
    id: "steam-1", name: "Fixture", launcher: "steam", app_id: "1", install_dir: "C:/fixture",
    art: { state: "pending", retryable: true }, image_url: null,
  }] as DetectedGame[]),
  detectDlls: vi.fn(async () => []),
  detectDlssEnabler: vi.fn(async () => false),
  detectAnticheat: vi.fn(async () => ({ detected: [] })),
  listBackups: vi.fn(async () => []),
  fetchSteamArt: vi.fn(async () => ({ state: "unavailable", retryable: false })),
}));

const { fetchSteamArt } = await import("@/lib/api");
const { scanGames, settings } = await import("@/lib/stores");

beforeEach(() => {
  vi.useFakeTimers();
  vi.mocked(fetchSteamArt).mockClear();
  settings.set(null);
});
afterEach(() => vi.useRealTimers());

it("treats an unprompted scan as automatic even when toasts are visible", async () => {
  await scanGames();
  expect(fetchSteamArt).toHaveBeenCalledWith("1", "automatic");
  await vi.runAllTimersAsync();
});

it("retains explicit user consent independently of silent notifications", async () => {
  await scanGames({ silent: true, trigger: "user_scan" });
  expect(fetchSteamArt).toHaveBeenCalledWith("1", "user_scan");
  await vi.runAllTimersAsync();
});
