# Understand anti-cheat and game-protection warnings

Changing game DLLs or NVIDIA profiles can conflict with a game's rules or integrity checks. Check the title's policy before modifying it. DLSSync's detection is evidence about files and datasets, not permission to modify a protected game or a guarantee against kicks, bans or launch failures.

## Read the detected kind

| Kind | Meaning in the app |
|---|---|
| `anti_cheat` | Evidence of an anti-cheat system; modifications may carry account-policy consequences. |
| `anti_tamper` | Evidence of integrity/protection software; changed files may prevent launch. This is not automatically an account-ban mechanism. |
| `drm` | Informational DRM detection, distinct from anti-cheat. |

This guide does not claim confirmed ban incidents without a concrete source. An absent warning is not proof that the game has no protection or permits a modification.

## How detection works

The detector combines local filename evidence, executable inspection and bundled/catalog dataset matches. The sources are distinguishable in the report:

- **Binary scan:** bounded matching of known protection-related filenames in the game directory.
- **PE inspection:** executable section/string fingerprints and heuristics for known protectors. A heuristic result is not a vendor certification.
- **Dataset:** bundled observations, plus signed catalog entries when available, matched by identifiers such as Steam app ID or normalized game name.

The scanner can miss renamed, differently located or unrecognized protection. Dataset records can be incomplete or stale. A matched name or leftover file can also need interpretation; inspect the reported source instead of treating detection as conclusive runtime proof.

## Manual and background actions differ

Game details surface protection warnings for DLL updates and per-game DLSS overrides. Read those warnings and the title's policy before proceeding manually. Do not disable anti-cheat to force a file change.

Background auto-apply excludes protected games under its filtering rules. Manual actions still pass integrity, compatibility, running-game, path and execution checks and can be rejected. “Warns” does not mean “never blocks,” and “no warning” does not mean “safe.” Nexus's separate manual-only network rule also applies; see [channel verification](nexus-build.md).

## Evidence

Source review: 2026-09-13. See [protection detector](../crates/anticheat-detect/src/lib.rs), [PE inspection](../crates/anticheat-detect/src/pe_inspect.rs), [bundled dataset](../crates/dll-catalog/anticheat-snapshot.json), [background filtering](../frontend/src/lib/backgroundScan.ts) and [game details](../frontend/src/components/GameDetailDrawer.svelte). No live anti-cheat test or external incident investigation was performed. Read [optional mods](optional-mods.md) and [DLSS profile overrides](dlss-overrides.md) for related limits.
