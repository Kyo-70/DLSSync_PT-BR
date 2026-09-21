import { describe, expect, it, vi } from "vitest";
import type { DetectedGame, GameArt, GameArtAsset } from "@/lib/api";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
  invoke: vi.fn(async () => undefined),
}));

const { artSrc, bestArtSrc, artDimensions, needsArtResolution } = await import("@/lib/gameArt");

function asset(over: Partial<GameArtAsset>): GameArtAsset {
  return {
    locator: "https://cdn.example.test/library_hero.jpg",
    locator_kind: "https_url",
    source: "steam_official_cdn",
    variant: "library_hero",
    width: 3840,
    height: 1240,
    verified: true,
    ...over,
  } as GameArtAsset;
}

function game(art: GameArt | undefined, imageUrl: string | null = null): DetectedGame {
  return {
    id: "g1",
    name: "How to Fish",
    launcher: "steam",
    install_dir: "C:/Games/How to Fish",
    app_id: "4001890",
    art,
    image_url: imageUrl,
    size_bytes: null,
  } as unknown as DetectedGame;
}

function art(over: Partial<GameArt>): GameArt {
  return {
    state: "resolved",
    landscape: null,
    portrait: null,
    error_code: null,
    retryable: false,
    cache_status: "hit",
    ...over,
  } as GameArt;
}

describe("cover art presentation", () => {
  it("serves an HTTPS locator as published", () => {
    expect(artSrc(asset({}))).toBe("https://cdn.example.test/library_hero.jpg");
  });

  it("routes a local cache file through the asset transport instead of a raw path", () => {
    const src = artSrc(asset({ locator: "C:/cache/4001890_hero.jpg", locator_kind: "local_file" }));
    expect(src).toContain("asset://localhost/");
    expect(src).not.toBe("C:/cache/4001890_hero.jpg");
  });

  it("prefers the requested orientation and falls back to the other one", () => {
    const portrait = asset({ variant: "library_600x900_2x", width: 1200, height: 1800, locator: "https://cdn/p.jpg" });
    const landscape = asset({});
    expect(bestArtSrc(game(art({ landscape, portrait })), "portrait")).toBe("https://cdn/p.jpg");
    expect(bestArtSrc(game(art({ landscape: null, portrait })), "landscape")).toBe("https://cdn/p.jpg");
  });

  it("reports the dimensions the backend observed for the drawn asset", () => {
    expect(artDimensions(game(art({ landscape: asset({}) })))).toEqual({ width: 3840, height: 1240 });
  });

  it("keeps showing an older snapshot that only has the legacy projection", () => {
    expect(bestArtSrc(game(undefined, "https://cdn/legacy.jpg"))).toBe("https://cdn/legacy.jpg");
  });

  it("does not retry a game whose sources reported no cover", () => {
    expect(needsArtResolution(game(art({ state: "unavailable" })))).toBe(false);
  });

  it("retries a recoverable source failure and never treats it as absence", () => {
    expect(needsArtResolution(game(art({ state: "source_failed", retryable: true })))).toBe(true);
    expect(needsArtResolution(game(art({ state: "source_failed", retryable: false })))).toBe(false);
  });

  it("resolves a game that was never attempted", () => {
    expect(needsArtResolution(game(art({ state: "pending" })))).toBe(true);
    expect(needsArtResolution(game(undefined, null))).toBe(true);
  });

  it("consults a configured fallback after local metadata has no art", () => {
    const missingLocal = game(art({ state: "unavailable", error_code: "epic_manifest:no_art" }));
    expect(needsArtResolution(missingLocal, true)).toBe(true);
    expect(needsArtResolution(missingLocal, false)).toBe(false);
    expect(needsArtResolution(game(art({ state: "unavailable", error_code: "steamgriddb:no_match" })), true)).toBe(false);
  });
});
