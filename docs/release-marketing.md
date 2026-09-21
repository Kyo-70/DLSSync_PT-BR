# Write accurate release descriptions

Release copy must help readers choose a build and complete a task without overstating safety or compatibility. This document is local guidance; publication still requires the release owner.

## Use a supported description

> DLSSync is an open-source Windows app for updating supported DLSS, FSR, XeSS, Streamline and DirectStorage DLLs already present in game folders. It also offers GPU and Windows device-driver workflows. Signed catalog metadata, per-entry hashes, publisher checks, local DLL backups and an operation journal help you inspect changes and recover retained snapshots.

Use “DLSS updater,” “FSR updater,” “XeSS updater” and “GPU driver updater” naturally where the task calls for them. Do not add “Also useful if you searched for” lists, hidden keyword blocks or search-ranking promises.

## Keep the factual boundaries

- **Release state:** link the actual changelog heading. Do not imply that a tag or downloadable artifact exists from version metadata alone; name the published assets you checked.
- **Integrity:** separate catalog signatures, artifact digests, Authenticode and backup SHA-256. Historical artifact rows can use MD5.
- **Sources:** catalog sources include first-party releases and labeled DLSS Swapper community-archive history. Do not claim vendor-direct-only distribution or unsupported succession/affiliation.
- **Recovery:** one-click restore starts an action for a retained snapshot. Missing/corrupt files, locks, permissions and validation failures can prevent recovery. GPU installers are not universally reversible.
- **Signing:** optional Windows signing is not evidence of a signed artifact. Tauri payload signatures do not establish Authenticode signing of the installer.
- **Compatibility:** a correct hash, publisher or architecture does not prove a game or hardware combination works. Avoid universal game/launcher detection claims.
- **UX:** describe routine game updates as one click, not a mandatory confirmation or review workflow. Follow [UI wording rules](../AGENTS.md#ux-wording-and-behavior-rules).
- **Privacy:** explain functional network actions and optional bearer-authenticated artwork lookup. Prefilled issue URLs transmit their contents when opened.
- **Comparison:** use only the [dated evidence matrix](competitive-comparison.md). “Not verified” is not “No.” Do not regenerate unsupported cells from the old registry.
- **Performance:** no current desktop footprint measurements are verified. Omit installer-size, RAM, startup and CPU promises until reproducible measurements exist.

## Describe the Nexus channel precisely

NexusBuild- has no self-updater and must make no automatic app-update or catalog calls. Catalog refresh and every other network action must be explicit and manual. Standard and Nexus must use the same commit and one `v1.7.0` tag, with distinct `NexusBuild-` assets. Standard's `latest.json` must reference only the exact Standard installer and contain its nonempty matching signature. Publication must refuse to delete an existing release.

The [channel guide](nexus-build.md) records residual source and artifact-verification gaps. Do not publish blanket compliance language until the release owner has resolved and tested them. Keep [the Nexus description](nexus-description-v1.7.bbcode) as a draft until then.

## Reuse existing assets

| Surface | Repository asset |
|---|---|
| GitHub hero | `.github/assets/nexus/banner-2560x720.png` |
| Nexus header | `.github/assets/nexus/banner-header-1300x372.png` |
| Social preview | `.github/assets/preview-card-clean.png` |
| Nexus preview card | `.github/assets/nexus/preview-card-clean-600x338.png` |
| Feature gallery | `.github/assets/nexus/gallery/` |

Review embedded text and screenshots separately before publication; a corrected README does not validate older image copy. This task does not update assets or remote descriptions.
