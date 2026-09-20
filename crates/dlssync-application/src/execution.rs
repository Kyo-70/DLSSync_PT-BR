use backup_store::{BackupEntry, BackupStore};
use dll_catalog::Catalog;
use dlssync_contracts::{ApplyPlanResult, RollbackPlanResult, UpdatePlan};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("update plan is stale: {0}")]
    Stale(String),
    #[error("unsafe target path: {0}")]
    UnsafeTarget(String),
    #[error("catalog release missing for {0}")]
    MissingRelease(String),
    #[error("download or integrity verification failed: {0}")]
    Catalog(#[from] dll_catalog::CatalogError),
    #[error("backup failed: {0}")]
    Backup(#[from] backup_store::BackupError),
    #[error("filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Authenticode verification failed: {0}")]
    Authenticode(String),
    #[error("incompatible binary architecture: {0}")]
    Architecture(String),
    #[error("integrity verification failed: {0}")]
    Integrity(String),
    #[error("operation journal failed: {0}")]
    Journal(#[from] operation_journal::JournalError),
    #[error("operation locked: {0}")]
    Locked(String),
    #[error("cancelled")]
    Cancelled,
    #[error("rolled_back: {cause}")]
    RolledBack { cause: String },
    #[error("rollback_failed: {cause}; recovery errors: {errors:?}")]
    RollbackFailed { cause: String, errors: Vec<String> },
}

/// Every hash is SHA-256 of observed bytes, independent of a catalog's legacy MD5.
#[derive(Debug, Clone)]
pub struct PreparedFile {
    pub target: PathBuf,
    pub staged: PathBuf,
    pub backup: PathBuf,
    pub previous_sha256: String,
    pub expected_sha256: String,
    pub expected_version: Option<String>,
}

pub fn validate_prepared_plan(
    plan: &UpdatePlan,
    files: &[PreparedFile],
) -> Result<(), ExecutionError> {
    if plan.items.len() != files.len() {
        return Err(ExecutionError::Stale(
            "prepared membership differs from plan".into(),
        ));
    }
    for file in files {
        let item = plan
            .items
            .iter()
            .find(|item| Path::new(&item.dll_path) == file.target)
            .ok_or_else(|| ExecutionError::Stale("prepared target absent from plan".into()))?;
        let change = plan
            .changes
            .iter()
            .find(|change| change.precondition.absolute_path == item.dll_path)
            .ok_or_else(|| ExecutionError::Stale("prepared precondition absent".into()))?;
        let algorithm = match change.artifact.hash.algorithm {
            dlssync_contracts::HashAlgorithm::Sha256 => dll_catalog::HashAlgo::Sha256,
            dlssync_contracts::HashAlgorithm::Md5 => dll_catalog::HashAlgo::Md5,
        };
        let observed = dll_catalog::hash_file_with(&file.staged, algorithm)?;
        if !observed.eq_ignore_ascii_case(&change.artifact.hash.digest)
            || !file
                .previous_sha256
                .eq_ignore_ascii_case(&change.precondition.observed_hash.digest)
            || Path::new(&item.backup_path) != file.backup
            // Enforce version equality only when the catalog attests a real PE file version.
            // Legacy catalog entries carry a package label in `package_version` and leave
            // `file_version` empty; the mandatory artifact hash check above still applies.
            || change
                .artifact
                .file_version
                .as_deref()
                .is_some_and(|expected| file.expected_version.as_deref() != Some(expected))
        {
            return Err(ExecutionError::Integrity(format!(
                "prepared artifact differs from verified plan: {}",
                file.target.display()
            )));
        }
    }
    Ok(())
}

trait FileOperations {
    fn hash(&mut self, path: &Path) -> std::io::Result<String>;
    fn replace(&mut self, source: &Path, target: &Path) -> std::io::Result<()>;
}

struct NativeFiles;

impl FileOperations for NativeFiles {
    fn hash(&mut self, path: &Path) -> std::io::Result<String> {
        dll_catalog::hex_sha256_file(path).map_err(std::io::Error::other)
    }

    fn replace(&mut self, source: &Path, target: &Path) -> std::io::Result<()> {
        replace_atomic(source, target)
    }
}

fn verify_hash(
    files: &mut impl FileOperations,
    path: &Path,
    expected: &str,
) -> Result<(), ExecutionError> {
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ExecutionError::Integrity(format!(
            "missing or invalid SHA-256 for {}",
            path.display()
        )));
    }
    let actual = files.hash(path)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(ExecutionError::Integrity(format!(
            "SHA-256 mismatch for {}: expected {expected}, observed {actual}",
            path.display()
        )));
    }
    Ok(())
}

