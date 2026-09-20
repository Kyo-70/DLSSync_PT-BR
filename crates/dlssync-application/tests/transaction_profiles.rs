use dlssync_application::{execution::PreparedFile, transaction::*};
use dlssync_contracts::{OperationActor, OperationStage};
use operation_journal::{RecoveryMember, RecoveryRecord};
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn prepared(root: &Path, name: &str) -> Result<PreparedFile, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(root)?;
    let file = PreparedFile {
        target: root.join(format!("{name}.dll")),
        staged: root.join(format!("{name}.staged")),
        backup: root.join(format!("{name}.backup")),
        previous_sha256: hex::encode(sha2::Sha256::digest(b"old")),
        expected_sha256: hex::encode(sha2::Sha256::digest(b"new")),
        expected_version: None,
    };
    std::fs::write(&file.target, b"old")?;
    std::fs::write(&file.backup, b"old")?;
    std::fs::write(&file.staged, b"new")?;
    Ok(file)
}
use sha2::Digest;

// Environment is changed only for a disposable child, never the parallel test host.
fn isolated(name: &str) -> Result<bool, Box<dyn std::error::Error>> {
    if std::env::var("TRANSACTION_PROFILE_CHILD").as_deref() == Ok(name) {
        return Ok(false);
    }
    let temp = tempfile::tempdir()?;
    let status = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", name, "--nocapture"])
        .env("TRANSACTION_PROFILE_CHILD", name)
        .env("LOCALAPPDATA", temp.path())
        .status()?;
    assert!(status.success(), "isolated child failed: {status}");
    Ok(true)
}

#[test]
fn cross_profile_pending_package_blocks_later_apply() -> TestResult {
    if isolated("cross_profile_pending_package_blocks_later_apply")? {
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let a = backup_store::BackupStore::open(temp.path().join("a.db"), temp.path().join("a"))?;
    let b = backup_store::BackupStore::open(temp.path().join("b.db"), temp.path().join("b"))?;
    let mut files = [
        prepared(&a.root_dir, "first")?,
        prepared(&a.root_dir, "second")?,
    ];
    let game = temp.path().join("game");
    std::fs::create_dir_all(&game)?;
    for (index, file) in files.iter_mut().enumerate() {
        let target = game.join(format!("member-{index}.dll"));
        std::fs::rename(&file.target, &target)?;
        file.target = target;
    }
    let ja = recovery_journal(&a)?;
    let mut checks = 0;
    let crash = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = apply_durable(
            &ja,
            &a.root_dir,
            "interrupted",
            OperationActor::Gui,
            &files,
            |_| {
                checks += 1;
                assert!(
                    checks != 4,
                    "simulated process interruption between package members"
                );
                Ok(())
            },
        );
    }));
    assert!(crash.is_err());
    assert_eq!(std::fs::read(&files[0].target)?, b"new");
    let mut later = files.to_vec();
    later[0].previous_sha256 = later[0].expected_sha256.clone();
    later[0].backup = b.root_dir.join("first.backup");
    std::fs::write(&later[0].backup, b"new")?;
    let result = apply_durable(
        &recovery_journal(&b)?,
        &b.root_dir,
        "later",
        OperationActor::Gui,
        &later,
        |_| Ok(()),
    );
    assert!(
        matches!(
            result,
            Err(dlssync_application::execution::ExecutionError::Locked(_))
        ),
        "cross-profile apply was not blocked: {result:?}"
    );
    assert_eq!(std::fs::read(&files[1].target)?, b"old");
    let recovered = recover_store(&a)?;
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].stage, OperationStage::RolledBack);
    assert_eq!(
        apply_durable(
            &recovery_journal(&b)?,
            &a.root_dir,
            "after-recovery",
            OperationActor::Gui,
            &files,
            |_| Ok(())
        )?,
        2
    );
    assert!(recover_store(&a)?.is_empty());
    Ok(())
}

