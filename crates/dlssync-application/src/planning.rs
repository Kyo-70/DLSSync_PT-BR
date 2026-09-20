//! Plans bind observations to exact catalog artifacts before an executor can write.
use crate::ExecutionError;
use dll_catalog::{Catalog, Release};
use dlssync_contracts::*;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

pub const VERIFIED_PLAN_SCHEMA: u16 = 1;

pub fn catalog_revision(catalog: &Catalog) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(catalog).expect("catalog serializes"),
    ))
}

fn fingerprint(plan: &UpdatePlan) -> String {
    let mut bound = plan.clone();
    bound.fingerprint.clear();
    hex::encode(Sha256::digest(
        serde_json::to_vec(&bound).expect("plan serializes"),
    ))
}

pub fn subset_update_plan(
    plan: &UpdatePlan,
    game_ids: &[String],
) -> Result<UpdatePlan, ExecutionError> {
    if plan.fingerprint != fingerprint(plan) {
        return Err(ExecutionError::Stale("plan fingerprint changed".into()));
    }
    let mut subset = plan.clone();
    subset.items.retain(|item| game_ids.contains(&item.game_id));
    subset
        .changes
        .retain(|change| game_ids.contains(&change.precondition.identity.game_id));
    subset.fingerprint = fingerprint(&subset);
    Ok(subset)
}

pub(crate) fn vendor(family: &str) -> &'static str {
    dll_scanner::family_vendor(family).unwrap_or("unknown")
}

fn release_for_item<'a>(
    catalog: &'a Catalog,
    item: &UpdatePlanItem,
) -> Result<&'a Release, ExecutionError> {
    let filename = Path::new(&item.dll_path)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or_default();
    catalog
        .vendors
        .get(vendor(&item.family))
        .and_then(|families| families.get(&item.family))
        .and_then(|family| {
            family.releases.iter().find(|release| {
                release.filename.eq_ignore_ascii_case(filename)
                    && release.version == item.target_version
                    && release.cdn_url == item.trust.source_url
                    && release
                        .sha256
                        .eq_ignore_ascii_case(&item.trust.expected_sha256)
            })
        })
        .ok_or_else(|| ExecutionError::MissingRelease(item.id.clone()))
}

fn descriptor(catalog: &Catalog, family: &str, release: &Release) -> ArtifactDescriptor {
    release.artifact.clone().unwrap_or_else(|| {
        let package_id = hex::encode(Sha256::digest(release.cdn_url.as_bytes()));
        ArtifactDescriptor {
            id: format!("{package_id}:{}:{}", release.filename, release.sha256),
            family: family.into(),
            filename: release.filename.clone(),
            // Legacy release versions are package labels, not attested PE versions.
            // Only an explicit artifact descriptor can supply a file_version;
            // the catalog artifact hash remains mandatory even without one.
            file_version: None,
            package_version: release.version.clone(),
            package_id,
            compatibility_line: "legacy-unclassified".into(),
            architecture: Architecture::X64,
            hash: ContentHash {
                algorithm: if release.hash_algorithm == "md5" {
                    HashAlgorithm::Md5
                } else {
                    HashAlgorithm::Sha256
                },
                digest: release.sha256.clone(),
            },
            size_bytes: release.size_bytes.into(),
            source_url: release.cdn_url.clone(),
            archive_entry: release.zip_entry.clone(),
            expected_publisher: release.signature_subject.clone(),
            observed_publisher: None,
            signature_status: SignatureStatus::NotChecked,
            dependencies: vec![],
            checked_at: catalog.generated_at.to_rfc3339(),
        }
    })
}

pub fn resolve_planned_release(
    catalog: &Catalog,
    item: &UpdatePlanItem,
) -> Result<Release, ExecutionError> {
    release_for_item(catalog, item).cloned()
}

fn normal_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn coherent_group(filename: &str) -> Option<&'static str> {
    let filename = filename.to_ascii_lowercase();
    if filename.starts_with("sl.") {
        Some("streamline")
    } else if filename.starts_with("dstorage") {
        Some("direct_storage")
    } else if filename.starts_with("amd_fidelityfx") && filename.contains("_vk") {
        Some("fsr_vk")
    } else if filename.starts_with("amd_fidelityfx") {
        Some("fsr_dx12")
    } else if filename.starts_with("libxess") || filename == "libxell.dll" {
        Some("xess")
    } else {
        None
    }
}

