//! Durable write boundary. OS locks outlive Rust panics and disappear on process exit.
use crate::execution::{
    apply_prepared_observed_with_publication, restore_verified_backup, ExecutionError, PreparedFile,
};
use dlssync_contracts::{OperationActor, OperationStage};
use operation_journal::{JournalStore, RecoveryMember, RecoveryRecord};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Component, Path, PathBuf};

pub struct OperationLocks {
    _files: Vec<File>,
}

/// Holds the shared installation-root locks and exact target locks for a
/// durable writer that persists its own recovery payload.
pub struct CoordinatedTargets {
    _preparation: OperationLocks,
    _targets: OperationLocks,
    index: JournalStore,
}

impl OperationLocks {
    pub fn acquire(keys: &[String]) -> Result<Self, ExecutionError> {
        Self::acquire_modes(keys.iter().map(|key| (key.clone(), false)).collect())
    }

    fn acquire_modes(keys: BTreeMap<String, bool>) -> Result<Self, ExecutionError> {
        // Profiles and portable installs must coordinate access to the same files.
        // This namespace belongs to the Windows user, not to a backup database.
        let directory = coordination_directory();
        std::fs::create_dir_all(&directory)?;
        let mut files = Vec::with_capacity(keys.len());
        for (key, shared) in keys {
            let name = hex::encode(Sha256::digest(key.as_bytes()));
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(directory.join(name))?;
            let result = if shared {
                file.try_lock_shared()
            } else {
                file.try_lock()
            };
            result.map_err(|error| ExecutionError::Locked(format!("{key}: {error}")))?;
            files.push(file);
        }
        Ok(Self { _files: files })
    }
}

fn coordination_directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("DLSSync")
        .join("operation-locks")
}

fn target_index() -> Result<JournalStore, ExecutionError> {
    Ok(JournalStore::open(
        coordination_directory().join("pending-targets.sqlite3"),
    )?)
}

fn journal_identity(journal: &JournalStore) -> String {
    canonical_path(journal.path())
        .to_string_lossy()
        .into_owned()
}

fn register_record(
    index: &JournalStore,
    journal: &JournalStore,
    record: &RecoveryRecord,
) -> Result<(), ExecutionError> {
    for member in &record.members {
        index.register_target(
            &journal_identity(journal),
            &record.id,
            &file_key(&member.target),
        )?;
    }
    Ok(())
}

impl CoordinatedTargets {
    pub fn register_recovery(
        &self,
        journal: &JournalStore,
        record: &RecoveryRecord,
    ) -> Result<(), ExecutionError> {
        register_record(&self.index, journal, record)
    }
}

/// Coordinate a non-standard durable writer with normal apply and restore
/// operations. `allowed_recovery_id` is used only when that writer reopens its
/// own interrupted record.
pub fn coordinate_targets(
    journal: &JournalStore,
    paths: &[PathBuf],
    allowed_recovery_id: Option<&str>,
) -> Result<CoordinatedTargets, ExecutionError> {
    let preparation = lock_preparation(paths)?;
    let mut keys: Vec<_> = paths.iter().map(|path| file_key(path)).collect();
    keys.sort();
    keys.dedup();
    let targets = OperationLocks::acquire(&keys)?;
    let index = target_index()?;
    sync_targets(&index, journal, &keys)?;
    let owner = journal_identity(journal);
    let blocked =
        pending_owners(&index, &keys)?
            .into_iter()
            .any(|(pending_journal, pending, _)| {
                allowed_recovery_id != Some(pending.id.as_str())
                    || journal_identity(&pending_journal) != owner
            });
    if blocked {
        return Err(ExecutionError::Locked(
            "unfinished recovery touches this target; recover it first".into(),
        ));
    }
    Ok(CoordinatedTargets {
        _preparation: preparation,
        _targets: targets,
        index,
    })
}

/// Fence an older recovery payload after a verified operation supersedes it.
/// Both the shared index and owning journal retain the original evidence.
pub fn supersede_recovery_targets(
    journal: &JournalStore,
    recovery_id: &str,
    paths: &[PathBuf],
    decision: &str,
) -> Result<(), ExecutionError> {
    let index = target_index()?;
    let owner = journal_identity(journal);
    for path in paths {
        let key = file_key(path);
        index.supersede_target(&owner, recovery_id, &key, decision)?;
        journal.supersede_member(recovery_id, &path.to_string_lossy(), decision)?;
    }
    Ok(())
}

