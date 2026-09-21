use backup_store::BackupStore;
use dll_catalog::Catalog;
use dlssync_application::policy::{self, ApplyPolicy};
use dlssync_application::{apply_update_plan, build_verified_update_plan, plan_items, scan_path};
use dlssync_contracts::UpdatePlan;
use sha2::{Digest, Sha256};
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn x64_dll(path: &Path) -> std::io::Result<()> {
    let mut bytes = vec![0; 128];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
    bytes[64..68].copy_from_slice(b"PE\0\0");
    bytes[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
    bytes[84..86].copy_from_slice(&2u16.to_le_bytes());
    bytes[86..88].copy_from_slice(&0x2000u16.to_le_bytes());
    bytes[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
    std::fs::write(path, bytes)
}

fn nvidia_plan(
    root: &Path,
    backups: &Path,
) -> Result<(Catalog, UpdatePlan), Box<dyn std::error::Error>> {
    x64_dll(&root.join("nvngx_dlss.dll"))?;
    let catalog = dll_catalog::embedded_fallback_catalog()?;
    let games = [scan_path(root)?];
    let items = plan_items(&catalog, &games, backups, None);
    let plan = build_verified_update_plan(&catalog, &games, items, backups)?;
    assert_eq!(plan.items.len(), 1);
    Ok((catalog, plan))
}

#[tokio::test]
async fn cli_parity_rejects_expanded_non_rdna4_fsr4_through_shared_policy() -> TestResult {
    let root = tempfile::tempdir()?;
    let store = BackupStore::open(root.path().join("db"), root.path().join("backups"))?;
    let mut catalog = dll_catalog::embedded_fallback_catalog()?;
    catalog.vendors.retain(|vendor, _| vendor == "amd");
    let families = catalog
        .vendors
        .get_mut("amd")
        .ok_or("missing AMD catalog")?;
    families.retain(|family, _| matches!(family.as_str(), "fsr_loader" | "fsr_upscaler"));
    for (family, entry) in families {
        entry.releases.truncate(1);
        let release = entry.releases.first_mut().ok_or("missing FSR release")?;
        release.version = if family == "fsr_loader" {
            "2.2.0"
        } else {
            "4.1.0"
        }
        .into();
        release.cdn_url = "http://127.0.0.1:1/must-not-download.zip".into();
        release.artifact = None;
        x64_dll(&root.path().join(&release.filename))?;
    }
    let games = [scan_path(root.path())?];
    let requested = plan_items(&catalog, &games, &store.root_dir, None)
        .into_iter()
        .filter(|item| item.family == "fsr_loader")
        .collect();
    let plan = build_verified_update_plan(&catalog, &games, requested, &store.root_dir)?;
    assert!(plan
        .changes
        .iter()
        .any(|change| change.added_as_dependency && change.artifact.family == "fsr_upscaler"));
    dlssync_application::validate_update_plan(&catalog, &plan)?;
    let policy = ApplyPolicy::default();
    let shared = policy::evaluate_plan(&plan, &policy)
        .err()
        .ok_or("shared gate accepted FSR4")?;
    let error = apply_update_plan(&catalog, &plan, &reqwest::Client::new(), &store, &policy)
        .await
        .err()
        .ok_or("CLI accepted FSR4")?;
    assert_eq!(error.to_string(), shared.to_string());
    assert!(error.to_string().contains("RDNA4"));
    assert!(store.list()?.is_empty());
    assert_eq!(std::fs::read_dir(&store.root_dir)?.count(), 0);
    Ok(())
}

#[tokio::test]
async fn cli_parity_rejects_backup_escape_before_preparation() -> TestResult {
    let root = tempfile::tempdir()?;
    let store = BackupStore::open(root.path().join("db"), root.path().join("backups"))?;
    let (catalog, original) = nvidia_plan(root.path(), &store.root_dir)?;
    for escape in [
        root.path().join("evil.dll"),
        store.root_dir.join("../evil.dll"),
    ] {
        let mut plan = original.clone();
        plan.items[0].backup_path = escape.to_string_lossy().into_owned();
        plan.fingerprint.clear();
        plan.fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&plan)?));
        dlssync_application::validate_update_plan(&catalog, &plan)?;
        let shared = policy::validate_plan_backups(&store.root_dir, &plan)
            .err()
            .ok_or("shared gate accepted escape")?;
        let error = apply_update_plan(
            &catalog,
            &plan,
            &reqwest::Client::new(),
            &store,
            &ApplyPolicy::default(),
        )
        .await
        .err()
        .ok_or("CLI accepted backup escape")?;
        assert_eq!(error.to_string(), shared.to_string());
        assert!(!escape.exists());
        assert!(store.list()?.is_empty());
        assert_eq!(std::fs::read_dir(&store.root_dir)?.count(), 0);
    }
    Ok(())
}

