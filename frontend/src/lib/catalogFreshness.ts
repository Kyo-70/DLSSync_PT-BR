/** Catalog freshness presentation.
 *
 *  The age of catalog content is computed from the authoritative ISO timestamp published by Rust,
 *  never from a formatted display string. The tone thresholds are presentation only; they do not
 *  decide trust, applicability or whether a refresh succeeded. */

export const FRESHNESS_SUCCESS_MAX_MINUTES = 180;
export const FRESHNESS_STALE_MINUTES = 26 * 60;

export type FreshnessTone = "success" | "neutral" | "warning";

export interface FreshnessAge {
  minutes: number;
  tone: FreshnessTone;
}

/** Minutes elapsed and its tone, or `null` when no usable timestamp exists.
 *  A missing or unparsable timestamp is an unknown age, never "fresh". */
export function freshnessAge(generatedAt: string | null | undefined, now: number): FreshnessAge | null {
  if (generatedAt === null || generatedAt === undefined || generatedAt === "") return null;
  // Accept both a full ISO timestamp and the legacy "YYYY-MM-DD HH:MM" display form.
  const candidate = generatedAt.includes("T")
    ? generatedAt
    : `${generatedAt.replace(" ", "T")}${generatedAt.length === 16 ? ":00Z" : "Z"}`;
  const parsed = new Date(candidate).getTime();
  if (Number.isNaN(parsed)) return null;
  const minutes = Math.floor((now - parsed) / 60_000);
  if (minutes < 0) return null;
  const tone: FreshnessTone =
    minutes < FRESHNESS_SUCCESS_MAX_MINUTES
      ? "success"
      : minutes < FRESHNESS_STALE_MINUTES
        ? "neutral"
        : "warning";
  return { minutes, tone };
}
