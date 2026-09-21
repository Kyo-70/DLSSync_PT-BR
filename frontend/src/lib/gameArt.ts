/** Cover art presentation.
 *
 *  Rust resolves art per launcher and reports its state, the exact variant it verified and the
 *  dimensions it observed. This module only turns a resolved asset into something the WebView can
 *  render, and decides which games still need a resolution attempt. It never builds a store URL and
 *  never treats a failed source as "this game has no cover". */

import { convertFileSrc } from "@tauri-apps/api/core";
import type { DetectedGame, GameArt, GameArtAsset } from "./api";

export type ArtOrientation = "landscape" | "portrait";

/** Renderable source for a verified asset. A local cache file needs the Tauri asset transport;
 *  an HTTPS locator is used as published. */
export function artSrc(asset: GameArtAsset | null | undefined): string | null {
  if (!asset) return null;
  if (asset.locator_kind === "local_file") {
    try {
      return convertFileSrc(asset.locator);
    } catch {
      return null;
    }
  }
  return asset.locator;
}

/** The asset to draw for one orientation, falling back to the other orientation and finally to the
 *  legacy projection so an older snapshot still shows its cover. */
export function bestArtSrc(
  game: Pick<DetectedGame, "art" | "image_url">,
  orientation: ArtOrientation = "landscape",
): string | null {
  const art = game.art;
  if (art) {
    const preferred = orientation === "portrait" ? art.portrait : art.landscape;
    const secondary = orientation === "portrait" ? art.landscape : art.portrait;
    return artSrc(preferred) ?? artSrc(secondary) ?? game.image_url ?? null;
  }
  return game.image_url ?? null;
}

/** Observed dimensions of the drawn asset, for callers that report resolution. */
export function artDimensions(
  game: Pick<DetectedGame, "art">,
  orientation: ArtOrientation = "landscape",
): { width: number; height: number } | null {
  const art = game.art;
  if (!art) return null;
  const asset = (orientation === "portrait" ? art.portrait : art.landscape) ?? art.landscape ?? art.portrait;
  return asset ? { width: asset.width, height: asset.height } : null;
}

/** True when another resolution attempt is justified.
 *
 *  `unavailable` is source-specific: a configured fallback may not have been consulted yet.
 *  `source_failed` is retried only when Rust marked it retryable, and a missing
 *  `art` field comes from an older snapshot that was never resolved. */
export function needsArtResolution(game: Pick<DetectedGame, "art" | "image_url">, hasConfiguredFallback = false): boolean {
  const art: GameArt | undefined = game.art;
  if (!art) return !game.image_url;
  if (art.state === "resolved") return false;
  if (art.state === "unavailable") {
    return hasConfiguredFallback && !art.error_code?.startsWith("steamgriddb:");
  }
  if (art.state === "source_failed") return art.retryable;
  return true;
}
