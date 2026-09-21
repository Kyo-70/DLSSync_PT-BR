import type { GameSnapshot } from "./api";

/** A PE version is not a package identity. Only a valid SHA-256 match establishes this label. */
export function sameSha256(a: string | null | undefined, b: string | null | undefined): boolean {
  return !!a && !!b && /^[a-f0-9]{64}$/i.test(a) && /^[a-f0-9]{64}$/i.test(b)
    && a.toLowerCase() === b.toLowerCase();
}

export function installedGameNames(games: GameSnapshot[], family: string, sha256: string): string[] {
  return [...new Set(games.filter(game => game.components?.some(component =>
    component.identity.family === family && component.observed_hash?.algorithm === "sha256"
      && sameSha256(component.observed_hash.digest, sha256),
  )).map(game => game.name))];
}

/** Observed file versions are shown separately, without claiming a matching catalog package. */
export function observedFamilyVersions(games: GameSnapshot[], family: string): { version: string; games: string[] }[] {
  const versions = new Map<string, Set<string>>();
  for (const game of games) {
    for (const component of game.components ?? []) {
      if (component.identity.family !== family || !component.observed_version) continue;
      const names = versions.get(component.observed_version) ?? new Set<string>();
      names.add(game.name);
      versions.set(component.observed_version, names);
    }
  }
  return [...versions].map(([version, names]) => ({ version, games: [...names] }));
}