/// Restore only a verified backup, then read the destination before reporting success.
/// A missing destination can be recovered, but a symlink is never a restore target.
pub fn restore_verified_backup(
    backup: &Path,
    target: &Path,
    expected_sha256: &str,
) -> Result<(), ExecutionError> {
    restore_with(&mut NativeFiles, backup, target, expected_sha256)
}

pub fn create_verified_backup(
    source: &Path,
    backup: &Path,
    expected_sha256: &str,
) -> Result<(), ExecutionError> {
    verify_hash(&mut NativeFiles, source, expected_sha256)?;
    copy_and_sync(source, backup)?;
    verify_hash(&mut NativeFiles, backup, expected_sha256)?;
    verify_hash(&mut NativeFiles, source, expected_sha256)
}

fn restore_with(
    files: &mut impl FileOperations,
    backup: &Path,
    target: &Path,
    expected_sha256: &str,
) -> Result<(), ExecutionError> {
    if !target
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
    {
        return Err(ExecutionError::UnsafeTarget(target.display().to_string()));
    }
    if is_system_target(target) {
        return Err(ExecutionError::UnsafeTarget(target.display().to_string()));
    }
    match std::fs::symlink_metadata(target) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(ExecutionError::UnsafeTarget(target.display().to_string()));
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    verify_hash(files, backup, expected_sha256)?;
    // An atomic write that failed before mutation may leave an already-correct
    // but locked destination. Its hash is sufficient proof; do not rewrite it.
    if files
        .hash(target)
        .is_ok_and(|actual| actual.eq_ignore_ascii_case(expected_sha256))
    {
        return Ok(());
    }
    files.replace(backup, target)?;
    verify_hash(files, target, expected_sha256)
}

/// One write/recovery boundary shared by adapters. Preparation must finish first.
/// The callback rechecks platform identity immediately before each replacement.
pub fn apply_prepared_files(
    prepared: &[PreparedFile],
    validate: impl FnMut(&PreparedFile) -> Result<(), ExecutionError>,
) -> Result<usize, ExecutionError> {
    apply_prepared_with(&mut NativeFiles, prepared, validate)
}

fn apply_prepared_with(
    files: &mut impl FileOperations,
    prepared: &[PreparedFile],
    validate: impl FnMut(&PreparedFile) -> Result<(), ExecutionError>,
) -> Result<usize, ExecutionError> {
    apply_transaction_with(files, prepared, validate, |_, _| Ok(()), |_, _| Ok(()))
}

pub(crate) fn apply_prepared_observed_with_publication(
    prepared: &[PreparedFile],
    validate: impl FnMut(&PreparedFile) -> Result<(), ExecutionError>,
    transition: impl FnMut(usize, dlssync_contracts::OperationStage) -> Result<(), ExecutionError>,
    publication: impl FnMut(usize, dlssync_contracts::OperationStage) -> Result<(), String>,
) -> Result<usize, ExecutionError> {
    apply_transaction_with(
        &mut NativeFiles,
        prepared,
        validate,
        transition,
        publication,
    )
}

