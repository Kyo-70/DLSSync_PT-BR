# Choose and verify the Nexus build

The Nexus channel keeps the DLL update and backup workflows but removes the app self-updater and automatic catalog refresh. Download it from [Nexus Mods, mod 1922](https://www.nexusmods.com/site/mods/1922). This page separates the required v1.7.0 channel contract from implementation and packaged-build evidence.

## Required channel rules

The Nexus build must satisfy all of these rules:

- No app self-updater, app-release polling, updater plugin, updater endpoint or updater permission.
- No automatic app-update or catalog network calls at startup, on Catalog open, on focus or on background ticks.
- **Refresh Catalog** is an explicit manual action. It is the only action allowed to fetch current catalog metadata and its detached signature.
- Every other network action must also be explicit and manual. Opening a view is not consent to a driver lookup, artwork search or download. Background DLL downloads are not permitted by this rule.
- App upgrades are installed manually from the Nexus page. Source and catalog links are transparency links, not permission to run an updater.
- Standard and Nexus must be built from the **same source commit and the same `v1.7.0` tag**. Nexus asset names must start with **`NexusBuild-`** and remain distinct from Standard assets.
- `latest.json` belongs only to the Standard updater and must reference the exact Standard installer and its matching signature. It must never select a Nexus package or rely on an ambiguous installer glob.

The former `2.0.0` / `NexusMods-Only` separate-tag lane is obsolete guidance, not an alternative v1.7.0 release procedure. Do not create a second channel tag to implement this contract.

## Catalog and local data

Nexus includes an embedded signed catalog for its initial offline state. Explicit refresh retrieves current metadata and a detached Ed25519 signature. The catalog path validates signature, schema and freshness/anti-downgrade constraints before accepting refreshed data; a failed refresh is not permission to use untrusted metadata.

DLL downloads remain separate actions. They use the selected catalog entry's algorithm and expected digest, which can be SHA-256 or historical MD5. Publisher verification is a separate Authenticode check. Sources can include labeled community archives as well as first-party releases. Neither a signed catalog nor a matching digest proves game compatibility.

Portable packaging is an independent mode. With `portable.flag` beside the executable, state resolves under its `data` directory and app self-update is disabled. Combining portable mode with Nexus must preserve the Nexus manual-only network rule. See [data locations](../README.md#where-your-data-lives).

## Source review and unresolved gaps

Reviewed 2026-09-19. Source checks and local packaging are separate from a release-package network capture. No publication was performed.

| Area | Source evidence | Verification status |
|---|---|---|
| App/catalog policy | [product.toml](../product.toml), [application policy](../crates/dlssync-application/src/lib.rs) | Nexus disables app updates and automatic catalog refresh; manual catalog trigger is represented. |
| Derived Nexus configuration | [Nexus preparation](../scripts/build-nexus.mjs), [strip verification](../scripts/verify-nexus-build.mjs) | Preparation writes derived files under `target/channels/nexus/config` and does not overwrite tracked configuration. The preparation and strip checks were run. The Rust build also derives its capability input in OUT_DIR, because Tauri validates discovered files before selecting inline capabilities. |
| Driver lookups | [Catalog](../frontend/src/views/Catalog.svelte), [Settings](../frontend/src/views/Settings.svelte), [stores](../frontend/src/lib/stores.ts), [driver command](../src-tauri/src/commands/drivers.rs) | Catalog no longer starts a driver lookup. Settings and Drivers gate automatic lookups with `isNexusBuild`. Local device inventory is read-only and does not query Windows Update. Exact packaged network behavior still requires capture. |
| Artwork | [art enrichment](../frontend/src/lib/stores.ts), [SteamGridDB requests](../src-tauri/src/commands/scan.rs) | Scan enrichment can call a network service. Full Nexus action gating and absence of incidental requests are not verified. |
| Release preservation | [release safety](../scripts/release-safety.mjs), [publisher](../scripts/publish-release.mjs) | The publication path refuses to delete an existing release. No publication was run in this task. |
| Standard updater feed | [release workflow](../.github/workflows/release.yml), [release safety](../scripts/release-safety.mjs) | Source validation requires `latest.json` to name only the exact Standard installer and contain a nonempty matching signature. No public feed was downloaded in this task. |
| Published files | No downloaded artifacts inspected | Signing state, available formats, asset names and public `latest.json` bytes remain unverified. |

Do not describe an unverified development build as fully manual-network compliant. The required rules above are release acceptance criteria, not a claim that these residual paths have already been fixed.

## Maintainer verification

Coordinate with the build owner before running commands. These entrypoints exist; listing them is not evidence that they passed:

```powershell
pnpm run check:nexus
pnpm run build:nexus
cargo xtask verify-release --channel nexus
```

Preparation produces `target/channels/nexus/config/tauri.conf.json` and `target/channels/nexus/config/default.capability.json`. Inspect the generated config, capabilities and packaged frontend for updater imports, endpoints and permissions. Preparation writes only derived files and does not overwrite tracked source configuration. `src-tauri/build.rs` derives the updater-free capability for Tauri ACL validation when the Nexus feature is selected. The build script invokes the pinned Tauri CLI directly and passes `--no-default-features` after the Cargo argument separator. Its overlay sets `plugins.updater` to JSON null so merge-patch removes the Standard endpoint. Omitting that key would inherit it. The Nexus Vite build replaces the updater package with a no-op module, so updater IPC is not included in its frontend. The Nexus channel also uses separate build outputs, so it does not share packaging state with the Standard channel.

Before declaring compliance, test the exact Nexus package with a network recorder. Cover startup, Catalog, Settings, Drivers, About, focus changes, tray/background ticks and scans without manual network consent. Then verify that each permitted button contacts only the service needed for that action. Separately verify both channel artifacts against the one release tag and commit, and check that Standard's `latest.json` names only the exact Standard installer. These checks remain unverified here.
