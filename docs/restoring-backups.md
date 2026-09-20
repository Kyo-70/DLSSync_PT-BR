# Restore a backup after a game update

Use **Backups** to restore a retained DLL snapshot when an update causes a regression. Restoration uses local files and SHA-256 validation; it is not guaranteed to succeed or finish instantly.

## Restore the affected files

1. Close the game and wait for its processes to exit. Do not run another update at the same time.
2. Open **Backups** and locate the affected game, original file and snapshot timestamp. For a package/set update, identify all related members from the same operation.
3. Choose **Restore** for the intended snapshot. The one-click action starts restoration; it can still fail validation or encounter a filesystem error.
4. Read the result. For a grouped or bulk action, inspect each file rather than assuming every member succeeded.
5. Rescan, then test the game. If it still fails, retain the snapshots and inspect **Journal** and the logs before making further changes.

Restoring one member of a multi-file package may leave a mixed set. Recover the associated members rather than substituting files from unrelated backup dates. See [Streamline sets](streamline.md).

## Find the snapshot data

The installed-mode root is `%USERPROFILE%\DLSSync\`. Portable mode uses `data\` beside the executable when `portable.flag` is present. Under the active root:

- `Backups\` contains snapshot files and the `backups.db` index.
- `Cache\operations.db` contains the operation journal; it is not disposable download cache.
- `Logs\` contains diagnostic logs.

Use the app's snapshot-reveal or folder-opening action to find the real location. A database row can remain after a snapshot was removed outside the app; seeing a row does not prove the file exists. Do not delete the root, journal or database while investigating recovery.

## Handle a failed restore

| Failure | Safe next action |
|---|---|
| Snapshot missing | Check the active data root and any separately retained copy. DLSSync cannot reconstruct a deleted snapshot from its database entry. |
| SHA-256 mismatch | Stop using that snapshot. Preserve it for diagnosis and use a separately verified backup or the launcher's repair path. |
| Game/DLL locked | Close the game normally and retry after it exits. Do not disable anti-cheat to force the write. |
| Permission denied or disk error | Check access to both the snapshot and target directory, plus free space. Do not edit database paths to bypass guards. |
| Unsafe target, symlink or protected system path | Verify that the target is the original game DLL location. Report unexpected rejection rather than weakening path validation. |
| `rollback_failed:` | Automatic recovery was incomplete. Stop further updates and inspect retained backups, errors and journal evidence. |

The execution error `rolled_back:` reports a failed update whose recovery path completed its verification; it is not a successful update. Preserve the original failure details. A journal record describes an operation; it is not a substitute for the snapshot bytes.

If no usable snapshot remains, game-launcher repair/reinstallation may restore shipped files, but it can also remove mods. That external recovery behavior is not controlled or verified by DLSSync.

## Driver recovery is separate

Game DLL snapshots do not roll back an entire GPU installer or Windows device-driver update. The system-driver path exports an available third-party INF package before installation and saves a SHA-256 file manifest. A requested export failure stops installation. A System Restore checkpoint is a separate attempt and is not guaranteed.

Use the system-driver backup controls only for their matching driver snapshots. The app verifies the saved files before reinstalling them and marks a restore complete only after the device reports the saved active version. Windows driver ranking still applies, and a reboot may be required. When that evidence is unavailable, use the vendor or Windows recovery procedure; no universal driver rollback is documented. See [GPU and Windows drivers](drivers.md).

## Evidence

Source review: 2026-09-19. See [backup index](../crates/backup-store/src/lib.rs), [verified recovery](../crates/dlssync-application/src/transaction.rs), [restore command](../src-tauri/src/commands/backup.rs), [path guards and data roots](../src-tauri/src/paths.rs), [Backups UI](../frontend/src/views/Backups.svelte), [execution errors](../crates/dlssync-application/src/execution.rs) and [system-driver helper](../src-tauri/src/lib.rs). No live restore or driver recovery was performed for this guide.
