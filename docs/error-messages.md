# Resolve update and restore error messages

Start with the failed stage, component and exact message, not only the notification title. DLSSync has frontend error classes, backend execution messages and separate driver result codes. They are not one interchangeable numeric error system.

## DLL update error classes

These identifiers come from `ApplyErrorClass` and the frontend classifier. Visible wording is localized and legacy message matching can map several causes to one class.

| Class | Stage / meaning | Safe next action |
|---|---|---|
| `network` | Download/request interrupted, timed out or rejected | Check connectivity and retry the intended operation. Repeated failures need the source URL and error in a report. |
| `signature` | Authenticode unreadable, absent, untrusted or unexpected publisher | Stop and inspect source/publisher evidence. Do not enable unsigned files as a routine workaround. |
| `hash` | Artifact digest does not match expected bytes | Refresh catalog explicitly and retry only with trusted metadata. Record the actual SHA-256 or MD5 algorithm. |
| `lock` | A file is open elsewhere | Close the game normally, wait for exit and retry. |
| `game_running` | Running-game guard refused the operation | Close the game before retrying. |
| `permission` | A required file operation was denied | Check the game/data-root permissions. Do not disable protection or bypass path guards. |
| `backup` | Snapshot/index unavailable or recovery needs attention | Check space and access. Preserve snapshots and journal; see [restore failures](restoring-backups.md#handle-a-failed-restore). |
| `missing` | Catalog release or expected archive member missing | Do not substitute another DLL. Report the family, selected version and missing filename. |
| `architecture` | Binary architecture incompatible with target | Stop; refresh/inspect the candidate and report incorrect catalog identity. |
| `streamline_locked` | Streamline set cannot be applied under current settings/constraints | Inspect the feature switch and complete set. Do not copy one plugin manually. |
| `driver_too_old` | Required driver condition not met | Read the candidate requirement and [driver guide](drivers.md). |
| `cancelled` | Operation cancelled | Check per-file outcomes and recovery state before deciding to retry. |
| `other` | Unclassified or missing error detail | Preserve the original message and inspect the journal/logs. Do not assume it is harmless. |

Some legacy UI hints suggest allowing unsigned files or describe every digest as SHA-256. Those hints are not the recovery advice in this guide: integrity uses the entry's actual algorithm, and bypassing publisher checks does not resolve a trust failure.

## Backend execution messages

These are exact message prefixes from the execution owner; text after the prefix supplies the operation-specific cause.

| Prefix | Meaning | Action |
|---|---|---|
| `update plan is stale:` | Prepared evidence no longer matches current state | Rescan and prepare a new update; do not replay the old plan. |
| `unsafe target path:` | Target rejected by containment/path rules | Check the original game location; do not edit paths to evade the guard. |
| `operation locked:` | Another operation owns the affected work | Wait for it to finish and inspect its result before retrying. |
| `operation journal failed:` | Durable operation recording failed | Preserve the data root, check space/access and report the failure. |
| `rolled_back:` | Update failed and the recovery path completed verification | Read the original cause. This is not a successful update. |
| `rollback_failed:` | Recovery was incomplete | Stop further mutation; inspect backups and recovery errors. |
| `filesystem operation failed:` | OS file operation failed | Preserve the detailed OS error; check locks, permissions and disk state. |

Restore can also reject a missing snapshot, an invalid SHA-256, a non-DLL target, a symlink or a protected system location. Fix the underlying cause rather than changing the database to bypass checks.

## GPU installer codes versus Windows Update codes

GPU installer process handling maps `0` to completion, `3010` to completion with reboot required, `1602` to cancellation and `1223` to declined UAC/cancellation. Other exits fail under the current mapping. These outcomes do not prove the desired active driver version; recheck after any required reboot.

Windows Update Agent uses its own result structures and HRESULTs. Do not interpret a WUA/HRESULT value using the GPU exit-code table or assume the same number has the same context. A complete safe-remediation catalog for every WUA, vendor-installer and HRESULT value is **not yet documented**. Preserve the exact code, stage, device and accompanying message for diagnosis.

## Report without exposing private data

Use [diagnostics](troubleshooting.md) to locate logs. Review/redact game paths, account names, API keys and device identifiers before sharing. A prefilled GitHub URL sends encoded report contents when it opens, even before you submit the issue; use a blank issue and paste reviewed text if needed.

## Evidence

Source review: 2026-09-13. See [error classes](../crates/dlssync-contracts/src/lib.rs), [frontend classification](../frontend/src/lib/applyErrorClass.ts), [execution errors](../crates/dlssync-application/src/execution.rs), [restore guards](../src-tauri/src/commands/backup.rs), [GPU exit mapping](../crates/driver-install/src/state.rs) and [WUA result handling](../crates/system-drivers/src/wua.rs). No failing operation was reproduced for this guide.