fn apply_transaction_with(
    files: &mut impl FileOperations,
    prepared: &[PreparedFile],
    mut validate: impl FnMut(&PreparedFile) -> Result<(), ExecutionError>,
    mut transition: impl FnMut(usize, dlssync_contracts::OperationStage) -> Result<(), ExecutionError>,
    mut publication: impl FnMut(usize, dlssync_contracts::OperationStage) -> Result<(), String>,
) -> Result<usize, ExecutionError> {
    let mut targets = std::collections::HashSet::new();
    for file in prepared {
        let target = std::fs::canonicalize(checked_target(&file.target.to_string_lossy())?)?;
        if !targets.insert(target.clone())
            || target == std::fs::canonicalize(&file.backup)?
            || target == std::fs::canonicalize(&file.staged)?
        {
            return Err(ExecutionError::UnsafeTarget(target.display().to_string()));
        }
        verify_hash(files, &file.target, &file.previous_sha256)?;
        verify_hash(files, &file.backup, &file.previous_sha256)?;
        verify_hash(files, &file.staged, &file.expected_sha256)?;
        validate(file)?;
    }
    for (index, file) in prepared.iter().enumerate() {
        // A precondition failure must restore prior members, but not overwrite
        // the current member's externally changed bytes.
        let preflight = (|| {
            checked_target(&file.target.to_string_lossy())?;
            verify_hash(files, &file.target, &file.previous_sha256)?;
            verify_hash(files, &file.backup, &file.previous_sha256)?;
            verify_hash(files, &file.staged, &file.expected_sha256)?;
            validate(file)?;
            transition(index, dlssync_contracts::OperationStage::Applying)?;
            let _ = publication(index, dlssync_contracts::OperationStage::Applying);
            Ok(())
        })();
        if let Err(error) = preflight {
            return Err(recover_after_failure(files, &prepared[..index], error));
        }
        // Include the current member even if replace reports an error: an I/O
        // failure can occur after mutation. Never use `?` outside this boundary.
        let result = files
            .replace(&file.staged, &file.target)
            .map_err(ExecutionError::Io)
            .and_then(|()| verify_hash(files, &file.target, &file.expected_sha256))
            .and_then(|()| {
                if let Some(expected) = &file.expected_version {
                    let observed = pe_version::read_dll_version(&file.target).map_err(|error| {
                        ExecutionError::Integrity(format!("installed version unreadable: {error}"))
                    })?;
                    if observed.file_version != *expected {
                        return Err(ExecutionError::Integrity(format!(
                            "installed version mismatch: expected {expected}, observed {}",
                            observed.file_version
                        )));
                    }
                }
                Ok(())
            })
            .and_then(|()| {
                transition(index, dlssync_contracts::OperationStage::VerifyingInstalled)?;
                let _ = publication(index, dlssync_contracts::OperationStage::VerifyingInstalled);
                Ok(())
            });
        if let Err(error) = result {
            return Err(recover_after_failure(files, &prepared[..=index], error));
        }
    }
    Ok(prepared.len())
}

fn recover_after_failure(
    files: &mut impl FileOperations,
    touched: &[PreparedFile],
    cause: ExecutionError,
) -> ExecutionError {
    if touched.is_empty() {
        return cause;
    }
    let errors: Vec<_> = touched
        .iter()
        .rev()
        .filter_map(|file| {
            restore_with(files, &file.backup, &file.target, &file.previous_sha256)
                .err()
                .map(|error| error.to_string())
        })
        .collect();
    if errors.is_empty() {
        ExecutionError::RolledBack {
            cause: cause.to_string(),
        }
    } else {
        ExecutionError::RollbackFailed {
            cause: cause.to_string(),
            errors,
        }
    }
}

