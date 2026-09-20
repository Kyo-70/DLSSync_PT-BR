import { describe, expect, it } from "vitest";
import {
  freshnessAge,
  FRESHNESS_STALE_MINUTES,
  FRESHNESS_SUCCESS_MAX_MINUTES,
} from "@/lib/catalogFreshness";

const NOW = Date.parse("2026-09-17T12:00:00Z");

describe("catalog freshness", () => {
  it("reads a full ISO timestamp published by Rust", () => {
    expect(freshnessAge("2026-09-17T11:30:00Z", NOW)).toEqual({ minutes: 30, tone: "success" });
  });

  it("still reads the legacy display form so the fallback keeps working", () => {
    expect(freshnessAge("2026-09-17 11:00", NOW)).toEqual({ minutes: 60, tone: "success" });
  });

  it("treats a missing or unparsable timestamp as unknown, never as fresh", () => {
    expect(freshnessAge(null, NOW)).toBeNull();
    expect(freshnessAge(undefined, NOW)).toBeNull();
    expect(freshnessAge("", NOW)).toBeNull();
    expect(freshnessAge("not a date", NOW)).toBeNull();
  });

  it("treats a timestamp in the future as unknown instead of negative age", () => {
    expect(freshnessAge("2026-09-17T12:30:00Z", NOW)).toBeNull();
  });

  it("separates the three tones at their exact boundaries", () => {
    const atSuccessEdge = NOW - (FRESHNESS_SUCCESS_MAX_MINUTES - 1) * 60_000;
    const atNeutralStart = NOW - FRESHNESS_SUCCESS_MAX_MINUTES * 60_000;
    const atWarningStart = NOW - FRESHNESS_STALE_MINUTES * 60_000;
    expect(freshnessAge(new Date(atSuccessEdge).toISOString(), NOW)?.tone).toBe("success");
    expect(freshnessAge(new Date(atNeutralStart).toISOString(), NOW)?.tone).toBe("neutral");
    expect(freshnessAge(new Date(atWarningStart).toISOString(), NOW)?.tone).toBe("warning");
  });
});
