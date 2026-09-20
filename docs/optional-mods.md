# Update game DLLs when optional mods are installed

Treat a mod-managed game folder as a separate compatibility case. DLSSync can detect known mod markers, but that does not prove that DLSS Enabler, OptiScaler or another mod works with a selected runtime version.

## What detection means

The scanner looks for exact marker filenames in a bounded directory scan, including `dlss-enabler.dll`, `dlss-enabler-upscaler.dll`, `nvngx-wrapper.dll`, `nvngx.dll`, `optiscaler.dll`, `dlssg_to_fsr3_amd_is_better.dll`, `dlss-enabler.log` and `dlss-enabler.asi`.

A marker suggests that a mod may manage runtime files. It does not identify a fully tested mod release or verify its configuration. A leftover log can be a marker even when the active installation has changed. Conversely, a bounded scan can miss a renamed or differently located mod. Generic `dxgi.dll` and `version.dll` names are deliberately not treated as definitive markers.

`nvngx.dll` is not the same filename as the recognized NVIDIA SR runtime `nvngx_dlss.dll`. Do not replace a wrapper with a vendor runtime based on a similar name.

## Before updating a modded game

1. Close the game and identify the mod and its installed version from your own installation records.
2. Read that mod's instructions for which DLLs it owns and which runtime versions it expects. DLSSync does not provide a complete mod compatibility matrix.
3. Inspect the game details and warnings. Keep [Streamline package members](streamline.md) together; a same-major offer is not a guarantee of coexistence.
4. Preserve the mod's configuration and original files separately. DLSSync's DLL snapshots do not back up every mod file or setting.
5. Update only a candidate supported by the evidence available to you. Test the game and [restore the associated snapshots](restoring-backups.md) if it regresses.

If the backend disables or rejects a candidate, do not rename files, edit the backup database or bypass signature/architecture checks to force it. Candidate display and execution validation are different steps.

## What is not documented

This guide does not offer installation procedures for OptiScaler, DLSS Enabler, RenoDX, Luma or arbitrary injection mods. Repository evidence does not establish universal coexistence, cross-GPU feature support, or a tested list of mod/game/version combinations. Those claims are **not verified**.

Do not assume a mod or profile override is permitted in a protected multiplayer game. Detection of anti-cheat, anti-tamper and DRM describes different evidence; an absent detection does not authorize a modification. See [protection warnings](anticheat.md).

## Evidence

Source review: 2026-09-13. See [marker names and bounded scan](../crates/dll-scanner/src/lib.rs), [candidate relations](../frontend/src/lib/relation.ts) and [set validation adapter](../src-tauri/src/commands/streamline_set.rs). Source inspection does not establish the runtime compatibility of an external mod.