#[test]
fn cli_parity_derived_backup_matches_legacy_planner_formula() -> TestResult {
    let root = tempfile::tempdir()?;
    let backup_root = root.path().join("backups");
    std::fs::create_dir(&backup_root)?;
    let (catalog, plan) = nvidia_plan(root.path(), &backup_root)?;
    dlssync_application::validate_update_plan(&catalog, &plan)?;
    for item in &plan.items {
        let change = plan
            .changes
            .iter()
            .find(|change| change.precondition.absolute_path == item.dll_path)
            .ok_or("missing identity")?;
        let relative = Path::new(&item.dll_path).strip_prefix(root.path().canonicalize()?)?;
        let old = backup_root
            .join(&plan.id)
            .join(hex::encode(Sha256::digest(item.game_id.as_bytes())))
            .join(relative);
        let derived = policy::derived_backup_destination(&backup_root, &plan, item)?;
        assert_eq!(
            derived.as_os_str().as_encoded_bytes(),
            old.as_os_str().as_encoded_bytes()
        );
        assert_eq!(Path::new(&item.backup_path), derived);
        assert!(!change.precondition.identity.relative_path.is_empty());
    }
    Ok(())
}

#[test]
fn cli_parity_backup_constructor_and_validator_match_mixed_separators() -> TestResult {
    let root = tempfile::tempdir()?;
    let backup_root = root.path().join("backups");
    let nested = root.path().join("bin").join("plugins");
    std::fs::create_dir_all(&nested)?;
    std::fs::create_dir(&backup_root)?;
    x64_dll(&nested.join("nvngx_dlss.dll"))?;
    let catalog = dll_catalog::embedded_fallback_catalog()?;
    let games = [scan_path(root.path())?];
    let items = plan_items(&catalog, &games, &backup_root, None);
    let mut plan = build_verified_update_plan(&catalog, &games, items, &backup_root)?;
    assert_eq!(plan.items.len(), 1);
    let relative_path = &plan.changes[0].precondition.identity.relative_path;
    assert!(relative_path.contains('/'));
    assert!(!relative_path.contains('\\'));

    plan.items[0].backup_path.clear();
    let constructed = policy::derived_backup_destination(&backup_root, &plan, &plan.items[0])?;
    plan.items[0].backup_path = constructed.to_string_lossy().into_owned();

    policy::validate_plan_backups(&backup_root, &plan)?;
    assert_eq!(Path::new(&plan.items[0].backup_path), constructed);
    Ok(())
}

#[test]
fn cli_parity_nested_backup_destination_preserves_path_identity() -> TestResult {
    let root = tempfile::tempdir()?;
    let backup_root = root.path().join("backups");
    let nested = root.path().join("bin").join("plugins");
    std::fs::create_dir_all(&nested)?;
    std::fs::create_dir(&backup_root)?;
    x64_dll(&nested.join("nvngx_dlss.dll"))?;
    let catalog = dll_catalog::embedded_fallback_catalog()?;
    let games = [scan_path(root.path())?];
    let items = plan_items(&catalog, &games, &backup_root, None);
    let plan = build_verified_update_plan(&catalog, &games, items, &backup_root)?;
    assert_eq!(plan.items.len(), 1);
    let item = &plan.items[0];
    let relative = Path::new(&item.dll_path).strip_prefix(root.path().canonicalize()?)?;
    let old = backup_root
        .join(&plan.id)
        .join(hex::encode(Sha256::digest(item.game_id.as_bytes())))
        .join(relative);
    let derived = policy::derived_backup_destination(&backup_root, &plan, item)?;
    assert_eq!(
        Path::new(&item.backup_path).as_os_str().as_encoded_bytes(),
        derived.as_os_str().as_encoded_bytes()
    );
    assert_eq!(derived, old);
    // The shared owner joins the slash-normalized bound identity; Windows treats
    // both separator spellings as the same path, including persisted plan checks.
    #[cfg(windows)]
    assert_ne!(
        derived.as_os_str().as_encoded_bytes(),
        old.as_os_str().as_encoded_bytes()
    );
    policy::validate_plan_backups(&backup_root, &plan)?;
    Ok(())
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "downloads an authentic NVIDIA DLSS release and validates Windows Authenticode"]
async fn cli_parity_nvidia_single_file_update_succeeds_with_contained_backup() -> TestResult {
    let root = tempfile::tempdir()?;
    let store = BackupStore::open(root.path().join("db"), root.path().join("backups"))?;
    let (catalog, plan) = nvidia_plan(root.path(), &store.root_dir)?;
    let before = std::fs::read(&plan.items[0].dll_path)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()?;
    let result =
        apply_update_plan(&catalog, &plan, &client, &store, &ApplyPolicy::default()).await?;
    assert_eq!(result.applied, 1);
    assert_eq!(result.backup_paths.len(), 1);
    let backup = Path::new(&result.backup_paths[0]);
    assert!(backup
        .canonicalize()?
        .starts_with(store.root_dir.canonicalize()?));
    assert_eq!(std::fs::read(backup)?, before);
    assert_ne!(std::fs::read(&plan.items[0].dll_path)?, before);
    assert_eq!(store.list()?.len(), 1);
    let release = dlssync_application::planning::resolve_planned_release(&catalog, &plan.items[0])?;
    let algorithm = if release.hash_algorithm == "md5" {
        dll_catalog::HashAlgo::Md5
    } else {
        dll_catalog::HashAlgo::Sha256
    };
    assert!(
        dll_catalog::hash_file_with(Path::new(&plan.items[0].dll_path), algorithm)?
            .eq_ignore_ascii_case(&release.sha256)
    );
    Ok(())
}