pub async fn apply_update_plan(
    catalog: &Catalog,
    plan: &UpdatePlan,
    client: &reqwest::Client,
    backups: &BackupStore,
    policy: &crate::policy::ApplyPolicy,
) -> Result<ApplyPlanResult, ExecutionError> {
    crate::validate_update_plan(catalog, plan)?;
    crate::policy::evaluate_plan(plan, policy)?;
    crate::policy::validate_plan_backups(&backups.root_dir, plan)?;
    let preparation_games = crate::transaction::plan_game_roots(plan)?;
    let _preparation_lock = crate::transaction::lock_preparation(&preparation_games)?;
    if plan.stale || plan.catalog_generated_at != catalog.generated_at.to_rfc3339() {
        return Err(ExecutionError::Stale(plan.id.clone()));
    }

    let selected: Vec<_> = plan.items.iter().filter(|item| item.selected).collect();
    let staging = tempfile::tempdir_in(&backups.root_dir)?;
    let mut prepared = Vec::with_capacity(selected.len());
    for (index, item) in selected.into_iter().enumerate() {
        let target = checked_target(&item.dll_path)?;
        verify_sibling_executable(&target)?;
        let vendor = family_vendor(&item.family);
        let release = crate::planning::resolve_planned_release(catalog, item)?;
        let item_staging = staging.path().join(index.to_string());
        std::fs::create_dir(&item_staging)?;
        let staged = {
            let _download_slot = crate::transaction::acquire_download_slot().await;
            dll_catalog::download_and_extract_dll(client, &release, &item_staging).await?
        };
        pe_version::require_x64_dll_pair(&target, &staged)
            .map_err(|error| ExecutionError::Architecture(error.to_string()))?;
        verify_publisher(&staged, vendor)?;
        let expected_sha256 = dll_catalog::hex_sha256_file(&staged)?;
        prepared.push((item, target, staged, expected_sha256));
    }

    let now = chrono::Utc::now();
    let mut backup_entries = Vec::with_capacity(prepared.len());
    for (item, target, _, _) in &prepared {
        let filename = target
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| ExecutionError::UnsafeTarget(item.dll_path.clone()))?;
        let backup_path = crate::policy::derived_backup_destination(&backups.root_dir, plan, item)?;
        dll_catalog::ensure_available_space(&backups.root_dir, std::fs::metadata(target)?.len())?;
        let previous_sha256 = item.trust.observed_sha256.as_deref().unwrap_or_default();
        crate::policy::create_contained_backup(
            &backups.root_dir,
            target,
            &backup_path,
            previous_sha256,
        )?;
        let entry = BackupEntry {
            id: uuid::Uuid::new_v4().to_string(),
            game_id: item.game_id.clone(),
            dll_family: item.family.clone(),
            dll_filename: filename.into(),
            original_path: target.clone(),
            backup_path,
            previous_version: item.current_version.clone(),
            previous_sha256: item.trust.observed_sha256.clone(),
            created_at: now,
            restored_at: None,
            size_bytes: std::fs::metadata(target).ok().map(|value| value.len()),
            backup_type: "dll".into(),
            device_class: None,
            hardware_id: None,
            driver_provider: None,
        };
        backups.insert(&entry)?;
        backup_entries.push(entry);
    }

    let files: Vec<_> = prepared
        .iter()
        .zip(&backup_entries)
        .map(
            |((_, target, staged, expected_sha256), backup)| PreparedFile {
                target: target.clone(),
                staged: staged.clone(),
                backup: backup.backup_path.clone(),
                previous_sha256: backup.previous_sha256.clone().unwrap_or_default(),
                expected_sha256: expected_sha256.clone(),
                expected_version: pe_version::read_dll_version(staged)
                    .ok()
                    .map(|version| version.file_version),
            },
        )
        .collect();
    crate::validate_update_plan(catalog, plan)?;
    let journal = crate::transaction::recovery_journal(backups)?;
    validate_prepared_plan(plan, &files)?;
    let replaced = crate::transaction::apply_durable(
        &journal,
        &backups.root_dir,
        &plan.id,
        dlssync_contracts::OperationActor::Cli,
        &files,
        |file| {
            pe_version::require_x64_dll_pair(&file.target, &file.staged)?;
            verify_sibling_executable(&file.target)?;
            dll_catalog::ensure_available_space(
                &file.target,
                std::fs::metadata(&file.staged)?.len(),
            )?;
            Ok(())
        },
    )?;

    Ok(ApplyPlanResult {
        plan_id: plan.id.clone(),
        applied: replaced as u32,
        backup_paths: backup_entries
            .iter()
            .map(|entry| entry.backup_path.display().to_string())
            .collect(),
    })
}

pub fn rollback_update_plan(plan: &UpdatePlan) -> Result<RollbackPlanResult, ExecutionError> {
    let mut restored = 0;
    let mut errors = Vec::new();
    for item in plan.items.iter().filter(|item| item.selected).rev() {
        match restore_verified_backup(
            Path::new(&item.backup_path),
            Path::new(&item.dll_path),
            item.trust.observed_sha256.as_deref().unwrap_or_default(),
        ) {
            Ok(()) => restored += 1,
            Err(error) => errors.push(format!("{}: {error}", item.id)),
        }
    }
    if !errors.is_empty() {
        return Err(ExecutionError::RollbackFailed {
            cause: plan.id.clone(),
            errors,
        });
    }
    Ok(RollbackPlanResult {
        plan_id: plan.id.clone(),
        restored,
    })
}

fn checked_target(value: &str) -> Result<PathBuf, ExecutionError> {
    let path = PathBuf::from(value);
    if !path.is_file()
        || !path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("dll"))
        || std::fs::symlink_metadata(&path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(true)
    {
        return Err(ExecutionError::UnsafeTarget(path.display().to_string()));
    }
    if is_system_target(&path) {
        return Err(ExecutionError::UnsafeTarget(path.display().to_string()));
    }
    Ok(path)
}

