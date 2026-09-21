# Update Streamline as a matched set

Streamline's `sl.*.dll` files are related package members. Update them through DLSSync's set workflow rather than replacing a single plugin with an unrelated release. Matching file hashes does not make a mixed package compatible.

## Why members move together

The interposer, common library and feature plugins share runtime interfaces. A plugin from one package can expect interfaces absent from another package's interposer. DLSSync identifies `sl.*.dll` members and prepares a coherent update set; catalog dependency metadata distinguishes mandatory dependencies from other installed members.

Recognized members include `sl.interposer.dll`, `sl.common.dll`, `sl.pcl.dll`, `sl.nis.dll`, `sl.directsr.dll`, `sl.reflex.dll`, `sl.dlss.dll`, `sl.dlss_g.dll` and `sl.dlss_d.dll`. This does not mean every game needs every plugin. Do not install missing optional features by filling the folder with all available files.

The separate NVIDIA runtimes `nvngx_dlss.dll`, `nvngx_dlssg.dll` and `nvngx_dlssd.dll` are not Streamline plugin filenames. The [family map](dll-families.md) keeps those identities separate.

## Update the installed set

1. Close the game and open its details in **Library**.
2. Inspect the installed Streamline members and warnings. Check the Streamline preference if the feature is disabled.
3. Use the update/set action. Let the app determine the matching package members and validate the current files before replacement.
4. Read every result. If an operation reports a recovery failure, stop updating and open [Backups](restoring-backups.md).
5. Launch and test the game only after resolving incomplete operations.

Set staging and rollback are recovery mechanisms, not a promise that every filesystem failure can be undone. Keep the associated snapshots and journal until you have tested the game.

## Do not compare unrelated version schemes

The source explicitly distinguishes Streamline SDK `2.x` versions from driver/OTA-managed `310.x` versions. The latter is not an ordinary upgrade target for an SDK set. The frontend relation logic avoids treating a different-major Streamline candidate as a normal update; set execution also applies its own checks.

An apparently newer number, a same-version badge or a correct digest does not establish interchangeability. Do not force a cross-scheme replacement to make a badge disappear. Package identity matters even when individual DLL internal versions differ.

## Mod-managed sets

The scanner detects known markers for DLSS Enabler, OptiScaler and related mods. Detection is a warning about file ownership, not a tested compatibility result. Same-major candidates may still be displayed; that does not authorize replacing a mod's runtime set without checking the mod's instructions.

Current behavior spans frontend candidate filtering and backend set validation during migration. An exhaustive mod/version allowlist and a tested game matrix are **not yet documented**. See [optional mods](optional-mods.md).

## Evidence

Source review: 2026-09-13. Relevant owners are [scanner set recognition](../crates/dll-scanner/src/lib.rs), [candidate version relations](../frontend/src/lib/relation.ts), [set command](../src-tauri/src/commands/streamline_set.rs), [application execution](../crates/dlssync-application/src/execution.rs) and [catalog dependencies](../crates/dll-catalog/src/v3.rs). These sources support the set model, not universal successful launches or guaranteed rollback.