/// Import legacy local pending members while holding their target locks. Journals
/// stay in their profiles; only references and irrevocable decisions live here.
fn sync_targets(
    index: &JournalStore,
    journal: &JournalStore,
    keys: &[String],
) -> Result<(), ExecutionError> {
    for record in journal.pending_recovery()? {
        for member in &record.members {
            let key = file_key(&member.target);
            if !keys.contains(&key) {
                continue;
            }
            index.register_target(&journal_identity(journal), &record.id, &key)?;
            for (owner, id, decision) in index.indexed_targets(&key)? {
                if owner == journal_identity(journal) && id == record.id {
                    if let Some(decision) = decision {
                        journal.supersede_member(
                            &id,
                            &member.target.to_string_lossy(),
                            &decision,
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Caller owns all target locks. A missing journal (or an index reservation with
/// no committed record) may be taken over only after a FULL-synchronous tombstone
/// commits. That tombstone wins if the removed profile later reappears. Permission,
/// corruption and other I/O failures are NOT treated as absence. No evidence is deleted.
fn pending_owners(
    index: &JournalStore,
    keys: &[String],
) -> Result<Vec<(JournalStore, RecoveryRecord, String)>, ExecutionError> {
    let mut pending = Vec::new();
    for key in keys {
        for (owner, id, decision) in index.indexed_targets(key)? {
            if decision.is_some() {
                continue;
            }
            let path = PathBuf::from(&owner);
            if !path.try_exists()? {
                index.supersede_target(
                    &owner,
                    &id,
                    key,
                    "takeover: owning journal is missing; old recovery permanently fenced",
                )?;
                continue;
            }
            let journal = JournalStore::open(path)?;
            match journal.recovery_record(&id) {
                Err(error) if error.is_missing_record() => {
                    index.supersede_target(
                        &owner,
                        &id,
                        key,
                        "takeover: reservation has no committed recovery record",
                    )?;
                }
                Err(error) => return Err(error.into()),
                Ok(record) => {
                    if (!record.stage.is_terminal()
                        || record.stage == OperationStage::RollbackFailed)
                        && record
                            .members
                            .iter()
                            .any(|member| file_key(&member.target) == *key)
                    {
                        for member in &record.members {
                            if file_key(&member.target) == *key
                                && !journal
                                    .member_superseded(&id, &member.target.to_string_lossy())?
                            {
                                pending.push((journal.clone(), record.clone(), key.clone()));
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(pending)
}

fn file_keys(prepared: &[PreparedFile]) -> Vec<String> {
    prepared.iter().map(|file| file_key(&file.target)).collect()
}

pub async fn acquire_download_slot() -> tokio::sync::OwnedSemaphorePermit {
    static LIMIT: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
        std::sync::OnceLock::new();
    LIMIT
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(4)))
        .clone()
        .acquire_owned()
        .await
        .expect("download limiter stays open")
}

/// Exclusive roots with shared ancestors also exclude a restore of a nested file.
/// Disjoint games can prepare concurrently, including games on the same volume.
pub fn lock_preparation(paths: &[PathBuf]) -> Result<OperationLocks, ExecutionError> {
    let mut keys = BTreeMap::new();
    for path in paths {
        let canonical = canonical_path(path);
        for (index, ancestor) in canonical.ancestors().enumerate() {
            let key = format!("preparing:{}", path_identity(ancestor));
            let shared = index != 0;
            keys.entry(key)
                .and_modify(|value| *value &= shared)
                .or_insert(shared);
        }
    }
    OperationLocks::acquire_modes(keys)
}

/// A launcher ID is presentation identity. Locks use the observed installation root.
pub fn plan_game_roots(
    plan: &dlssync_contracts::UpdatePlan,
) -> Result<Vec<PathBuf>, ExecutionError> {
    let mut roots = Vec::new();
    for change in &plan.changes {
        let relative = Path::new(&change.precondition.identity.relative_path);
        if relative.as_os_str().is_empty()
            || !relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err(ExecutionError::UnsafeTarget(relative.display().to_string()));
        }
        let absolute = canonical_path(Path::new(&change.precondition.absolute_path));
        let mut root = absolute.clone();
        for _ in relative.components() {
            if !root.pop() {
                return Err(ExecutionError::UnsafeTarget(absolute.display().to_string()));
            }
        }
        if path_identity(&root.join(relative)) != path_identity(&absolute) {
            return Err(ExecutionError::UnsafeTarget(absolute.display().to_string()));
        }
        roots.push(root);
    }
    roots.sort();
    roots.dedup();
    Ok(roots)
}

fn canonical_path(path: &Path) -> PathBuf {
    path.canonicalize()
        .or_else(|error| {
            let parent = path.parent().ok_or(error)?;
            parent
                .canonicalize()
                .map(|parent| parent.join(path.file_name().unwrap_or_default()))
        })
        .unwrap_or_else(|_| path.to_path_buf())
}

fn path_identity(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

fn file_key(path: &Path) -> String {
    format!("file:{}", path_identity(&canonical_path(path)))
}

pub fn recovery_journal(
    backups: &backup_store::BackupStore,
) -> Result<JournalStore, ExecutionError> {
    Ok(JournalStore::open(
        backups.root_dir.join(".operations.sqlite3"),
    )?)
}

pub fn recover_store(
    backups: &backup_store::BackupStore,
) -> Result<Vec<RecoveryRecord>, ExecutionError> {
    let records = recover_pending_operations(&recovery_journal(backups)?, &backups.root_dir)?;
    let _ = sync_restore_verifications(backups);
    Ok(records)
}

pub fn sync_restore_verifications(
    backups: &backup_store::BackupStore,
) -> Result<usize, ExecutionError> {
    let journal = recovery_journal(backups)?;
    let entries = backups.list()?;
    let mut changed = 0;
    for record in journal.all_recovery_records()? {
        let at = record
            .updated_at
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);
        for member in record
            .members
            .iter()
            .filter(|member| member.stage == OperationStage::RolledBack)
        {
            for entry in entries.iter().filter(|entry| {
                member.backup_id.as_deref() == Some(entry.id.as_str())
                    || entry.backup_path == member.backup
            }) {
                if backups.restore_verification(&entry.id)?.is_some() {
                    continue;
                }
                backups.mark_restored(&entry.id, at)?;
                backups.record_restore_verification(
                    &entry.id,
                    &backup_store::RestoreVerificationRecord {
                        verified: true,
                        at: Some(at),
                        detail: Some(
                            "recovery journal records a restored destination SHA-256 match"
                                .to_string(),
                        ),
                    },
                )?;
                changed += 1;
            }
        }
    }
    Ok(changed)
}

pub fn project_recovery_history(
    backups: &backup_store::BackupStore,
    history: &JournalStore,
) -> Result<usize, ExecutionError> {
    let recovery = recovery_journal(backups)?;
    let source_store_id = recovery.source_store_id()?;
    let mut projected = 0;
    for record in recovery.all_recovery_records()? {
        let created_at = record
            .started_at
            .clone()
            .or_else(|| record.updated_at.clone())
            .unwrap_or_default();
        let status = match record.stage {
            OperationStage::Completed | OperationStage::RolledBack => {
                dlssync_contracts::OperationStatus::Succeeded
            }
            OperationStage::Cancelled => dlssync_contracts::OperationStatus::Cancelled,
            OperationStage::RollbackFailed | OperationStage::Blocked => {
                dlssync_contracts::OperationStatus::Failed
            }
            _ => dlssync_contracts::OperationStatus::Started,
        };
        let kind = match record.kind {
            operation_journal::RecoveryKind::ExplicitRestore
            | operation_journal::RecoveryKind::RollbackOnError
            | operation_journal::RecoveryKind::StartupRecovery => {
                dlssync_contracts::OperationKind::Rollback
            }
            _ => dlssync_contracts::OperationKind::DllApply,
        };
        let operation = dlssync_contracts::OperationRecord {
            id: format!("recovery:{source_store_id}:{}", record.id),
            created_at,
            actor: record.actor,
            kind,
            status,
            target: record.game_ids.first().cloned(),
            summary: match record.kind {
                operation_journal::RecoveryKind::ExplicitRestore => "Explicit backup restore",
                operation_journal::RecoveryKind::RollbackOnError => {
                    "Automatic rollback after failure"
                }
                operation_journal::RecoveryKind::StartupRecovery => "Startup recovery",
                _ => "Durable DLL operation",
            }
            .to_string(),
            details: BTreeMap::from([
                ("source_store_id".to_string(), source_store_id.clone()),
                ("recovery_record_id".to_string(), record.id.clone()),
                ("plan_id".to_string(), record.plan_id.clone()),
                ("stage".to_string(), format!("{:?}", record.stage)),
            ]),
            duration_ms: None,
            backup_id: record
                .members
                .iter()
                .find_map(|member| member.backup_id.clone()),
            error: record.error.clone(),
        };
        if history.project_recovery_once(&source_store_id, &record, &operation)? {
            projected += 1;
        }
    }
    Ok(projected)
}

/// Explicit restore and CLI rollback share the same durable recovery operation.
/// Legacy backups without a creation hash retain that provenance gap; the bytes
/// selected for this restore still receive a fresh hash and destination readback.
pub fn restore_entries(
    backups: &backup_store::BackupStore,
    entries: &[backup_store::BackupEntry],
    actor: OperationActor,
) -> Result<usize, ExecutionError> {
    let root = backups.root_dir.canonicalize()?;
    let paths: Vec<_> = entries
        .iter()
        .map(|entry| entry.original_path.clone())
        .collect();
    let _preparation = lock_preparation(&paths)?;
    let keys: Vec<_> = entries
        .iter()
        .map(|entry| file_key(&entry.original_path))
        .collect();
    let _locks = OperationLocks::acquire(&keys)?;
    let mut members = Vec::with_capacity(entries.len());
    for entry in entries {
        if !entry.backup_path.canonicalize()?.starts_with(&root) {
            return Err(ExecutionError::UnsafeTarget(
                entry.backup_path.display().to_string(),
            ));
        }
        let hash = dll_catalog::hex_sha256_file(&entry.backup_path)?;
        if entry
            .previous_sha256
            .as_ref()
            .is_some_and(|expected| !expected.eq_ignore_ascii_case(&hash))
        {
            return Err(ExecutionError::Integrity(format!(
                "backup creation hash mismatch: {}",
                entry.id
            )));
        }
        let current_hash = if entry.original_path.exists() {
            dll_catalog::hex_sha256_file(&entry.original_path)?
        } else {
            hash.clone()
        };
        members.push(RecoveryMember {
            target: entry.original_path.clone(),
            backup: entry.backup_path.clone(),
            previous_sha256: hash,
            expected_sha256: current_hash,
            stage: OperationStage::Applying,
            game_id: Some(entry.game_id.clone()),
            component_id: Some(format!("{}:{}", entry.game_id, entry.dll_filename)),
            backup_id: Some(entry.id.clone()),
        });
    }
    for (entry, member) in entries.iter().zip(&members) {
        if entry.original_path.is_file() {
            let id = uuid::Uuid::new_v4().to_string();
            let filename = entry.original_path.file_name().ok_or_else(|| {
                ExecutionError::UnsafeTarget(entry.original_path.display().to_string())
            })?;
            let saved = root.join("restore-undo").join(&id).join(filename);
            crate::execution::create_verified_backup(
                &entry.original_path,
                &saved,
                &member.expected_sha256,
            )?;
            let mut undo = entry.clone();
            undo.id = id;
            undo.backup_path = saved;
            undo.previous_sha256 = Some(member.expected_sha256.clone());
            undo.previous_version = pe_version::read_dll_version(&entry.original_path)
                .ok()
                .map(|version| version.file_version);
            undo.created_at = chrono::Utc::now();
            undo.restored_at = None;
            undo.size_bytes = Some(std::fs::metadata(&entry.original_path)?.len());
            backups.insert(&undo)?;
        }
    }
    let journal = recovery_journal(backups)?;
    let index = target_index()?;
    sync_targets(&index, &journal, &keys)?;
    let superseded = pending_owners(&index, &keys)?;
    let mut record = RecoveryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        plan_id: format!(
            "restore:{}",
            entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ),
        actor,
        stage: OperationStage::Applying,
        members,
        error: None,
        source_store_id: journal.source_store_id().ok(),
        operation_id: None,
        game_ids: entries.iter().map(|entry| entry.game_id.clone()).collect(),
        kind: operation_journal::RecoveryKind::ExplicitRestore,
        started_at: Some(chrono::Utc::now().to_rfc3339()),
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
    };
    register_record(&index, &journal, &record)?;
    journal.save_recovery(&record)?;
    recover_record(&journal, &root, &mut record)?;
    if record.stage != OperationStage::RolledBack {
        return Err(ExecutionError::RollbackFailed {
            cause: "explicit restore".into(),
            errors: vec![record.error.unwrap_or_default()],
        });
    }
    // Verified user intent wins, but neither old payloads nor backup files are
    // rewritten. Commit the shared fence first so a crash cannot revive recovery.
    let decision = format!(
        "verified manual restore {} in {}",
        record.id,
        journal_identity(&journal)
    );
    for (owner, pending, key) in superseded {
        index.supersede_target(&journal_identity(&owner), &pending.id, &key, &decision)?;
        for member in &pending.members {
            if file_key(&member.target) == key {
                owner.supersede_member(&pending.id, &member.target.to_string_lossy(), &decision)?;
            }
        }
    }
    for entry in entries {
        let verified_at = chrono::Utc::now();
        backups.mark_restored(&entry.id, verified_at)?;
        let _ = backups.record_restore_verification(
            &entry.id,
            &backup_store::RestoreVerificationRecord {
                verified: true,
                at: Some(verified_at),
                detail: Some("restored file SHA-256 matched the selected backup".to_string()),
            },
        );
    }
    Ok(entries.len())
}

/// Requires all downloads, signatures, backups and plan validation to finish
/// before entry. Completion is committed before the caller receives success.
pub fn apply_durable(
    journal: &JournalStore,
    lock_root: &Path,
    plan_id: &str,
    actor: OperationActor,
    prepared: &[PreparedFile],
    validate: impl FnMut(&PreparedFile) -> Result<(), ExecutionError>,
) -> Result<usize, ExecutionError> {
    apply_durable_observed(
        journal,
        lock_root,
        plan_id,
        actor,
        prepared,
        validate,
        |_, _| Ok(()),
    )
}

pub fn apply_durable_observed(
    journal: &JournalStore,
    lock_root: &Path,
    plan_id: &str,
    actor: OperationActor,
    prepared: &[PreparedFile],
    validate: impl FnMut(&PreparedFile) -> Result<(), ExecutionError>,
    mut publication: impl FnMut(usize, OperationStage) -> Result<(), String>,
) -> Result<usize, ExecutionError> {
    let keys = file_keys(prepared);
    let _locks = OperationLocks::acquire(&keys)?;
    let index = target_index()?;
    sync_targets(&index, journal, &keys)?;
    if !pending_owners(&index, &keys)?.is_empty() {
        return Err(ExecutionError::Locked(
            "unfinished recovery touches this target; recover it first".into(),
        ));
    }
    let mut record = RecoveryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        plan_id: plan_id.into(),
        actor,
        stage: OperationStage::Planned,
        error: None,
        source_store_id: journal.source_store_id().ok(),
        operation_id: None,
        game_ids: Vec::new(),
        kind: operation_journal::RecoveryKind::Apply,
        started_at: Some(chrono::Utc::now().to_rfc3339()),
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
        members: prepared
            .iter()
            .map(|file| RecoveryMember {
                target: file.target.clone(),
                backup: file.backup.clone(),
                previous_sha256: file.previous_sha256.clone(),
                expected_sha256: file.expected_sha256.clone(),
                stage: OperationStage::Planned,
                game_id: None,
                component_id: None,
                backup_id: None,
            })
            .collect(),
    };
    register_record(&index, journal, &record)?;
    journal.save_recovery(&record)?;
    let result = apply_prepared_observed_with_publication(
        prepared,
        validate,
        |index, stage| {
            record.stage = stage;
            record.updated_at = Some(chrono::Utc::now().to_rfc3339());
            record.members[index].stage = if stage == OperationStage::VerifyingInstalled {
                OperationStage::Completed
            } else {
                stage
            };
            journal.save_recovery(&record)?;
            Ok(())
        },
        &mut publication,
    );
    match result {
        Ok(count) => {
            record.stage = OperationStage::Completed;
            record.updated_at = Some(chrono::Utc::now().to_rfc3339());
            if let Err(error) = journal.save_recovery(&record) {
                let cause = error.to_string();
                record.error = Some(cause.clone());
                let recovered = recover_record(journal, lock_root, &mut record);
                return Err(match recovered {
                    Ok(()) if record.stage == OperationStage::RolledBack => {
                        ExecutionError::RolledBack { cause }
                    }
                    other => ExecutionError::RollbackFailed {
                        cause,
                        errors: vec![format!("{other:?}; {:?}", record.error)],
                    },
                });
            }
            let _ = publication(prepared.len(), OperationStage::Completed);
            Ok(count)
        }
        Err(error) => {
            record.stage = match &error {
                ExecutionError::Cancelled => OperationStage::Cancelled,
                ExecutionError::RolledBack { .. } => OperationStage::RolledBack,
                ExecutionError::RollbackFailed { .. } => OperationStage::RollbackFailed,
                _ => OperationStage::Blocked,
            };
            record.error = Some(error.to_string());
            record.updated_at = Some(chrono::Utc::now().to_rfc3339());
            for member in &mut record.members {
                if matches!(
                    member.stage,
                    OperationStage::Applying | OperationStage::Completed
                ) {
                    member.stage = if dll_catalog::hex_sha256_file(&member.target)
                        .is_ok_and(|hash| hash.eq_ignore_ascii_case(&member.previous_sha256))
                    {
                        OperationStage::RolledBack
                    } else {
                        OperationStage::RollbackFailed
                    };
                } else {
                    member.stage = if matches!(error, ExecutionError::Cancelled) {
                        OperationStage::Cancelled
                    } else {
                        OperationStage::Blocked
                    };
                }
            }
            if let Err(journal_error) = journal.save_recovery(&record) {
                return Err(ExecutionError::RollbackFailed {
                    cause: error.to_string(),
                    errors: vec![format!("recovery state not committed: {journal_error}")],
                });
            }
            let _ = publication(prepared.len(), record.stage);
            Err(error)
        }
    }
}

fn recover_record(
    journal: &JournalStore,
    backup_root: &Path,
    record: &mut RecoveryRecord,
) -> Result<(), ExecutionError> {
    record.stage = OperationStage::RollingBack;
    journal.save_recovery(record)?;
    let root = backup_root.canonicalize()?;
    let mut errors = Vec::new();
    for index in (0..record.members.len()).rev() {
        let member = &record.members[index];
        if journal.member_superseded(&record.id, &member.target.to_string_lossy())? {
            continue;
        }
        // Planned/blocked entries never crossed the persisted write boundary.
        if matches!(
            member.stage,
            OperationStage::Planned | OperationStage::Blocked
        ) {
            continue;
        }
        let restored = (|| -> Result<(), ExecutionError> {
            if !member.backup.canonicalize()?.starts_with(&root) {
                return Err(ExecutionError::UnsafeTarget(
                    member.backup.display().to_string(),
                ));
            }
            match dll_catalog::hex_sha256_file(&member.target) {
                Ok(hash)
                    if !hash.eq_ignore_ascii_case(&member.previous_sha256)
                        && !hash.eq_ignore_ascii_case(&member.expected_sha256) =>
                {
                    return Err(ExecutionError::Stale(format!(
                        "externally changed target retained: {}",
                        member.target.display()
                    )));
                }
                Err(error) if member.target.exists() => return Err(error.into()),
                _ => {}
            }
            restore_verified_backup(&member.backup, &member.target, &member.previous_sha256)
        })();
        record.members[index].stage = match restored {
            Ok(()) => OperationStage::RolledBack,
            Err(error) => {
                errors.push(error.to_string());
                OperationStage::RollbackFailed
            }
        };
        if let Err(error) = journal.save_recovery(record) {
            errors.push(error.to_string());
        }
    }
    record.stage = if errors.is_empty() {
        OperationStage::RolledBack
    } else {
        OperationStage::RollbackFailed
    };
    if !errors.is_empty() {
        record.error = Some(errors.join("; "));
    }
    journal.save_recovery(record)?;
    Ok(())
}

/// Called during startup. Never restore an operation owned by a live process,
/// and never overwrite bytes installed by somebody else after interruption.
pub fn recover_pending_operations(
    journal: &JournalStore,
    backup_root: &Path,
) -> Result<Vec<RecoveryRecord>, ExecutionError> {
    let mut recovered = Vec::new();
    for pending in journal.pending_recovery()? {
        // Recipe transactions keep a richer presence-aware payload and use
        // this generic record only as the shared target fence.
        if pending.kind == operation_journal::RecoveryKind::Unknown
            && pending.members.iter().all(|member| {
                member
                    .component_id
                    .as_deref()
                    .is_some_and(|component| component.starts_with("recipe:"))
            })
        {
            continue;
        }
        let paths: Vec<_> = pending
            .members
            .iter()
            .map(|member| member.target.clone())
            .collect();
        let _preparation = match lock_preparation(&paths) {
            Ok(locks) => locks,
            Err(ExecutionError::Locked(_)) => continue,
            Err(error) => return Err(error),
        };
        let keys: Vec<_> = pending
            .members
            .iter()
            .map(|member| file_key(&member.target))
            .collect();
        let _locks = match OperationLocks::acquire(&keys) {
            Ok(locks) => locks,
            Err(ExecutionError::Locked(_)) => continue,
            Err(error) => return Err(error),
        };
        let index = target_index()?;
        sync_targets(&index, journal, &keys)?;
        // Never run a legacy/unindexed rollback over a different pending owner.
        if pending_owners(&index, &keys)?
            .iter()
            .any(|(owner, record, _)| {
                journal_identity(owner) != journal_identity(journal) || record.id != pending.id
            })
        {
            continue;
        }
        // Another process may have completed while the initial query was read.
        let Some(mut current) = journal
            .pending_recovery()?
            .into_iter()
            .find(|record| record.id == pending.id)
        else {
            continue;
        };
        recover_record(journal, backup_root, &mut current)?;
        recovered.push(current);
    }
    Ok(recovered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn restore_fixture(root: &Path) -> (backup_store::BackupStore, backup_store::BackupEntry) {
        let store =
            backup_store::BackupStore::open(root.join("backups.db"), root.join("backups")).unwrap();
        let target = root.join("game/game.dll");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let backup = store.root_dir.join("original.dll");
        std::fs::write(&target, b"new").unwrap();
        std::fs::write(&backup, b"old").unwrap();
        let entry = backup_store::BackupEntry {
            id: "original".into(),
            game_id: "game".into(),
            dll_family: "dlss_sr".into(),
            dll_filename: "game.dll".into(),
            original_path: target,
            backup_path: backup,
            previous_version: None,
            previous_sha256: Some(hex::encode(Sha256::digest(b"old"))),
            created_at: chrono::Utc::now(),
            restored_at: None,
            size_bytes: Some(3),
            backup_type: "dll".into(),
            device_class: None,
            hardware_id: None,
            driver_provider: None,
        };
        store.insert(&entry).unwrap();
        (store, entry)
    }

    #[test]
    fn manual_restore_verifies_bytes_and_preserves_an_undo_backup() {
        let root = tempfile::tempdir().unwrap();
        let (store, entry) = restore_fixture(root.path());
        assert_eq!(
            restore_entries(&store, std::slice::from_ref(&entry), OperationActor::Gui).unwrap(),
            1
        );
        assert_eq!(std::fs::read(&entry.original_path).unwrap(), b"old");
        assert!(store.get(&entry.id).unwrap().restored_at.is_some());
        let verification = store.restore_verification(&entry.id).unwrap().unwrap();
        assert!(verification.verified);
        assert!(verification.at.is_some());
        let undo = store
            .list()
            .unwrap()
            .into_iter()
            .find(|saved| saved.id != entry.id)
            .unwrap();
        assert_eq!(std::fs::read(&undo.backup_path).unwrap(), b"new");
        assert!(recovery_journal(&store)
            .unwrap()
            .pending_recovery()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn manual_restore_can_recreate_a_deleted_file() {
        let root = tempfile::tempdir().unwrap();
        let (store, entry) = restore_fixture(root.path());
        std::fs::remove_file(&entry.original_path).unwrap();
        restore_entries(&store, std::slice::from_ref(&entry), OperationActor::Gui).unwrap();
        assert_eq!(std::fs::read(&entry.original_path).unwrap(), b"old");
    }

    #[test]
    fn corrupt_backup_is_rejected_before_manual_restore_writes() {
        let root = tempfile::tempdir().unwrap();
        let (store, entry) = restore_fixture(root.path());
        std::fs::write(&entry.backup_path, b"tampered").unwrap();
        assert!(
            restore_entries(&store, std::slice::from_ref(&entry), OperationActor::Gui).is_err()
        );
        assert_eq!(std::fs::read(&entry.original_path).unwrap(), b"new");
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn legacy_restore_does_not_invent_a_creation_hash() {
        let root = tempfile::tempdir().unwrap();
        let (store, mut entry) = restore_fixture(root.path());
        entry.id = "legacy".into();
        entry.previous_sha256 = None;
        store.insert(&entry).unwrap();
        restore_entries(&store, std::slice::from_ref(&entry), OperationActor::Gui).unwrap();
        assert_eq!(std::fs::read(&entry.original_path).unwrap(), b"old");
        assert!(store.get(&entry.id).unwrap().previous_sha256.is_none());
    }

    #[test]
    fn cancellation_before_writes_leaves_original_bytes() {
        let root = tempfile::tempdir().unwrap();
        let (journal, file) = fixture(root.path());
        let result = apply_durable(
            &journal,
            root.path(),
            "cancel",
            OperationActor::Gui,
            std::slice::from_ref(&file),
            |_| Err(ExecutionError::Cancelled),
        );
        assert!(matches!(result, Err(ExecutionError::Cancelled)));
        assert_eq!(std::fs::read(&file.target).unwrap(), b"old");
        assert!(journal.pending_recovery().unwrap().is_empty());
    }

    #[test]
    fn cancellation_between_writes_restores_the_complete_set() {
        let root = tempfile::tempdir().unwrap();
        let (journal, first) = fixture(root.path());
        let (_, second) = fixture(&root.path().join("second"));
        let files = [first, second];
        let mut checks = 0;
        let result = apply_durable(
            &journal,
            root.path(),
            "cancel",
            OperationActor::Gui,
            &files,
            |_| {
                checks += 1;
                if checks == 4 {
                    Err(ExecutionError::Cancelled)
                } else {
                    Ok(())
                }
            },
        );
        assert!(matches!(result, Err(ExecutionError::RolledBack { .. })));
        for file in &files {
            assert_eq!(std::fs::read(&file.target).unwrap(), b"old");
        }
        assert!(journal.pending_recovery().unwrap().is_empty());
    }
    fn fixture(root: &Path) -> (JournalStore, PreparedFile) {
        let journal = JournalStore::open(root.join("journal.db")).unwrap();
        let file = PreparedFile {
            expected_version: None,
            target: root.join("game.dll"),
            staged: root.join("staged.dll"),
            backup: root.join("backup.dll"),
            previous_sha256: hex::encode(Sha256::digest(b"old")),
            expected_sha256: hex::encode(Sha256::digest(b"new")),
        };
        std::fs::write(&file.target, b"old").unwrap();
        std::fs::write(&file.backup, b"old").unwrap();
        std::fs::write(&file.staged, b"new").unwrap();
        (journal, file)
    }
    fn interrupted(journal: &JournalStore, file: &PreparedFile) {
        journal
            .save_recovery(&RecoveryRecord {
                id: "interrupted".into(),
                plan_id: "plan".into(),
                actor: OperationActor::Cli,
                stage: OperationStage::Applying,
                error: None,
                source_store_id: None,
                operation_id: None,
                game_ids: Vec::new(),
                kind: operation_journal::RecoveryKind::Apply,
                started_at: None,
                updated_at: None,
                members: vec![RecoveryMember {
                    target: file.target.clone(),
                    backup: file.backup.clone(),
                    previous_sha256: file.previous_sha256.clone(),
                    expected_sha256: file.expected_sha256.clone(),
                    stage: OperationStage::Applying,
                    game_id: None,
                    component_id: None,
                    backup_id: None,
                }],
            })
            .unwrap();
        std::fs::write(&file.target, b"new").unwrap();
    }

    #[test]
    fn same_game_preparation_and_nested_restore_cannot_overlap() {
        let game = tempfile::tempdir().unwrap();
        let paths = vec![game.path().to_path_buf()];
        let nested = game.path().join("bin/game.dll");
        std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        std::fs::write(&nested, b"old").unwrap();
        let gui = lock_preparation(&paths).unwrap();
        assert!(matches!(
            lock_preparation(&paths),
            Err(ExecutionError::Locked(_))
        ));
        assert!(matches!(
            lock_preparation(std::slice::from_ref(&nested)),
            Err(ExecutionError::Locked(_))
        ));
        drop(gui);
        let _restore = lock_preparation(&[nested]).unwrap();
        assert!(matches!(
            lock_preparation(&paths),
            Err(ExecutionError::Locked(_))
        ));
    }

    #[test]
    fn disjoint_game_preparations_do_not_block_each_other() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let _first = lock_preparation(&[first]).unwrap();
        assert!(lock_preparation(&[second]).is_ok());
    }

    #[test]
    fn duplicate_lock_rejected_and_drop_releases_it() {
        let root = tempfile::tempdir().unwrap();
        let keys = vec![file_key(root.path())];
        let lock = OperationLocks::acquire(&keys).unwrap();
        assert!(matches!(
            OperationLocks::acquire(&keys),
            Err(ExecutionError::Locked(_))
        ));
        drop(lock);
        assert!(OperationLocks::acquire(&keys).is_ok());
    }

    #[test]
    fn completed_transaction_does_not_recover_on_reopen() {
        let root = tempfile::tempdir().unwrap();
        let (journal, file) = fixture(root.path());
        apply_durable(
            &journal,
            root.path(),
            "plan",
            OperationActor::Cli,
            std::slice::from_ref(&file),
            |_| Ok(()),
        )
        .unwrap();
        let reopened = JournalStore::open(root.path().join("journal.db")).unwrap();
        assert!(reopened.pending_recovery().unwrap().is_empty());
        assert_eq!(std::fs::read(&file.target).unwrap(), b"new");
    }

    #[test]
    fn interrupted_write_is_restored_after_store_reopen() {
        let root = tempfile::tempdir().unwrap();
        let (journal, file) = fixture(root.path());
        interrupted(&journal, &file);
        drop(journal);
        let reopened = JournalStore::open(root.path().join("journal.db")).unwrap();
        let result = recover_pending_operations(&reopened, root.path()).unwrap();
        assert_eq!(result[0].stage, OperationStage::RolledBack);
        assert_eq!(std::fs::read(&file.target).unwrap(), b"old");
        assert!(reopened.pending_recovery().unwrap().is_empty());
    }

    #[test]
    fn recovery_retains_external_changes_and_corrupt_backup_evidence() {
        let root = tempfile::tempdir().unwrap();
        let (journal, file) = fixture(root.path());
        interrupted(&journal, &file);
        std::fs::write(&file.target, b"external").unwrap();
        assert_eq!(
            recover_pending_operations(&journal, root.path()).unwrap()[0].stage,
            OperationStage::RollbackFailed
        );
        assert_eq!(std::fs::read(&file.target).unwrap(), b"external");
        std::fs::write(&file.target, b"new").unwrap();
        std::fs::write(&file.backup, b"corrupt").unwrap();
        assert_eq!(
            recover_pending_operations(&journal, root.path()).unwrap()[0].stage,
            OperationStage::RollbackFailed
        );
        assert_eq!(std::fs::read(&file.target).unwrap(), b"new");
        assert!(file.backup.exists());
    }

    #[test]
    fn active_process_lock_prevents_startup_recovery() {
        let root = tempfile::tempdir().unwrap();
        let (journal, file) = fixture(root.path());
        interrupted(&journal, &file);
        let _lock = OperationLocks::acquire(&file_keys(std::slice::from_ref(&file))).unwrap();
        assert!(recover_pending_operations(&journal, root.path())
            .unwrap()
            .is_empty());
        assert_eq!(std::fs::read(&file.target).unwrap(), b"new");
    }

    #[test]
    fn unreadable_installed_version_recovers_before_completion_is_committed() {
        let root = tempfile::tempdir().unwrap();
        let (journal, mut file) = fixture(root.path());
        file.expected_version = Some("1.2.3.4".into());
        let result = apply_durable(
            &journal,
            root.path(),
            "plan",
            OperationActor::Cli,
            std::slice::from_ref(&file),
            |_| Ok(()),
        );
        assert!(matches!(result, Err(ExecutionError::RolledBack { .. })));
        assert_eq!(std::fs::read(&file.target).unwrap(), b"old");
        assert!(journal.pending_recovery().unwrap().is_empty());
    }

    #[test]
    #[ignore = "helper executed by the parent restart test in a disposable process"]
    fn crash_child() {
        let root = std::path::PathBuf::from(
            std::env::var_os("DLSSYNC_RECOVERY_TEST_ROOT").expect("test root"),
        );
        let (journal, first) = fixture(&root);
        let (_, second) = fixture(&root.join("second"));
        let files = [first, second];
        let mut checks = 0;
        let _ = apply_durable(
            &journal,
            &root,
            "crash-plan",
            OperationActor::Cli,
            &files,
            |_| {
                checks += 1;
                if checks == 4 {
                    assert_eq!(std::fs::read(&files[0].target).unwrap(), b"new");
                    std::process::exit(23);
                }
                Ok(())
            },
        );
        panic!("crash boundary was not reached");
    }

    #[test]
    fn process_exit_between_members_releases_locks_and_recovers_on_restart() {
        let root = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "transaction::tests::crash_child",
                "--ignored",
                "--nocapture",
            ])
            .env("DLSSYNC_RECOVERY_TEST_ROOT", root.path())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(23));
        let journal = JournalStore::open(root.path().join("journal.db")).unwrap();
        assert_eq!(std::fs::read(root.path().join("game.dll")).unwrap(), b"new");
        let recovered = recover_pending_operations(&journal, root.path()).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].stage, OperationStage::RolledBack);
        assert_eq!(std::fs::read(root.path().join("game.dll")).unwrap(), b"old");
        assert_eq!(
            std::fs::read(root.path().join("second/game.dll")).unwrap(),
            b"old"
        );
        assert!(journal.pending_recovery().unwrap().is_empty());
    }
}