#[test]
fn missing_profile_takeover_fences_returning_old_journal() -> TestResult {
    if isolated("missing_profile_takeover_fences_returning_old_journal")? {
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let a = backup_store::BackupStore::open(temp.path().join("a.db"), temp.path().join("a"))?;
    let b = backup_store::BackupStore::open(temp.path().join("b.db"), temp.path().join("b"))?;
    let mut file = prepared(&a.root_dir, "game")?;
    let target = temp.path().join("game.dll");
    std::fs::rename(&file.target, &target)?;
    file.target = target;
    let ja = recovery_journal(&a)?;
    let crash = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = apply_durable(
            &ja,
            &a.root_dir,
            "interrupted",
            OperationActor::Gui,
            std::slice::from_ref(&file),
            |_| {
                panic!("interrupted before write");
            },
        );
    }));
    assert!(crash.is_err());
    let old_id = ja
        .pending_recovery()?
        .first()
        .ok_or("missing pending record")?
        .id
        .clone();
    let hidden = temp.path().join("hidden-profile");
    std::fs::rename(&a.root_dir, &hidden)?;
    let mut later = prepared(&b.root_dir, "game")?;
    later.target = file.target.clone();
    assert_eq!(
        apply_durable(
            &recovery_journal(&b)?,
            &b.root_dir,
            "takeover",
            OperationActor::Gui,
            &[later],
            |_| Ok(())
        )?,
        1
    );
    std::fs::rename(&hidden, &a.root_dir)?;
    assert!(recover_store(&a)?.is_empty());
    assert!(ja.pending_recovery()?.is_empty());
    assert_eq!(ja.recovery_record(&old_id)?.stage, OperationStage::Planned);
    assert_eq!(std::fs::read(&file.target)?, b"new");
    assert_eq!(std::fs::read(&file.backup)?, b"old");
    let index = operation_journal::JournalStore::open(
        std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("missing test namespace")?)
            .join("DLSSync/operation-locks/pending-targets.sqlite3"),
    )?;
    let key = format!(
        "file:{}",
        file.target
            .canonicalize()?
            .to_string_lossy()
            .replace('\\', "/")
            .to_lowercase()
    );
    assert!(index
        .indexed_targets(&key)?
        .iter()
        .any(|(_, id, decision)| id == &old_id
            && decision
                .as_ref()
                .is_some_and(|text| text.contains("owning journal is missing"))));
    Ok(())
}

#[test]
fn manual_restore_supersedes_failed_rollback_preserving_diagnostics() -> TestResult {
    if isolated("manual_restore_supersedes_failed_rollback_preserving_diagnostics")? {
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let store = backup_store::BackupStore::open(
        temp.path().join("backups.db"),
        temp.path().join("backups"),
    )?;
    let file = prepared(&store.root_dir, "game")?;
    let journal = recovery_journal(&store)?;
    let record = RecoveryRecord {
        id: "failed".into(),
        plan_id: "failed-plan".into(),
        actor: OperationActor::Gui,
        stage: OperationStage::RollbackFailed,
        error: Some("original failure diagnostic".into()),
        source_store_id: None,
        operation_id: None,
        game_ids: Vec::new(),
        kind: operation_journal::RecoveryKind::Unknown,
        started_at: None,
        updated_at: None,
        members: vec![RecoveryMember {
            target: file.target.clone(),
            backup: file.backup.clone(),
            previous_sha256: file.previous_sha256.clone(),
            expected_sha256: file.expected_sha256.clone(),
            stage: OperationStage::RollbackFailed,
            game_id: None,
            component_id: None,
            backup_id: None,
        }],
    };
    journal.save_recovery(&record)?;
    let history = store.root_dir.join("historical.dll");
    std::fs::write(&history, b"historical")?;
    let entry = backup_store::BackupEntry {
        id: "historical".into(),
        game_id: "game".into(),
        dll_family: "dlss_sr".into(),
        dll_filename: "game.dll".into(),
        original_path: file.target.clone(),
        backup_path: history,
        previous_version: None,
        previous_sha256: Some(hex::encode(sha2::Sha256::digest(b"historical"))),
        created_at: chrono::Utc::now(),
        restored_at: None,
        size_bytes: Some(10),
        backup_type: "dll".into(),
        device_class: None,
        hardware_id: None,
        driver_provider: None,
    };
    store.insert(&entry)?;
    restore_entries(&store, std::slice::from_ref(&entry), OperationActor::Gui)?;
    assert!(
        journal.pending_recovery()?.is_empty(),
        "manual restore left a pending rollback"
    );
    assert_eq!(journal.recovery_record("failed")?.error, record.error);
    assert_eq!(std::fs::read(&file.backup)?, b"old");
    let next_backup = store.root_dir.join("next.backup");
    std::fs::write(&next_backup, b"historical")?;
    let next = PreparedFile {
        previous_sha256: entry.previous_sha256.ok_or("missing fixture hash")?,
        backup: next_backup,
        ..file
    };
    assert_eq!(
        apply_durable(
            &journal,
            &store.root_dir,
            "next",
            OperationActor::Gui,
            &[next],
            |_| Ok(())
        )?,
        1
    );
    assert!(recover_store(&store)?.is_empty());
    Ok(())
}
