import { describe, it, expect } from "vitest";
import { mergeFamilyReleases } from "@/lib/catalogReleases";
import type { Release } from "@/lib/api";

function rel(version: string, packed: number, sha: string, filename = "x.dll"): Release {
  return {
    version,
    version_packed: packed,
    filename,
    sha256: sha,
    size_bytes: 0,
    signed: true,
    released_at: "2025-01-01T00:00:00Z",
    source: "test",
    cdn_url: "https://example.test/x",
    release_notes: null,
    signature_subject: null,
    channel: "stable",
    is_dev: false,
    min_driver: null,
  } as Release;
}

describe("mergeFamilyReleases", () => {
  it("merges families, dedupes by version+sha, and sorts newest-first", () => {
    const dx12 = [rel("1.0.2", 10002, "a", "amd_fidelityfx_dx12.dll"), rel("1.0.0", 10000, "c")];
    const vk = [rel("1.0.1", 10001, "b", "amd_fidelityfx_vk.dll"), rel("1.0.0", 10000, "c")];
    const merged = mergeFamilyReleases([dx12, vk]);
    expect(merged.map((r) => r.version)).toEqual(["1.0.2", "1.0.1", "1.0.0"]);
  });

  it("keeps same-version entries with different hashes (distinct files)", () => {
    const merged = mergeFamilyReleases([
      [rel("1.0.0", 10000, "a", "dx12.dll")],
      [rel("1.0.0", 10000, "b", "vk.dll")],
    ]);
    expect(merged.length).toBe(2);
  });

  it("handles empty input", () => {
    expect(mergeFamilyReleases([])).toEqual([]);
    expect(mergeFamilyReleases([[], []])).toEqual([]);
  });

  it("keeps DLSS patch ordering when JSON rounded the packed u64", () => {
    const rounded = Number(310n << 48n);
    const merged = mergeFamilyReleases([[rel("310.9.1.0", rounded, "a"), rel("310.9.1.1", rounded, "b")]]);
    expect(merged.map(release => release.version)).toEqual(["310.9.1.1", "310.9.1.0"]);
  });
});
