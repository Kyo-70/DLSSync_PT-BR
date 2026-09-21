# Choose and reset NVIDIA profile settings

DLSSync changes NVIDIA driver-profile settings through NVAPI DRS. It does not add DLSS to games, and a saved profile does not prove an effect inside a game.

## Choose the scope

- **Per-game:** open the centered game dialog and its DLSS Overrides section. The application profile is resolved from the game executable, with NVIDIA basename matching as a fallback.
- **Global:** open **Drivers > DLSS Overrides**. These changes affect the NVIDIA Base profile.
- Per-game settings can inherit global values. Reset removes local overrides; an inherited value can remain active.

## Apply and verify

1. Read the current values from the driver.
2. Change the intended settings. Unchanged settings are not rewritten.
3. Apply. The app saves the requested changes and opens a new DRS session to read them back.
4. A success message requires matching readback. A rejected write, missing privilege or readback mismatch remains visible.
5. Restart the game to test its behavior. Check the game's protection policy before changing its profile.

The installed driver reports its interface version, driver version and available setting IDs. This profile-access evidence is separate from the conservative game/runtime capability assessment. Unknown game support is not changed to compatible just to enable profile editing. Neural Rendering remains read-only in the preset registry.

SR, RR and FG retain separate preset namespaces. Latest and FG Default are distinct numeric values. These values are not a quality ranking. A GPU or game can ignore a stored setting that it does not support.

**Reset to default** restores the known, exposed settings in the selected scope, then checks that local overrides are absent. It does not restore DLL files or every NVIDIA setting. Profile updates requiring administrator privileges report that requirement; they do not bypass Windows security.

## Implementation and evidence

The loader resolves `nvapi64.dll` from System32. See [NVAPI DRS](../crates/nvapi-drs/src/ffi.rs), [commands](../src-tauri/src/commands/dlss_profile.rs) and [wire contracts](../crates/dlssync-contracts/src/dlss_profile.rs).

The isolated validation example creates an unbound profile, saves SR/RR/FG values, reads them through a new session, deletes only that profile and verifies its absence. This proves profile persistence on the tested driver, not in-game effects.

Primary API definitions: [NVIDIA NVAPI](https://github.com/NVIDIA/nvapi/blob/main/nvapi.h) and [driver setting IDs](https://github.com/NVIDIA/nvapi/blob/main/NvApiDriverSettings.h).