fn is_system_target(path: &Path) -> bool {
    let Some(windows) = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot"))
    else {
        return false;
    };
    let normalize = |path: &Path| {
        path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .trim_start_matches("\\\\?\\")
            .replace('/', "\\")
            .to_ascii_lowercase()
    };
    let target = normalize(path);
    let windows = normalize(Path::new(&windows));
    target == windows || target.starts_with(&(windows + "\\"))
}

fn verify_sibling_executable(target: &Path) -> Result<(), ExecutionError> {
    let Some(parent) = target.parent() else {
        return Ok(());
    };
    let executables: Vec<_> = std::fs::read_dir(parent)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        })
        .take(2)
        .collect();
    // Multiple executables require the explicit game resolver in a verified plan.
    if executables.len() == 1 {
        pe_version::require_x64_executable(&executables[0])
            .map_err(|error| ExecutionError::Architecture(error.to_string()))?;
    }
    Ok(())
}

fn verify_publisher(path: &Path, vendor: &str) -> Result<(), ExecutionError> {
    let info = pe_version::read_authenticode(path)
        .ok_or_else(|| ExecutionError::Authenticode("signature is missing".into()))?;
    pe_version::enforce_subject(&info, vendor).map_err(ExecutionError::Authenticode)?;
    if !info.trusted {
        return Err(ExecutionError::Authenticode(
            "the Authenticode chain is not trusted".into(),
        ));
    }
    Ok(())
}

fn copy_and_sync(source: &Path, destination: &Path) -> std::io::Result<()> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut input = std::fs::File::open(source)?;
    std::io::copy(&mut input, &mut output)?;
    output.sync_all()
}

fn replace_atomic(source: &Path, destination: &Path) -> std::io::Result<()> {
    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "target has no parent")
    })?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    let mut input = std::fs::File::open(source)?;
    std::io::copy(&mut input, &mut staged)?;
    staged.as_file().sync_all()?;
    staged.persist(destination).map_err(|error| error.error)?;
    Ok(())
}

