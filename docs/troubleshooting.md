# Find diagnostics and report a problem

Keep the exact error and affected file/device when an operation fails. Logs and the operation journal help explain what happened; they do not replace missing backup files or prove that a game works.

## Find the active logs

Use **About → Logs**, the **Open logs folder** action or the command palette. Installed mode uses `%USERPROFILE%\DLSSync\Logs\`; portable mode uses `data\Logs\` beside the executable when `portable.flag` is present. Daily files use a name such as `dlssync.log.yyyy-mm-dd`.

Log lines include timestamps, levels, modules and messages. Read the failed operation's stage and per-file result alongside **Journal**. Under the active data root, `Cache\operations.db` contains durable journal data: do not delete the whole Cache folder as a generic fix.

## Share only reviewed diagnostics

**About → Report a problem** and the failed apply surface's **Report issue** action build a prefilled GitHub issue URL. The report can include app version, OS/build, timestamp, recent log lines and operation context. Logs are encoded into the report text, not attached as a file.

**Opening that URL sends its encoded contents to GitHub before you submit an issue.** The app does not automatically submit the issue, but editing the form afterward does not undo the initial transmission. If you need to redact first, avoid the prefilled action: inspect/copy the local diagnostics, remove private content, then open a [blank issue](https://github.com/xt0n1-t3ch/DLSSync/issues/new) and paste only the reviewed text.

Remove personal paths, account names, API keys and device identifiers you do not intend to share. Keep the component family, versions, error class and relevant stage when possible. Exporting/copying a journal entry is not the same as uploading it; review the output before sharing.

## Choose the next action

- **Missing game:** check the install folder and rescan. A recognized launcher does not guarantee every installation is discovered.
- **Failed update:** use the [error-message reference](error-messages.md) before retrying or changing settings.
- **Failed restore:** preserve snapshots and journal data; follow [backup recovery](restoring-backups.md).
- **Reveal opens only a folder:** the snapshot may be missing. A stored row does not recreate its bytes.
- **Driver version unchanged:** distinguish the installer process result from observed active version; see [drivers](drivers.md).

For a diagnostic launch, `RUST_LOG` can increase detail. In Git Bash, set it for the intended executable invocation, for example `RUST_LOG=dlssync=debug,dll_catalog=debug ./DLSSync.exe` from its actual folder. More detailed logs may contain more local information; inspect them before sharing.

## Evidence

Source review: 2026-09-13. Data roots are defined in [paths](../src-tauri/src/paths.rs). Reporting is implemented in [issue reporting](../src-tauri/src/commands/diagnostics.rs), with entrypoints in [About](../frontend/src/views/About.svelte) and [apply progress](../frontend/src/components/ApplyProgressModal.svelte). The exact contents of a report should be inspected before transmission; no issue was opened or submitted for this review.
