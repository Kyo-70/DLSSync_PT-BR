# DLSSync documentation


- [Local mods](local-mods.md): local-only recipe actions, durable recovery and explicit unsupported cases.

Start with [installation and one-click game updates](../README.md). The repository version is 1.7.0 and its [changelog section](../CHANGELOG.md#170---2026-09-20) records the released scope. Source-based guides describe implementation and limits, not a certified release or game compatibility matrix.

## Complete a user task

| Guide | Use it to |
|---|---|
| [Identify a game's DLSS, FSR and XeSS DLLs](dll-families.md) | Match recognized files to SR, FG, RR, loaders, API variants and package families. |
| [Update Streamline as a matched set](streamline.md) | Understand dependencies, version schemes and why related members move together. |
| [Understand versions and compatibility states](versions-and-compatibility.md) | Distinguish file/package versions, pins, unknown/experimental results and integrity from compatibility. |
| [Update GPU and Windows device drivers](drivers.md) | Inspect local device identities, select a package, and read active-version and recovery evidence. |
| [Restore a backup after a game update](restoring-backups.md) | Restore retained snapshots and investigate incomplete recovery. |
| [Choose and reset DLSS presets](dlss-overrides.md) | Use global/per-game profiles, installed-driver access checks and verified profile readback. |
| [Update game DLLs when optional mods are installed](optional-mods.md) | Interpret mod markers without assuming tested coexistence. |
| [Resolve update and restore error messages](error-messages.md) | Identify error classes, execution messages and separate driver codes. |
| [Find diagnostics and report a problem](troubleshooting.md) | Locate logs and redact data before opening a prefilled issue URL. |
| [Understand anti-cheat and game-protection warnings](anticheat.md) | Distinguish anti-cheat, anti-tamper and DRM evidence. |
| [Verify a download before responding to Windows warnings](signing-reality.md) | Separate Windows signing, updater signatures and vendor DLL signatures. |
| [Choose and verify the Nexus build](nexus-build.md) | Understand manual-only channel rules and remaining network/artifact verification gaps. |
| [Compare documented updater capabilities](competitive-comparison.md) | Read dated, per-cell source evidence and explicitly unverified competitor claims. |

## Architecture and contributor references

| Reference | Scope |
|---|---|
| [DLSSync 1.7 architecture](architecture-1.7.md) | Shared application/contracts ownership, CLI and distribution boundaries. |
| [Runtime contracts](runtime-contracts.md) | Wire types, authoritative observations and migration target rules; not a completion certificate. |
| [Catalog formats and source verification](catalog-format.md) | Artifact identity, dependency metadata, digest algorithms and publisher-generation design. External publisher deployment claims need separate verification. |
| [Translations](translations.md) | Existing locale/key/metadata contributor reference. Check current locale registration and package scripts against source before adding a language. |
| [CDP validation](cdp-validation.md) | Existing WebView2 debugging reference. Use checked-in tools and the active task's browser/tool permissions; historical local conveniences are not repository prerequisites. |

Standing ownership and gate instructions live in [AGENTS.md](../AGENTS.md), with [CLAUDE.md](../CLAUDE.md) pointing to the same policy. Also see [CONTRIBUTING.md](../CONTRIBUTING.md), [test entrypoints](../tests/index.md), [product policy](../product.toml) and [release history](../CHANGELOG.md). Listed commands and tests are not claims of a current passing run.

## Release-copy references

- [Write accurate release descriptions](release-marketing.md): task-oriented wording, evidence requirements and existing asset locations.
- [Nexus v1.7 BBCode draft](nexus-description-v1.7.bbcode): local draft, not approved for publication while channel/artifact gaps remain.

Historical `docs/handoffs/` material is separate from current instructions and was not reviewed or modified in this documentation task. The short machine-readable entrypoint is [llms.txt](../llms.txt).