fn coherent_dependencies(
    catalog: &Catalog,
    game: &ScannedGame,
    artifact: &ArtifactDescriptor,
    parent: &Path,
) -> Result<Vec<String>, ExecutionError> {
    let mut ids = artifact.dependencies.clone();
    let Some(group) = coherent_group(&artifact.filename) else {
        return Ok(ids);
    };
    let mut names: HashSet<_> = game
        .components
        .iter()
        .filter(|component| {
            Path::new(&component.path)
                .canonicalize()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf))
                .as_deref()
                == Some(parent)
        })
        .filter_map(|component| Path::new(&component.path).file_name()?.to_str())
        .filter(|filename| coherent_group(filename) == Some(group))
        .map(str::to_ascii_lowercase)
        .collect();
    if group == "streamline" {
        names.extend(["sl.common.dll".into(), "sl.interposer.dll".into()]);
    }
    if group == "direct_storage" {
        names.extend(["dstorage.dll".into(), "dstoragecore.dll".into()]);
    }
    if artifact.family == "xess_fg" {
        names.insert("libxell.dll".into());
    }
    for filename in names {
        if filename.eq_ignore_ascii_case(&artifact.filename) {
            continue;
        }
        let candidate = catalog
            .vendors
            .values()
            .flat_map(|families| families.iter())
            .flat_map(|(family, entry)| entry.releases.iter().map(move |release| (family, release)))
            .filter(|(_, release)| release.filename.eq_ignore_ascii_case(&filename))
            .map(|(family, release)| descriptor(catalog, family, release))
            .find(|candidate| candidate.package_id == artifact.package_id)
            .ok_or_else(|| {
                ExecutionError::MissingRelease(format!("coherent package member: {filename}"))
            })?;
        ids.push(candidate.id);
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

fn bound_path(root: &Path, value: &str) -> Result<(PathBuf, PathBuf), ExecutionError> {
    let root = root.canonicalize()?;
    let input = Path::new(value);
    if std::fs::symlink_metadata(input)?.file_type().is_symlink() {
        return Err(ExecutionError::UnsafeTarget(value.into()));
    }
    let absolute = input.canonicalize()?;
    let relative = absolute
        .strip_prefix(root)
        .map_err(|_| ExecutionError::UnsafeTarget(value.into()))?
        .to_path_buf();
    if !normal_relative(&relative) {
        return Err(ExecutionError::UnsafeTarget(value.into()));
    }
    Ok((absolute, relative))
}

/// Dependencies are explicit plan members. A missing required installed member
/// blocks the plan; adding new game files requires a separately reviewed recipe.
pub fn build_verified_update_plan(
    catalog: &Catalog,
    games: &[ScannedGame],
    items: Vec<UpdatePlanItem>,
    backup_root: &Path,
) -> Result<UpdatePlan, ExecutionError> {
    catalog.validate_artifacts()?;
    let mut selected: Vec<_> = items.into_iter().filter(|item| item.selected).collect();
    let requested: HashSet<_> = selected
        .iter()
        .map(|item| (item.game_id.clone(), item.dll_path.clone()))
        .collect();
    let mut index = 0;
    while index < selected.len() {
        let item = selected[index].clone();
        let artifact = descriptor(catalog, &item.family, release_for_item(catalog, &item)?);
        let game = games
            .iter()
            .find(|game| game.id == item.game_id)
            .ok_or_else(|| ExecutionError::Stale(format!("game missing: {}", item.game_id)))?;
        let item_parent = Path::new(&item.dll_path)
            .canonicalize()?
            .parent()
            .unwrap()
            .to_path_buf();
        for dependency in &coherent_dependencies(catalog, game, &artifact, &item_parent)? {
            let (family, release) = catalog
                .vendors
                .values()
                .flat_map(|families| families.iter())
                .flat_map(|(family, entry)| {
                    entry.releases.iter().map(move |release| (family, release))
                })
                .find(|(family, release)| descriptor(catalog, family, release).id == *dependency)
                .ok_or_else(|| ExecutionError::MissingRelease(dependency.clone()))?;
            let component = game
                .components
                .iter()
                .find(|component| {
                    component.family == *family
                        && Path::new(&component.path)
                            .file_name()
                            .is_some_and(|filename| {
                                filename.eq_ignore_ascii_case(&release.filename)
                            })
                        && Path::new(&component.path)
                            .canonicalize()
                            .ok()
                            .and_then(|path| path.parent().map(Path::to_path_buf))
                            == Some(item_parent.clone())
                })
                .ok_or_else(|| {
                    ExecutionError::Stale(format!(
                        "required installed dependency missing: {}",
                        release.filename
                    ))
                })?;
            if let Some(existing) = selected.iter().find(|candidate| {
                candidate.game_id == game.id && candidate.dll_path == component.path
            }) {
                if descriptor(
                    catalog,
                    &existing.family,
                    release_for_item(catalog, existing)?,
                )
                .id != *dependency
                {
                    return Err(ExecutionError::Stale(format!(
                        "conflicting dependency: {}",
                        component.path
                    )));
                }
            } else {
                selected.push(UpdatePlanItem {
                    id: dependency.clone(),
                    game_id: game.id.clone(),
                    game_name: game.name.clone(),
                    dll_path: component.path.clone(),
                    family: family.clone(),
                    current_version: component.current_version.clone(),
                    target_version: release.version.clone(),
                    backup_path: String::new(),
                    selected: true,
                    trust: TrustEvidence {
                        source_url: release.cdn_url.clone(),
                        expected_sha256: release.sha256.clone(),
                        observed_sha256: component.sha256.clone(),
                        signature_subject: release.signature_subject.clone(),
                        signature_verified: release.signed,
                        anti_cheat_risk: None,
                    },
                });
            }
        }
        index += 1;
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let mut changes = Vec::with_capacity(selected.len());
    let mut seen = HashSet::new();
    for item in &mut selected {
        let game = games
            .iter()
            .find(|game| game.id == item.game_id)
            .ok_or_else(|| ExecutionError::Stale(item.game_id.clone()))?;
        let (absolute, relative) = bound_path(Path::new(&game.install_dir), &item.dll_path)?;
        let path_key = absolute.to_string_lossy().to_ascii_lowercase();
        if !seen.insert(path_key) {
            return Err(ExecutionError::UnsafeTarget(format!(
                "duplicate target: {}",
                absolute.display()
            )));
        }
        let identity = pe_version::read_pe_identity(&absolute)?;
        if !identity.is_dll || identity.architecture != Architecture::X64 {
            return Err(ExecutionError::Architecture(absolute.display().to_string()));
        }
        let observed_hash = dll_catalog::hex_sha256_file(&absolute)?;
        if item
            .trust
            .observed_sha256
            .as_ref()
            .is_none_or(|hash| !hash.eq_ignore_ascii_case(&observed_hash))
        {
            return Err(ExecutionError::Stale(format!(
                "file changed since scan: {}",
                absolute.display()
            )));
        }
        let artifact = descriptor(catalog, &item.family, release_for_item(catalog, item)?);
        let added_as_dependency =
            !requested.contains(&(item.game_id.clone(), item.dll_path.clone()));
        let relative_path = relative.to_string_lossy().replace('\\', "/");
        let filename = relative.file_name().unwrap().to_string_lossy().into_owned();
        item.id = format!("{}:{}:{relative_path}", item.game_id, item.family);
        item.dll_path = absolute.to_string_lossy().into_owned();
        let component_identity = ComponentIdentity {
            game_id: item.game_id.clone(),
            relative_path,
            family: item.family.clone(),
            filename,
            architecture: identity.architecture,
            graphics_api: None,
        };
        item.backup_path =
            crate::policy::construct_backup_destination(backup_root, &id, &component_identity)?
                .to_string_lossy()
                .into_owned();
        let observed_version = pe_version::read_dll_version(&absolute)
            .ok()
            .map(|version| version.file_version);
        item.current_version = observed_version.clone();
        let set_id = format!(
            "{}:{}:{}",
            item.game_id,
            relative.parent().unwrap_or(Path::new("")).display(),
            artifact.package_id
        );
        changes.push(PlannedChange {
            precondition: FilePrecondition {
                identity: component_identity,
                absolute_path: item.dll_path.clone(),
                observed_hash: ContentHash {
                    algorithm: HashAlgorithm::Sha256,
                    digest: observed_hash,
                },
                observed_version,
                size_bytes: std::fs::metadata(&absolute)?.len().into(),
                checked_at: now.clone(),
            },
            artifact,
            added_as_dependency,
            set_id,
            compatibility: CompatibilityDecision {
                status: CompatibilityStatus::Unknown,
                reason_code: "hardware_compatibility_not_evaluated".into(),
                evidence: vec![Evidence {
                    source: "local_pe_and_catalog".into(),
                    observed_at: now.clone(),
                    detail: "x64 DLL identity verified; platform compatibility remains separate"
                        .into(),
                }],
            },
        });
    }
    selected.sort_by(|a, b| a.id.cmp(&b.id));
    changes.sort_by(|a, b| {
        a.precondition
            .absolute_path
            .cmp(&b.precondition.absolute_path)
    });
    let mut plan = UpdatePlan {
        schema_version: VERIFIED_PLAN_SCHEMA,
        catalog_revision: catalog_revision(catalog),
        changes,
        id,
        created_at: now,
        catalog_generated_at: catalog.generated_at.to_rfc3339(),
        fingerprint: String::new(),
        stale: false,
        items: selected,
    };
    plan.fingerprint = fingerprint(&plan);
    Ok(plan)
}

/// Runs before network/preparation and again before the write boundary.
pub fn validate_update_plan(catalog: &Catalog, plan: &UpdatePlan) -> Result<(), ExecutionError> {
    let stale = |reason: &str| ExecutionError::Stale(format!("{}: {reason}", plan.id));
    if plan.schema_version != VERIFIED_PLAN_SCHEMA {
        return Err(stale("legacy or unknown schema; rebuild plan"));
    }
    if plan.stale
        || plan.catalog_revision != catalog_revision(catalog)
        || plan.catalog_generated_at != catalog.generated_at.to_rfc3339()
    {
        return Err(stale("catalog revision changed"));
    }
    if plan.fingerprint != fingerprint(plan) {
        return Err(stale("plan fingerprint changed"));
    }
    if plan.items.len() != plan.changes.len() || plan.items.iter().any(|item| !item.selected) {
        return Err(stale("plan membership changed"));
    }
    let mut paths = HashSet::new();
    let mut checked_groups = HashSet::new();
    for item in &plan.items {
        let change = plan
            .changes
            .iter()
            .find(|change| change.precondition.absolute_path == item.dll_path)
            .ok_or_else(|| stale("component precondition missing"))?;
        let path = Path::new(&item.dll_path);
        if let Some(group) = coherent_group(&change.artifact.filename) {
            let parent = path
                .parent()
                .ok_or_else(|| stale("target parent missing"))?;
            if checked_groups.insert((parent.to_path_buf(), group)) {
                for entry in std::fs::read_dir(parent)? {
                    let entry = entry?;
                    let name = entry.file_name();
                    if name.to_str().is_some_and(|name| {
                        name.to_ascii_lowercase().ends_with(".dll")
                            && coherent_group(name) == Some(group)
                    }) {
                        let observed = entry.path().canonicalize()?;
                        if !plan
                            .changes
                            .iter()
                            .any(|member| Path::new(&member.precondition.absolute_path) == observed)
                        {
                            return Err(stale("coherent set inventory changed"));
                        }
                    }
                }
            }
        }
        if path.canonicalize()? != path
            || !paths.insert(item.dll_path.to_ascii_lowercase())
            || std::fs::symlink_metadata(path)?.file_type().is_symlink()
        {
            return Err(stale("target path changed"));
        }
        if change.artifact != descriptor(catalog, &item.family, release_for_item(catalog, item)?) {
            return Err(stale("artifact identity changed"));
        }
        if change.precondition.observed_hash.algorithm != HashAlgorithm::Sha256
            || !change.precondition.observed_hash.is_valid()
            || dll_catalog::hex_sha256_file(path)? != change.precondition.observed_hash.digest
            || std::fs::metadata(path)?.len().to_string() != change.precondition.size_bytes.0
        {
            return Err(stale("installed bytes changed"));
        }
        let pe = pe_version::read_pe_identity(path)?;
        if !pe.is_dll || pe.architecture != change.precondition.identity.architecture {
            return Err(stale("PE identity changed"));
        }
        let version = pe_version::read_dll_version(path)
            .ok()
            .map(|value| value.file_version);
        if version != change.precondition.observed_version {
            return Err(stale("installed version changed"));
        }
        for dependency in &change.artifact.dependencies {
            if !plan
                .changes
                .iter()
                .any(|member| member.set_id == change.set_id && member.artifact.id == *dependency)
            {
                return Err(stale("required dependency missing from set"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pe(path: &Path, marker: u8) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = vec![marker; 128];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
        bytes[84..86].copy_from_slice(&2u16.to_le_bytes());
        bytes[86..88].copy_from_slice(&0x2000u16.to_le_bytes());
        bytes[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        std::fs::write(path, bytes).unwrap();
    }

    fn fixture(root: &Path) -> (Catalog, ScannedGame) {
        let catalog = dll_catalog::embedded_fallback_catalog().unwrap();
        let mut components = vec![];
        for (index, folder) in ["a", "b"].iter().enumerate() {
            let path = root.join(folder).join("nvngx_dlss.dll");
            pe(&path, index as u8);
            components.push(ScannedComponent {
                family: "dlss_sr".into(),
                path: path.to_string_lossy().into_owned(),
                current_version: None,
                sha256: Some(dll_catalog::hex_sha256_file(&path).unwrap()),
            });
        }
        (
            catalog,
            ScannedGame {
                id: "game".into(),
                name: "Game".into(),
                launcher: "manual".into(),
                install_dir: root.to_string_lossy().into_owned(),
                components,
            },
        )
    }

    fn plan(catalog: &Catalog, game: &ScannedGame, backup_root: &Path) -> UpdatePlan {
        let games = std::slice::from_ref(game);
        let items = crate::plan_items(catalog, games, backup_root, None);
        assert_eq!(items.len(), 2);
        build_verified_update_plan(catalog, games, items, backup_root).unwrap()
    }

    #[test]
    fn same_game_preparation_cannot_overlap_across_backup_profiles() {
        let root = tempfile::tempdir().unwrap();
        let (catalog, mut game) = fixture(root.path());
        game.id = "steam:123".into();
        let gui_plan = plan(&catalog, &game, &root.path().join("gui-backups"));
        game.id = "manual-path-id".into();
        let cli_plan = plan(&catalog, &game, &root.path().join("cli-backups"));
        let gui_roots = crate::transaction::plan_game_roots(&gui_plan).unwrap();
        let cli_roots = crate::transaction::plan_game_roots(&cli_plan).unwrap();
        assert_eq!(gui_roots, cli_roots);
        let gui = crate::transaction::lock_preparation(&gui_roots).unwrap();
        assert!(matches!(
            crate::transaction::lock_preparation(&cli_roots),
            Err(ExecutionError::Locked(_))
        ));
        drop(gui);
        assert!(crate::transaction::lock_preparation(&cli_roots).is_ok());
    }

    #[test]
    fn relative_identity_separates_duplicate_basenames_and_backup_paths() {
        let root = tempfile::tempdir().unwrap();
        let (catalog, game) = fixture(root.path());
        let plan = plan(&catalog, &game, &root.path().join("backups"));
        assert_eq!(plan.schema_version, VERIFIED_PLAN_SCHEMA);
        assert_ne!(plan.items[0].id, plan.items[1].id);
        assert_ne!(plan.items[0].backup_path, plan.items[1].backup_path);
        assert_eq!(
            plan.changes[0].precondition.identity.relative_path,
            "a/nvngx_dlss.dll"
        );
        validate_update_plan(&catalog, &plan).unwrap();
    }

    #[test]
    fn catalog_change_with_same_timestamp_invalidates_plan() {
        let root = tempfile::tempdir().unwrap();
        let (mut catalog, game) = fixture(root.path());
        let plan = plan(&catalog, &game, &root.path().join("backups"));
        catalog.incompatible_games.push("changed-game".into());
        assert!(validate_update_plan(&catalog, &plan).is_err());
    }

    #[test]
    fn changed_bytes_after_plan_are_rejected_before_download() {
        let root = tempfile::tempdir().unwrap();
        let (catalog, game) = fixture(root.path());
        let plan = plan(&catalog, &game, &root.path().join("backups"));
        pe(Path::new(&plan.items[0].dll_path), 9);
        assert!(validate_update_plan(&catalog, &plan).is_err());
    }

    #[test]
    fn changed_scan_observation_cannot_be_rebound_silently() {
        let root = tempfile::tempdir().unwrap();
        let (catalog, game) = fixture(root.path());
        let games = [game];
        let items = crate::plan_items(&catalog, &games, root.path(), None);
        pe(Path::new(&items[0].dll_path), 9);
        assert!(build_verified_update_plan(&catalog, &games, items, root.path()).is_err());
    }

    #[test]
    fn selection_path_and_dependency_edits_invalidate_fingerprint() {
        let root = tempfile::tempdir().unwrap();
        let (catalog, game) = fixture(root.path());
        let original = plan(&catalog, &game, &root.path().join("backups"));
        for edit in 0..3 {
            let mut changed = original.clone();
            match edit {
                0 => changed.items[0].selected = false,
                1 => changed.items[0].dll_path = changed.items[1].dll_path.clone(),
                _ => changed.changes[0]
                    .artifact
                    .dependencies
                    .push("new-dependency".into()),
            }
            assert!(validate_update_plan(&catalog, &changed).is_err());
        }
    }

    #[tokio::test]
    async fn legacy_plan_rejected_without_backup_or_network_mutation() {
        let root = tempfile::tempdir().unwrap();
        let (catalog, game) = fixture(root.path());
        let items = crate::plan_items(&catalog, &[game], root.path(), None);
        let legacy =
            crate::build_update_plan_at(&catalog.generated_at.to_rfc3339(), items, root.path());
        let store =
            backup_store::BackupStore::open(root.path().join("db"), root.path().join("backups"))
                .unwrap();
        let error = crate::apply_update_plan(
            &catalog,
            &legacy,
            &reqwest::Client::new(),
            &store,
            &crate::policy::ApplyPolicy::default(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, ExecutionError::Stale(_)));
        assert!(store.list().unwrap().is_empty());
        assert_eq!(std::fs::read_dir(&store.root_dir).unwrap().count(), 0);
    }

    fn storage_fixture(root: &Path) -> (Catalog, ScannedGame) {
        let mut vendors = serde_json::Map::new();
        let mut families = serde_json::Map::new();
        let mut components = Vec::new();
        for (family, filename, version, hash) in [
            ("direct_storage", "dstorage.dll", "2.1.0.0", "a"),
            ("direct_storage_core", "dstoragecore.dll", "1.9.0.0", "b"),
        ] {
            families.insert(family.into(), serde_json::json!({"latest": version, "releases": [{
                "version":version,"version_packed":1,"filename":filename,"sha256":hash.repeat(64),
                "size_bytes":128,"signed":false,"released_at":"2026-09-12T00:00:00Z",
                "source":"fixture","cdn_url":"https://vendor.invalid/storage.zip","hash_algorithm":"sha256"
            }]}));
            let path = root.join(filename);
            pe(&path, 0);
            components.push(ScannedComponent {
                family: family.into(),
                path: path.to_string_lossy().into_owned(),
                current_version: None,
                sha256: Some(dll_catalog::hex_sha256_file(&path).unwrap()),
            });
        }
        vendors.insert("microsoft".into(), families.into());
        let catalog = serde_json::from_value(serde_json::json!({"schema_version":2,"generated_at":"2026-09-12T00:00:00Z","vendors":vendors})).unwrap();
        (
            catalog,
            ScannedGame {
                id: "storage-game".into(),
                name: "Storage Game".into(),
                launcher: "manual".into(),
                install_dir: root.to_string_lossy().into_owned(),
                components,
            },
        )
    }

    #[test]
    fn fsr_scanned_members_resolve_their_dedicated_catalog_families() {
        let root = tempfile::tempdir().unwrap();
        let mut families = serde_json::Map::new();
        for (family, filename, version, package) in [
            (
                "fsr_upscaler",
                "amd_fidelityfx_upscaler_dx12.dll",
                "4.1.1.2740",
                "dx12",
            ),
            (
                "fsr_loader",
                "amd_fidelityfx_loader_dx12.dll",
                "2.3.0.2740",
                "dx12",
            ),
            ("fsr_upscaler_vk", "amd_fidelityfx_vk.dll", "3.1.5.0", "vk"),
        ] {
            pe(&root.path().join(filename), 0);
            families.insert(family.into(), serde_json::json!({"latest":version,"releases":[{
                "version":version,"version_packed":1,"filename":filename,"sha256":"a".repeat(64),
                "size_bytes":128,"signed":false,"released_at":"2026-09-12T00:00:00Z",
                "source":"fixture","cdn_url":format!("https://vendor.invalid/{package}.zip"),"hash_algorithm":"sha256"
            }]}));
        }
        let catalog: Catalog = serde_json::from_value(serde_json::json!({
            "schema_version":2,"generated_at":"2026-09-12T00:00:00Z","vendors":{"amd":families}
        }))
        .unwrap();
        let game = crate::scan_path(root.path()).unwrap();
        let games = [game];
        let items = crate::plan_items(&catalog, &games, &root.path().join("backups"), None);
        assert_eq!(
            items.len(),
            3,
            "every scanned AMD member must resolve its own catalog family"
        );
        let plan =
            build_verified_update_plan(&catalog, &games, items, &root.path().join("backups"))
                .unwrap();
        assert_eq!(plan.changes.len(), 3);
        validate_update_plan(&catalog, &plan).unwrap();
    }

    #[test]
    fn same_package_dependency_is_added_despite_different_internal_versions() {
        let root = tempfile::tempdir().unwrap();
        let (catalog, game) = storage_fixture(root.path());
        let games = [game];
        let items = crate::plan_items(&catalog, &games, root.path(), None)
            .into_iter()
            .filter(|item| item.family == "direct_storage")
            .collect();
        let plan =
            build_verified_update_plan(&catalog, &games, items, &root.path().join("backups"))
                .unwrap();
        assert_eq!(plan.items.len(), 2);
        assert_ne!(plan.items[0].target_version, plan.items[1].target_version);
        assert_eq!(
            plan.changes
                .iter()
                .filter(|change| change.added_as_dependency)
                .count(),
            1
        );
        validate_update_plan(&catalog, &plan).unwrap();
    }

    #[test]
    fn missing_required_member_blocks_plan_without_adding_game_files() {
        let root = tempfile::tempdir().unwrap();
        let (catalog, mut game) = storage_fixture(root.path());
        game.components
            .retain(|component| component.family == "direct_storage");
        std::fs::remove_file(root.path().join("dstoragecore.dll")).unwrap();
        let games = [game];
        let items = crate::plan_items(&catalog, &games, root.path(), None);
        assert!(build_verified_update_plan(&catalog, &games, items, root.path()).is_err());
        assert!(!root.path().join("dstoragecore.dll").exists());
    }

    #[test]
    fn new_coherent_member_after_planning_invalidates_inventory() {
        let root = tempfile::tempdir().unwrap();
        let (catalog, game) = storage_fixture(root.path());
        let games = [game];
        let items = crate::plan_items(&catalog, &games, root.path(), None);
        let plan =
            build_verified_update_plan(&catalog, &games, items, &root.path().join("backups"))
                .unwrap();
        pe(&root.path().join("dstorage-extra.dll"), 0);
        assert!(validate_update_plan(&catalog, &plan).is_err());
    }
}
