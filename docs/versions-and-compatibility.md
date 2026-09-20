# Understand versions and compatibility states

Use the installed file, selected artifact and compatibility result together when deciding whether to update. A correct hash proves that bytes match an expected digest; it does **not** prove that a game works, that a GPU supports the feature or that a mod combination is compatible.

## Separate the version numbers

| Version or identity | Meaning |
|---|---|
| DLSSync version | The app version. `1.7.0` is the current release line; it is not a DLL version. |
| Package version | The release/archive containing a group of artifacts. |
| Internal file version | Version metadata read from the Windows DLL. A loader and upscaler from one package can report different versions. |
| Catalog revision | The catalog observation used to choose an artifact, not proof that every upstream source was queried successfully today. |
| Installed GPU driver version | Windows' observed driver version, normalized for comparison where needed. It can differ from the vendor's marketing/package number. |

Do not compare DirectX and Vulkan artifacts, a loader and an upscaler, or Streamline SDK and driver-managed version schemes as if they were one release series.

## Interpret the current result

The frontend's version relation and the backend's compatibility decision answer different questions. Exact visible labels can vary by surface and locale.

| State or condition | What you can conclude | What to do |
|---|---|---|
| Update available / `outdated` relation | The selected catalog target is newer under the relation rules. | Check the matching family, settings and compatibility result before updating. |
| Same / current relation | The relation logic considers the installed component current or deliberately leaves it unchanged. | Do not infer that a game was tested. Streamline cross-scheme guards can leave a component unchanged. |
| Ahead of catalog | Installed version compares newer than the selected catalog target. | Keep it unless you deliberately need a supported older candidate; do not force a downgrade to clear a badge. |
| No target / unknown version | A usable candidate or observation is missing. | Rescan and inspect catalog availability; do not infer compatibility from the filename alone. |
| Unknown compatibility | The available evidence does not establish a supported match. | Treat the result as uncertainty, not approval. |
| Experimental | The candidate/decision is explicitly experimental. | Keep recovery data and expect that runtime behavior may differ. |
| Incompatible architecture | The binary architecture does not match the target. | Do not rename or force the file; report an incorrect catalog candidate. |
| Driver requirement not met | The candidate needs a newer driver under the app's rules. | Read the requirement and [driver guide](drivers.md); do not bypass the check. |

Architecture validation, publisher verification, source metadata and successful copy verification are useful evidence. None substitutes for running the actual game with its settings and hardware. No comprehensive tested game/hardware matrix is documented here.

## Select or pin a version

Open the game details and use the available version picker for the component. The catalog determines which historical or experimental candidates exist; a picker is not a list of every release ever published. Saved pins survive rescans through settings, but a pin does not override missing assets, set dependencies or execution validation.

Refresh the catalog explicitly if you need current metadata. On Nexus, only the manual refresh action may fetch it. If the catalog or game files change after preparation, the operation can be rejected as stale. Rescan and prepare again rather than replaying an old operation blindly.

## Read trust separately

Ed25519 verifies catalog metadata. Catalog artifact integrity uses the recorded algorithm, including MD5 for historical records and SHA-256 for other records. Authenticode publisher verification is separate. Local backup snapshots use SHA-256 regardless of a downloaded artifact's historical catalog algorithm.

An archive can contain an original vendor-signed file without being a first-party distribution source. Check the actual source and package identity rather than relying on a vendor logo.

## Evidence

Source review: 2026-09-13. See [relation logic](../frontend/src/lib/relation.ts), [version helpers](../frontend/src/lib/versions.ts), [runtime contract types](../crates/dlssync-contracts/src/runtime.rs), [execution checks](../crates/dlssync-application/src/execution.rs), [settings persistence](../src-tauri/src/commands/settings.rs) and [catalog format](catalog-format.md). Runtime contract documentation includes target boundaries; it is not proof that every legacy UI path already exposes every field.