fn family_vendor(family: &str) -> &'static str {
    dll_scanner::family_vendor(family).unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dlssync_contracts::{TrustEvidence, UpdatePlanItem};
    use tempfile::tempdir;

    fn prepared_fixture(dir: &Path, name: &str) -> PreparedFile {
        let folder = dir.join(name);
        std::fs::create_dir(&folder).unwrap();
        let target = folder.join("same.dll");
        let staged = folder.join("staged.dll");
        let backup = folder.join("backup.dll");
        std::fs::write(&target, format!("old-{name}")).unwrap();
        std::fs::write(&backup, format!("old-{name}")).unwrap();
        std::fs::write(&staged, format!("new-{name}")).unwrap();
        PreparedFile {
            expected_version: None,
            previous_sha256: dll_catalog::hex_sha256_file(&target).unwrap(),
            expected_sha256: dll_catalog::hex_sha256_file(&staged).unwrap(),
            target,
            staged,
            backup,
        }
    }

    #[derive(Default)]
    struct FaultFiles {
        writes: usize,
        fail_post_read: Option<PathBuf>,
        fail_write: Option<PathBuf>,
        fail_restore: Option<PathBuf>,
        corrupt_restore: bool,
    }

    impl FileOperations for FaultFiles {
        fn hash(&mut self, path: &Path) -> std::io::Result<String> {
            if self.writes > 0 && self.fail_post_read.as_deref() == Some(path) {
                self.fail_post_read = None;
                return Err(std::io::Error::other("injected post-write read failure"));
            }
            NativeFiles.hash(path)
        }

        fn replace(&mut self, source: &Path, target: &Path) -> std::io::Result<()> {
            let restoring = source.file_name().unwrap() == "backup.dll";
            if restoring && self.fail_restore.as_deref() == Some(target) {
                return Err(std::io::Error::other("injected recovery write failure"));
            }
            NativeFiles.replace(source, target)?;
            self.writes += 1;
            if !restoring && self.fail_write.as_deref() == Some(target) {
                return Err(std::io::Error::other("injected failure after replacement"));
            }
            if restoring && self.corrupt_restore {
                std::fs::write(target, b"corrupt-restore")?;
            }
            Ok(())
        }
    }

    fn assert_original(file: &PreparedFile) {
        assert_eq!(
            dll_catalog::hex_sha256_file(&file.target).unwrap(),
            file.previous_sha256
        );
    }

    #[test]
    fn unreadable_hash_after_second_write_restores_entire_set() {
        let dir = tempdir().unwrap();
        let files = vec![
            prepared_fixture(dir.path(), "a"),
            prepared_fixture(dir.path(), "b"),
        ];
        let mut ops = FaultFiles {
            fail_post_read: Some(files[1].target.clone()),
            ..Default::default()
        };
        let error = apply_prepared_with(&mut ops, &files, |_| Ok(())).unwrap_err();
        assert!(matches!(error, ExecutionError::RolledBack { .. }));
        files.iter().for_each(assert_original);
    }

    #[test]
    fn failed_write_that_changed_bytes_restores_current_and_previous_members() {
        let dir = tempdir().unwrap();
        let files = vec![
            prepared_fixture(dir.path(), "a"),
            prepared_fixture(dir.path(), "b"),
        ];
        let mut ops = FaultFiles {
            fail_write: Some(files[1].target.clone()),
            ..Default::default()
        };
        assert!(matches!(
            apply_prepared_with(&mut ops, &files, |_| Ok(())),
            Err(ExecutionError::RolledBack { .. })
        ));
        files.iter().for_each(assert_original);
    }

    #[test]
    fn recovery_failure_does_not_skip_other_members_or_discard_backups() {
        let dir = tempdir().unwrap();
        let files = vec![
            prepared_fixture(dir.path(), "a"),
            prepared_fixture(dir.path(), "b"),
        ];
        let mut ops = FaultFiles {
            fail_write: Some(files[1].target.clone()),
            fail_restore: Some(files[1].target.clone()),
            ..Default::default()
        };
        let error = apply_prepared_with(&mut ops, &files, |_| Ok(())).unwrap_err();
        assert!(
            matches!(error, ExecutionError::RollbackFailed { ref errors, .. } if errors.len() == 1)
        );
        assert_original(&files[0]);
        assert_eq!(
            dll_catalog::hex_sha256_file(&files[1].target).unwrap(),
            files[1].expected_sha256
        );
        for file in &files {
            assert_eq!(
                dll_catalog::hex_sha256_file(&file.backup).unwrap(),
                file.previous_sha256
            );
        }
    }

    #[test]
    fn successful_restore_write_with_wrong_bytes_is_rollback_failed() {
        let dir = tempdir().unwrap();
        let file = prepared_fixture(dir.path(), "a");
        let mut ops = FaultFiles {
            fail_write: Some(file.target.clone()),
            corrupt_restore: true,
            ..Default::default()
        };
        assert!(matches!(
            apply_prepared_with(&mut ops, &[file], |_| Ok(())),
            Err(ExecutionError::RollbackFailed { .. })
        ));
    }

    #[test]
    fn second_member_precondition_failure_restores_first_without_overwriting_external_change() {
        let dir = tempdir().unwrap();
        let files = vec![
            prepared_fixture(dir.path(), "a"),
            prepared_fixture(dir.path(), "b"),
        ];
        let mut validations = 0;
        let error = apply_prepared_files(&files, |_| {
            validations += 1;
            if validations == 3 {
                std::fs::write(&files[1].target, b"external-change")?;
            }
            Ok(())
        })
        .unwrap_err();
        assert!(matches!(error, ExecutionError::RolledBack { .. }));
        assert_original(&files[0]);
        assert_eq!(std::fs::read(&files[1].target).unwrap(), b"external-change");
    }

    #[test]
    fn invalid_second_backup_rejects_set_before_first_write() {
        let dir = tempdir().unwrap();
        let files = vec![
            prepared_fixture(dir.path(), "a"),
            prepared_fixture(dir.path(), "b"),
        ];
        std::fs::write(&files[1].backup, b"corrupt").unwrap();
        assert!(apply_prepared_files(&files, |_| Ok(())).is_err());
        files.iter().for_each(assert_original);
    }

    #[test]
    fn same_basename_in_different_directories_has_independent_bytes() {
        let dir = tempdir().unwrap();
        let files = vec![
            prepared_fixture(dir.path(), "a"),
            prepared_fixture(dir.path(), "b"),
        ];
        assert_eq!(apply_prepared_files(&files, |_| Ok(())).unwrap(), 2);
        for file in &files {
            assert_eq!(
                dll_catalog::hex_sha256_file(&file.target).unwrap(),
                file.expected_sha256
            );
        }
    }

    #[test]
    fn phase4_publication_failure_does_not_rollback_valid_writes() {
        let dir = tempdir().unwrap();
        let file = prepared_fixture(dir.path(), "publication-isolation");
        let expected = file.expected_sha256.clone();
        let mut publications = 0;
        let result = apply_transaction_with(
            &mut NativeFiles,
            std::slice::from_ref(&file),
            |_| Ok(()),
            |_, _| Ok(()),
            |_, _| {
                publications += 1;
                Err("forced state:event delivery failure".to_string())
            },
        );

        assert_eq!(result.unwrap(), 1);
        assert_eq!(publications, 2);
        assert_eq!(
            dll_catalog::hex_sha256_file(&file.target).unwrap(),
            expected
        );
        assert_ne!(
            dll_catalog::hex_sha256_file(&file.target).unwrap(),
            file.previous_sha256
        );
    }

    #[test]
    fn backup_copy_never_overwrites_existing_baseline() {
        let dir = tempdir().unwrap();
        let file = prepared_fixture(dir.path(), "a");
        assert_eq!(
            copy_and_sync(&file.staged, &file.backup)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_original(&file);
        assert_eq!(
            dll_catalog::hex_sha256_file(&file.backup).unwrap(),
            file.previous_sha256
        );
    }

    #[test]
    fn recovery_can_recreate_missing_destination_from_verified_backup() {
        let dir = tempdir().unwrap();
        let file = prepared_fixture(dir.path(), "a");
        std::fs::remove_file(&file.target).unwrap();
        restore_verified_backup(&file.backup, &file.target, &file.previous_sha256).unwrap();
        assert_original(&file);
    }

    #[test]
    fn rollback_restores_backup_bytes_atomically() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("game.dll");
        let backup = dir.path().join("backup.dll");
        std::fs::write(&target, b"new").unwrap();
        std::fs::write(&backup, b"old").unwrap();
        let plan = UpdatePlan {
            schema_version: 0,
            catalog_revision: String::new(),
            changes: Vec::new(),
            id: "plan".into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            catalog_generated_at: chrono::Utc::now().to_rfc3339(),
            fingerprint: "fingerprint".into(),
            stale: false,
            items: vec![UpdatePlanItem {
                id: "item".into(),
                game_id: "game".into(),
                game_name: "Game".into(),
                dll_path: target.display().to_string(),
                family: "dlss_sr".into(),
                current_version: None,
                target_version: "1".into(),
                backup_path: backup.display().to_string(),
                selected: true,
                trust: TrustEvidence {
                    source_url: String::new(),
                    expected_sha256: String::new(),
                    observed_sha256: Some(dll_catalog::hex_sha256_file(&backup).unwrap()),
                    signature_subject: None,
                    signature_verified: false,
                    anti_cheat_risk: None,
                },
            }],
        };
        let result = rollback_update_plan(&plan).unwrap();
        assert_eq!(result.restored, 1);
        assert_eq!(std::fs::read(target).unwrap(), b"old");
    }

    #[tokio::test]
    async fn stale_plan_is_rejected_before_network_or_backup_mutation() {
        let dir = tempdir().unwrap();
        let catalog = dll_catalog::embedded_fallback_catalog().unwrap();
        let plan = UpdatePlan {
            schema_version: 0,
            catalog_revision: String::new(),
            changes: Vec::new(),
            id: "stale-plan".into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            catalog_generated_at: "2000-01-01T00:00:00+00:00".into(),
            fingerprint: String::new(),
            stale: false,
            items: Vec::new(),
        };
        let backups =
            BackupStore::open(dir.path().join("backups.db"), dir.path().join("files")).unwrap();
        let error = apply_update_plan(
            &catalog,
            &plan,
            &reqwest::Client::new(),
            &backups,
            &crate::policy::ApplyPolicy::default(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, ExecutionError::Stale(_)));
        assert!(backups.list().unwrap().is_empty());
    }
}
