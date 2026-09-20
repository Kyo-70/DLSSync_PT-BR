//! Policy and backup destination checks shared by every apply adapter.
use crate::ExecutionError;
use dlssync_contracts::{ComponentIdentity, UpdatePlan, UpdatePlanItem};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Default)]
pub struct ApplyPolicy {
    pub fsr4_capable: bool,
    pub allow_streamline: bool,
}

/// Evaluate all members, including automatically expanded dependencies, before preparation.
pub fn evaluate_plan(plan: &UpdatePlan, policy: &ApplyPolicy) -> Result<(), ExecutionError> {
    for item in &plan.items {
        if let Some(reason) =
            fsr4_block_reason(policy.fsr4_capable, &item.family, &item.target_version)
        {
            return Err(ExecutionError::UnsafeTarget(reason));
        }
        let filename = Path::new(&item.dll_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if let Some(reason) = streamline_block_reason(
            filename,
            policy.allow_streamline,
            item.current_version.as_deref().and_then(version_major),
            version_major(&item.target_version),
        ) {
            return Err(ExecutionError::UnsafeTarget(reason));
        }
    }
    Ok(())
}

fn version_major(version: &str) -> Option<u16> {
    version.split('.').next()?.parse().ok()
}

pub fn fsr4_block_reason(capable: bool, family: &str, version: &str) -> Option<String> {
    if !capable
        && matches!(family, "fsr_upscaler" | "fsr_upscaler_vk" | "fsr_fg")
        && version_major(version).is_some_and(|major| major >= 4)
    {
        Some("FSR 4 requires an AMD RDNA4 GPU (Radeon RX 9000 series) and none was detected — refusing the set. Pick a 3.1.x FSR release for this PC instead.".into())
    } else {
        None
    }
}

pub fn streamline_block_reason(
    filename: &str,
    allow: bool,
    installed: Option<u16>,
    target: Option<u16>,
) -> Option<String> {
    if !dll_scanner::is_streamline_plugin(filename) {
        return None;
    }
    if !allow {
        return Some(format!("{filename} is an NVIDIA Streamline plugin (sl.*). Enable 'Update NVIDIA Streamline runtime' in Settings → Advanced to update its version-locked set."));
    }
    if let (Some(installed), Some(target)) = (installed, target) {
        if installed != target {
            return Some(format!("{filename} is NVIDIA Streamline v{installed}.x but the update is v{target}.x. The Streamline plug-ins are version-locked; refusing a cross-major update."));
        }
    }
    None
}

fn unsafe_backup(path: &Path, reason: &str) -> ExecutionError {
    ExecutionError::UnsafeTarget(format!("backup destination {}: {reason}", path.display()))
}

/// Resolve every existing ancestor (including Windows junctions), not just a string prefix.
/// Missing suffixes must consist only of normal path components.
pub fn validate_backup_destination(root: &Path, path: &Path) -> Result<(), ExecutionError> {
    let canonical_root = root.canonicalize()?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| unsafe_backup(path, "outside active backup store"))?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(unsafe_backup(path, "non-normal or empty relative path"));
    }
    let mut ancestor = root.to_path_buf();
    for part in relative.components() {
        ancestor.push(part.as_os_str());
        match std::fs::symlink_metadata(&ancestor) {
            Ok(_) => {
                let resolved = ancestor
                    .canonicalize()
                    .map_err(|_| unsafe_backup(&ancestor, "unresolvable existing ancestor"))?;
                if !resolved.starts_with(&canonical_root) {
                    return Err(unsafe_backup(
                        &ancestor,
                        "junction or symlink escapes active backup store",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Construct one copy destination from the normalized identity bound into the plan.
/// This does not read a caller-supplied `backup_path`.
pub fn construct_backup_destination(
    root: &Path,
    plan_id: &str,
    identity: &ComponentIdentity,
) -> Result<PathBuf, ExecutionError> {
    if uuid::Uuid::parse_str(plan_id).is_err() {
        return Err(unsafe_backup(root, "invalid plan UUID"));
    }
    let normalized_relative = identity.relative_path.replace('\\', "/");
    let relative = Path::new(&normalized_relative);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(unsafe_backup(relative, "invalid bound component identity"));
    }
    let path = root
        .join(plan_id)
        .join(hex::encode(Sha256::digest(identity.game_id.as_bytes())))
        .join(relative);
    Ok(path)
}

/// Reconstruct the copy destination from the component identity bound into the plan.
pub fn derived_backup_destination(
    root: &Path,
    plan: &UpdatePlan,
    item: &UpdatePlanItem,
) -> Result<PathBuf, ExecutionError> {
    let change = plan
        .changes
        .iter()
        .find(|change| change.precondition.absolute_path == item.dll_path)
        .ok_or_else(|| unsafe_backup(root, "component identity missing"))?;
    if change.precondition.identity.game_id != item.game_id {
        return Err(unsafe_backup(root, "component identity game mismatch"));
    }
    construct_backup_destination(root, &plan.id, &change.precondition.identity)
}

/// Validate a persisted destination independently from constructing a copy target.
pub fn validate_planned_backup_destination(
    root: &Path,
    plan: &UpdatePlan,
    item: &UpdatePlanItem,
) -> Result<(), ExecutionError> {
    let supplied = Path::new(&item.backup_path);
    validate_backup_destination(root, supplied)?;
    let expected = derived_backup_destination(root, plan, item)?;
    if supplied.as_os_str() != expected.as_os_str() {
        return Err(unsafe_backup(
            supplied,
            "does not match derived destination",
        ));
    }
    Ok(())
}

pub fn validate_plan_backups(root: &Path, plan: &UpdatePlan) -> Result<(), ExecutionError> {
    for item in &plan.items {
        validate_planned_backup_destination(root, plan, item)?;
    }
    Ok(())
}

/// Recheck at the copy boundary, after making the parent directory, as well as at review.
pub fn create_contained_backup(
    root: &Path,
    source: &Path,
    destination: &Path,
    hash: &str,
) -> Result<(), ExecutionError> {
    validate_backup_destination(root, destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| unsafe_backup(destination, "missing parent"))?;
    std::fs::create_dir_all(parent)?;
    validate_backup_destination(root, destination)?;
    crate::execution::create_verified_backup(source, destination, hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forged_plan_backup_rejected_even_with_recomputed_fingerprint(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("backups");
        std::fs::create_dir(&root)?;
        let path = temp.path().join("nvngx_dlss.dll");
        let mut bytes = vec![0; 128];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
        bytes[84..86].copy_from_slice(&2u16.to_le_bytes());
        bytes[86..88].copy_from_slice(&0x2000u16.to_le_bytes());
        bytes[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        std::fs::write(&path, bytes)?;
        let catalog = dll_catalog::embedded_fallback_catalog()?;
        let games = [dlssync_contracts::ScannedGame {
            id: "game".into(),
            name: "Game".into(),
            launcher: "manual".into(),
            install_dir: temp.path().display().to_string(),
            components: vec![dlssync_contracts::ScannedComponent {
                family: "dlss_sr".into(),
                path: path.display().to_string(),
                current_version: None,
                sha256: Some(dll_catalog::hex_sha256_file(&path)?),
            }],
        }];
        let items = crate::plan_items(&catalog, &games, &root, None);
        let original = crate::build_verified_update_plan(&catalog, &games, items, &root)?;
        validate_plan_backups(&root, &original)?;
        for escape in [temp.path().join("evil.dll"), root.join("../evil.dll")] {
            let mut forged = original.clone();
            forged.items[0].backup_path = escape.display().to_string();
            forged.fingerprint.clear();
            forged.fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&forged)?));
            crate::validate_update_plan(&catalog, &forged)?;
            assert!(validate_plan_backups(&root, &forged).is_err());
            assert!(!temp.path().join("evil.dll").exists());
        }
        Ok(())
    }

    #[test]
    fn backup_destination_rejects_outside_and_parent_traversal(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("backups");
        std::fs::create_dir(&root)?;
        assert!(validate_backup_destination(&root, &temp.path().join("evil.dll")).is_err());
        assert!(validate_backup_destination(&root, &root.join("../evil.dll")).is_err());
        assert!(validate_backup_destination(&root, &root.join("safe/game/a.dll")).is_ok());
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn backup_destination_rejects_junction_escape() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("backups");
        let outside = temp.path().join("outside");
        std::fs::create_dir(&root)?;
        std::fs::create_dir(&outside)?;
        let junction = root.join("redirect");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .status()?;
        assert!(status.success());
        assert!(validate_backup_destination(&root, &junction.join("a.dll")).is_err());
        std::fs::remove_dir(junction)?;
        Ok(())
    }

    #[test]
    fn expanded_fsr4_member_rejected_by_shared_policy() -> Result<(), Box<dyn std::error::Error>> {
        let mut plan = crate::build_update_plan("now", vec![]);
        for (family, version) in [("fsr_loader", "2.2.0"), ("fsr_upscaler", "4.1.0")] {
            plan.items.push(serde_json::from_value(serde_json::json!({
                "id": family, "game_id": "game", "game_name": "Game", "dll_path": format!("{family}.dll"),
                "family": family, "target_version": version, "backup_path": "unused", "selected": true,
                "trust": {"source_url": "https://example.test/sdk.zip", "expected_sha256": "a".repeat(64), "signature_verified": false}
            }))?);
        }
        let rejected = evaluate_plan(&plan, &ApplyPolicy::default());
        assert!(rejected.is_err_and(|error| error.to_string().contains("RDNA4")));
        assert!(evaluate_plan(
            &plan,
            &ApplyPolicy {
                fsr4_capable: true,
                allow_streamline: false
            }
        )
        .is_ok());
        Ok(())
    }
}
