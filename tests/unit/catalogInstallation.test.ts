import { describe, expect, it } from "vitest";
import { sameSha256, installedGameNames, observedFamilyVersions } from "../../frontend/src/lib/catalogInstallation";
import type { GameSnapshot } from "../../frontend/src/lib/api";

describe("catalog installation identity", () => {
  it("does not treat missing hashes or legacy MD5 as an installed release", () => {
    expect(sameSha256(null, null)).toBe(false);
    expect(sameSha256("a".repeat(32), "a".repeat(32))).toBe(false);
    expect(sameSha256("A".repeat(64), "a".repeat(64))).toBe(true);
  });
  it("requires the family and SHA-256 algorithm and includes all matching games", () => {
    const games = ["One", "Two", "Wrong family", "MD5"].map((name, index) => ({
      name, id: name, install_dir: name,
      components: [{ identity: { family: index === 2 ? "fsr_upscaler" : "dlss_sr" },
        observed_hash: { algorithm: index === 3 ? "md5" : "sha256", digest: "a".repeat(64) } }],
    })) as GameSnapshot[];
    expect(installedGameNames(games, "dlss_sr", "a".repeat(64))).toEqual(["One", "Two"]);
  });
  it("keeps observed file versions available without inferring a package match", () => {
    const games = [{ id: "one", name: "One", install_dir: "One", components: [
      { identity: { family: "dlss_fg" }, observed_version: "310.9.0.0" },
      { identity: { family: "dlss_fg" }, observed_version: "310.9.1.0" },
      { identity: { family: "sl_dlss_fg" }, observed_version: "2.14.1" },
    ] }] as GameSnapshot[];
    expect(observedFamilyVersions(games, "dlss_fg")).toEqual([
      { version: "310.9.0.0", games: ["One"] }, { version: "310.9.1.0", games: ["One"] },
    ]);
    expect(installedGameNames(games, "dlss_fg", "a".repeat(64))).toEqual([]);
  });
});
