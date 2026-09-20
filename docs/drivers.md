# Update GPU and Windows device drivers

Use **Drivers** to inspect detected hardware, check available packages and start an installation. GPU installers and Windows Update Agent use different paths. Neither an installer exit code nor a recovery attempt proves the active driver was changed or can be restored.

## Update a GPU driver

1. Open **Drivers** and check the detected NVIDIA, AMD or Intel adapter and installed version. On a multi-GPU machine, choose the intended card.
2. Check for updates and read the package version, date and release notes. If the result is **Unknown**, do not assume the current driver is latest.
3. Start the install action if a direct package is available. If only a vendor-page action is available, open it and select the appropriate package there.
4. The app downloads the GPU installer and checks its Authenticode publisher against the expected vendor before launch. Respond to Windows administrator approval only after checking the requested operation and publisher.
5. Follow any vendor UI, read the outcome and reboot when requested. Recheck the active driver version afterward.

NVIDIA and Intel launches have vendor-specific arguments in the current implementation. The AMD source does not synthesize an installer URL. It records an observed official AMD page, so the correct AMD action is to open that page and select the package there. This page action is not proof that a package matches the device or that installation succeeded.

## Understand package matching

The app reads GPU and driver observations and normalizes versions for comparison. NVIDIA's Windows driver-store version can be decoded into its marketing version; Intel uses its catalog version; AMD needs a mapping between Windows and Adrenalin package versions. Different number formats do not by themselves prove a downgrade.

GPU lookup sources include NVIDIA's GeForce lookup service and product mapping, Intel's catalog, and an observed official AMD page. The compatibility parameters help select a result; they do not form a proven compatibility matrix for every OEM or legacy branch.

For a laptop, OEM-customized package, unsupported device or ambiguous branch, inspect the vendor/OEM guidance before installing. Exact coverage of every OEM, legacy and enterprise branch is **not yet documented**. A newer generic version is not proof that it is the right package for your device.

## Read installation outcomes correctly

The GPU installer state machine interprets these process exit codes:

| Code | App interpretation | Next action |
|---|---|---|
| `0` | Process completed | Recheck the observed driver version. |
| `3010` | Process completed; reboot required | Reboot, then check the active version. |
| `1602` | Cancelled | Retry only if you still want the installation. |
| `1223` | UAC declined/cancelled | Nothing about this code proves installation; approve only an intended retry. |
| Other | Failed | Preserve the code and vendor output; use [diagnostics](troubleshooting.md). |

These are GPU installer process results, not Windows Update Agent result codes or proof of active-version equality. The [runtime contract](runtime-contracts.md) requires separate installed-driver observations; it does not certify that every legacy path already performs complete readback.

## Update Windows device drivers

The system/device section uses Windows Update Agent for applicable updates, separate from the GPU catalog. Inspect the detected device and candidate, then start the intended action. The system-driver helper requests elevation because Windows installation/export operations can require it. Downgrade guards can refuse unsuitable candidates.

The local inventory joins present PnP device instances with installed signed-driver records. Exact hardware and compatible IDs link Windows Update offers to their installed device, current version and INF package. Missing versions and device problems stay explicit. The inventory is local and can be inspected without an update query.

Before installation, the backend queries the current offers again and resolves the target from Windows data. Renderer-supplied device context is not authoritative. Windows Update downloads and installs the selected update; per-update result codes and HRESULTs are checked. A successful package operation is followed by a fresh device inventory. The outcome distinguishes a verified active version, a required restart and an unverified version.

When a third-party INF can be exported, the elevated helper saves it before installation and records a SHA-256 file manifest. If that requested export fails, installation stops. A valid export is retained even when installation fails. A System Restore checkpoint is attempted separately and is not guaranteed.

Recovery verifies the snapshot's files before invoking the system PnPUtil executable. Windows driver ranking still applies: `/install` does not force a lower-ranked package. A backup is marked restored only when the exact device reports the saved version. A process exit or pending reboot is not verified rollback. See [backup recovery limits](restoring-backups.md#driver-recovery-is-separate).

This workflow covers drivers offered by Windows Update and installed packages available in the DriverStore. It is not a proprietary OEM package database and does not claim universal device or firmware coverage.

## Network and privacy

Driver resolution can send hardware/OS compatibility parameters to vendor services. The Standard frontend can check drivers when Drivers opens. Catalog browsing no longer starts a driver check. Local device inventory does not contact an update service.

Nexus requires every network action to be explicit and manual. Complete trigger gating and packaged Nexus network silence remain unverified; see [Nexus verification gaps](nexus-build.md). Opening release notes or a driver page also contacts the destination through your browser.

## Evidence

Source review: 2026-09-19. See [GPU resolution](../crates/driver-catalog/src/lib.rs), [vendor sources](../crates/driver-catalog/src/sources/mod.rs), [AMD source](../crates/driver-catalog/src/sources/amd.rs), [installer verification](../crates/driver-install/src/verify.rs), [launch arguments](../crates/driver-install/src/launch.rs), [exit handling](../crates/driver-install/src/state.rs), [Windows Update Agent](../crates/system-drivers/src/wua.rs), [snapshot support](../crates/system-drivers/src/snapshot.rs) and [elevated helper](../src-tauri/src/lib.rs). The real read-only inventory and update query were exercised. No real driver installation, reboot or recovery was run for this change.
