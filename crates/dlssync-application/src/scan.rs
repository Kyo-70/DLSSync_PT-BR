use dll_catalog::Catalog;
use dll_scanner::{InstallObservationError, ObservationErrorCause, ObservationStage};
use dlssync_contracts::{
    ApplicabilityStatus, Architecture, ArtifactDescriptor, ByteCount, CompatibilityDecision,
    CompatibilityStatus, ComponentIdentity, ComponentState, ComponentStatus, ContentHash, Evidence,
    GameSnapshot, GameStateStatus, HashAlgorithm, ObservationError, ObservationErrorCode,
    ScannedComponent, ScannedGame, SignatureStatus, SupportStatus, TrustEvidence, UpdatePlanItem,
};
use launcher_scan::{DetectedGame, LauncherKind};
use parking_lot::{Mutex, MutexGuard};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

static PROJECTION_GATE: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
pub const GAME_PROJECTION_SCOPE: &str = "games_projection";

pub fn projection_guard() -> MutexGuard<'static, ()> {
    PROJECTION_GATE.lock()
}

#[derive(Debug, thiserror::Error)]
pub enum ScanUseCaseError {
    #[error("launcher scan: {0}")]
    Launcher(#[from] launcher_scan::ScanError),
    #[error("component scan: {0}")]
    Component(#[from] dll_scanner::ScanError),
    #[error("scan path has no display name: {0}")]
    InvalidPath(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentObservation {
    pub components: Vec<ScannedComponent>,
    pub complete: bool,
    pub errors: Vec<ObservationError>,
    /// Disk observation alone cannot establish current, applicable, or supported status.
    pub status: GameStateStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectionSettings {
    pub disabled_families: BTreeSet<String>,
    pub pinned_versions: BTreeMap<String, String>,
}

impl ProjectionSettings {
    pub fn pin_key(family: &str, absolute_path: &str) -> String {
        format!("{family}|{absolute_path}")
    }

    fn is_disabled(&self, family: &str) -> bool {
        self.disabled_families
            .iter()
            .any(|disabled| disabled.eq_ignore_ascii_case(family))
    }

    fn pinned_version<'a>(&'a self, component: &ScannedComponent) -> Option<&'a str> {
        let key = Self::pin_key(&component.family, &component.path);
        self.pinned_versions.get(&key).map(String::as_str)
    }
}

pub fn observe_components(root: &Path) -> ComponentObservation {
    observe_components_at(root, &chrono::Utc::now().to_rfc3339())
}

pub fn observe_game_snapshot(
    game: &DetectedGame,
    catalog: Option<&Catalog>,
    settings: &ProjectionSettings,
) -> GameSnapshot {
    observe_game_snapshot_at(game, catalog, settings, &chrono::Utc::now().to_rfc3339())
}

fn observe_game_snapshot_at(
    game: &DetectedGame,
    catalog: Option<&Catalog>,
    settings: &ProjectionSettings,
    checked_at: &str,
) -> GameSnapshot {
    let observation = observe_components_at(&game.install_dir, checked_at);
    let components = observation
        .components
        .iter()
        .map(|component| {
            component_state(game, component, catalog, settings, &observation, checked_at)
        })
        .collect::<Vec<_>>();
    let status = game_status(observation.complete, &components);
    GameSnapshot {
        id: game.id.clone(),
        name: game.name.clone(),
        install_dir: game.install_dir.display().to_string(),
        launcher: serde_json::to_value(game.launcher)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string)),
        art_url: game.image_url.clone(),
        revision: Default::default(),
        checked_at: Some(checked_at.to_string()),
        observation_complete: observation.complete,
        observation_errors: observation.errors,
        status,
        components,
    }
}

pub fn reproject_game_snapshot(
    snapshot: &GameSnapshot,
    catalog: Option<&Catalog>,
    settings: &ProjectionSettings,
) -> GameSnapshot {
    let game = DetectedGame {
        id: snapshot.id.clone(),
        name: snapshot.name.clone(),
        launcher: snapshot
            .launcher
            .as_deref()
            .and_then(|launcher| {
                serde_json::from_value(serde_json::Value::String(launcher.into())).ok()
            })
            .unwrap_or(LauncherKind::Manual),
        install_dir: snapshot.install_dir.clone().into(),
        app_id: None,
        native_ids: Default::default(),
        art: Default::default(),
        image_url: snapshot.art_url.clone(),
        size_bytes: None,
    };
    let checked_at = snapshot.checked_at.as_deref().unwrap_or_default();
    let components = snapshot
        .components
        .iter()
        .map(|existing| {
            let absolute_path = game
                .install_dir
                .join(existing.identity.relative_path.replace('/', "\\"));
            let observed = ScannedComponent {
                family: existing.identity.family.clone(),
                path: absolute_path.display().to_string(),
                current_version: existing.observed_version.clone(),
                sha256: existing
                    .observed_hash
                    .as_ref()
                    .filter(|hash| hash.algorithm == HashAlgorithm::Sha256)
                    .map(|hash| hash.digest.clone()),
            };
            project_component_state(
                &game,
                &observed,
                existing.identity.architecture,
                catalog,
                settings,
                snapshot.observation_complete,
                existing.observation_errors.clone(),
                existing.checked_at.as_deref().unwrap_or(checked_at),
            )
        })
        .collect::<Vec<_>>();
    let mut projected = snapshot.clone();
    projected.status = game_status(snapshot.observation_complete, &components);
    projected.components = components;
    projected
}

fn game_status(observation_complete: bool, components: &[ComponentState]) -> GameStateStatus {
    if !observation_complete {
        GameStateStatus::Unknown
    } else if components.is_empty() {
        GameStateStatus::NoComponents
    } else if components
        .iter()
        .any(|component| component.status == ComponentStatus::UpdateAvailable)
    {
        GameStateStatus::UpdateAvailable
    } else if components
        .iter()
        .all(|component| component.status == ComponentStatus::Current)
    {
        GameStateStatus::Current
    } else if components.iter().all(|component| {
        matches!(
            component.status,
            ComponentStatus::Current
                | ComponentStatus::Newer
                | ComponentStatus::Disabled
                | ComponentStatus::ExternallyManaged
                | ComponentStatus::Incompatible
        )
    }) {
        GameStateStatus::NonActionable
    } else if components
        .iter()
        .any(|component| component.status == ComponentStatus::Unknown)
    {
        GameStateStatus::Unknown
    } else {
        GameStateStatus::Unchecked
    }
}

fn component_state(
    game: &DetectedGame,
    component: &ScannedComponent,
    catalog: Option<&Catalog>,
    settings: &ProjectionSettings,
    observation: &ComponentObservation,
    checked_at: &str,
) -> ComponentState {
    let path = Path::new(&component.path);
    let architecture = pe_version::read_pe_identity(path)
        .map(|identity| identity.architecture)
        .unwrap_or(Architecture::Unknown);
    let component_errors = observation
        .errors
        .iter()
        .filter(|error| error.path.as_deref() == Some(component.path.as_str()))
        .cloned()
        .collect();
    project_component_state(
        game,
        component,
        architecture,
        catalog,
        settings,
        observation.complete,
        component_errors,
        checked_at,
    )
}

#[allow(clippy::too_many_arguments)]
fn project_component_state(
    game: &DetectedGame,
    component: &ScannedComponent,
    architecture: Architecture,
    catalog: Option<&Catalog>,
    settings: &ProjectionSettings,
    observation_complete: bool,
    component_errors: Vec<ObservationError>,
    checked_at: &str,
) -> ComponentState {
    let path = Path::new(&component.path);
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let relative_path = path
        .strip_prefix(&game.install_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let pinned_version = settings.pinned_version(component);
    let release = catalog.and_then(|catalog| match pinned_version {
        Some(version) => catalog.find_file(
            family_vendor(&component.family),
            &component.family,
            version,
            &filename,
        ),
        None => catalog.find_latest_for_file(
            family_vendor(&component.family),
            &component.family,
            &filename,
        ),
    });
    let candidate = release
        .as_ref()
        .map(|release| artifact_descriptor(catalog.unwrap(), &component.family, release));
    let incompatible = catalog.is_some_and(|catalog| {
        catalog.incompatible_games.iter().any(|value| {
            value.eq_ignore_ascii_case(&game.id) || value.eq_ignore_ascii_case(&game.name)
        })
    });
    let status = if !observation_complete || !component_errors.is_empty() {
        ComponentStatus::Unknown
    } else if incompatible {
        ComponentStatus::Incompatible
    } else if settings.is_disabled(&component.family) {
        ComponentStatus::Disabled
    } else if let Some(release) = release.as_ref() {
        component_relation(component, &filename, release, pinned_version.is_some())
    } else if pinned_version.is_some() {
        ComponentStatus::Unknown
    } else {
        ComponentStatus::Unchecked
    };
    let externally_managed = status == ComponentStatus::ExternallyManaged;
    let compatibility = if incompatible {
        CompatibilityDecision {
            status: CompatibilityStatus::Incompatible,
            reason_code: "catalog_game_incompatible".into(),
            evidence: vec![Evidence {
                source: "catalog".into(),
                observed_at: checked_at.to_string(),
                detail: "catalog explicitly lists this game as incompatible".into(),
            }],
        }
    } else {
        CompatibilityDecision {
            status: CompatibilityStatus::Unknown,
            reason_code: if release.is_some() {
                "compatibility_not_evaluated"
            } else if pinned_version.is_some() {
                "pinned_catalog_candidate_unavailable"
            } else {
                "catalog_candidate_unavailable"
            }
            .into(),
            evidence: release
                .is_some()
                .then(|| Evidence {
                    source: "local_observation_and_catalog".into(),
                    observed_at: checked_at.to_string(),
                    detail: "catalog artifact and installed file relation evaluated; game and hardware compatibility remain unverified".into(),
                })
                .into_iter()
                .collect(),
        }
    };
    ComponentState {
        component_id: format!("{}:{}:{relative_path}", game.id, component.family),
        identity: ComponentIdentity {
            game_id: game.id.clone(),
            relative_path,
            family: component.family.clone(),
            filename,
            architecture,
            graphics_api: None,
        },
        observed_version: component.current_version.clone(),
        observed_hash: component.sha256.as_ref().map(|digest| ContentHash {
            algorithm: HashAlgorithm::Sha256,
            digest: digest.clone(),
        }),
        candidate,
        status,
        compatibility,
        support: SupportStatus::Unknown,
        applicability: match status {
            ComponentStatus::UpdateAvailable => ApplicabilityStatus::Applicable,
            ComponentStatus::Current
            | ComponentStatus::Newer
            | ComponentStatus::Disabled
            | ComponentStatus::ExternallyManaged
            | ComponentStatus::Incompatible => ApplicabilityStatus::NotApplicable,
            ComponentStatus::Unchecked | ComponentStatus::Unknown => ApplicabilityStatus::Unknown,
        },
        owner: externally_managed.then(|| "external_streamline_runtime".to_string()),
        checked_at: Some(checked_at.to_string()),
        observation_complete: observation_complete && component_errors.is_empty(),
        observation_errors: component_errors,
        catalog_revision: catalog.map(crate::planning::catalog_revision),
        revision: String::new(),
    }
}

fn component_relation(
    component: &ScannedComponent,
    filename: &str,
    release: &dll_catalog::Release,
    explicitly_pinned: bool,
) -> ComponentStatus {
    if component.sha256.as_deref().is_some_and(|observed| {
        (release.hash_algorithm.eq_ignore_ascii_case("sha256")
            && observed.eq_ignore_ascii_case(&release.sha256))
            || release.artifact.as_ref().is_some_and(|artifact| {
                artifact.hash.algorithm == HashAlgorithm::Sha256
                    && observed.eq_ignore_ascii_case(&artifact.hash.digest)
            })
    }) {
        return ComponentStatus::Current;
    }
    let Some(observed_version) = component.current_version.as_deref() else {
        return ComponentStatus::Unknown;
    };
    let target_version = release
        .artifact
        .as_ref()
        .and_then(|artifact| artifact.file_version.as_deref())
        .unwrap_or(&release.version);
    if dll_scanner::is_streamline_plugin(filename)
        && version_major(observed_version)
            .zip(version_major(target_version))
            .is_some_and(|(observed, target)| observed != target)
    {
        return ComponentStatus::ExternallyManaged;
    }
    if explicitly_pinned {
        return match compare_versions(observed_version, target_version, release.version_packed) {
            Some(Ordering::Equal) => ComponentStatus::Current,
            Some(Ordering::Less | Ordering::Greater) => ComponentStatus::UpdateAvailable,
            None => ComponentStatus::Unknown,
        };
    }
    match compare_versions(observed_version, target_version, release.version_packed) {
        Some(Ordering::Less) => ComponentStatus::UpdateAvailable,
        Some(Ordering::Equal) => ComponentStatus::Current,
        Some(Ordering::Greater) => ComponentStatus::Newer,
        None => ComponentStatus::Unknown,
    }
}

fn compare_versions(observed: &str, target: &str, target_fallback: u64) -> Option<Ordering> {
    let observed = pack_version(observed)?;
    let target = pack_version(target).unwrap_or(target_fallback);
    Some(observed.cmp(&target))
}

fn version_major(version: &str) -> Option<u16> {
    version
        .split('.')
        .next()?
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

fn pack_version(version: &str) -> Option<u64> {
    let mut parts = [0_u16; 4];
    let mut saw_numeric = false;
    for (index, raw) in version.split('.').take(4).enumerate() {
        let digits = raw
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect::<String>();
        if digits.is_empty() {
            if index == 0 {
                return None;
            }
            continue;
        }
        parts[index] = digits.parse().ok()?;
        saw_numeric = true;
    }
    saw_numeric.then(|| {
        ((parts[0] as u64) << 48)
            | ((parts[1] as u64) << 32)
            | ((parts[2] as u64) << 16)
            | parts[3] as u64
    })
}

fn artifact_descriptor(
    catalog: &Catalog,
    family: &str,
    release: &dll_catalog::Release,
) -> ArtifactDescriptor {
    release.artifact.clone().unwrap_or_else(|| {
        use sha2::{Digest as _, Sha256};
        let package_id = hex::encode(Sha256::digest(release.cdn_url.as_bytes()));
        ArtifactDescriptor {
            id: format!("{package_id}:{}:{}", release.filename, release.sha256),
            family: family.to_string(),
            filename: release.filename.clone(),
            file_version: None,
            package_version: release.version.clone(),
            package_id,
            compatibility_line: "legacy-unclassified".to_string(),
            architecture: Architecture::X64,
            hash: ContentHash {
                algorithm: if release.hash_algorithm == "md5" {
                    HashAlgorithm::Md5
                } else {
                    HashAlgorithm::Sha256
                },
                digest: release.sha256.clone(),
            },
            size_bytes: ByteCount::from(release.size_bytes),
            source_url: release.cdn_url.clone(),
            archive_entry: release.zip_entry.clone(),
            expected_publisher: release.signature_subject.clone(),
            observed_publisher: None,
            signature_status: SignatureStatus::NotChecked,
            dependencies: Vec::new(),
            checked_at: catalog.generated_at.to_rfc3339(),
        }
    })
}

pub fn scan_path(root: &Path) -> Result<ScannedGame, ScanUseCaseError> {
    let name = root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ScanUseCaseError::InvalidPath(root.display().to_string()))?;
    Ok(ScannedGame {
        id: stable_game_id(root),
        name: name.to_string(),
        launcher: "manual".into(),
        install_dir: root.display().to_string(),
        components: scan_components(root)?,
    })
}

pub fn scan_installed_games() -> Result<Vec<ScannedGame>, ScanUseCaseError> {
    let launchers = [
        LauncherKind::Steam,
        LauncherKind::Epic,
        LauncherKind::Gog,
        LauncherKind::Ubisoft,
        LauncherKind::EaDesktop,
        LauncherKind::Xbox,
        LauncherKind::Battlenet,
    ];
    launcher_scan::scan_all(&launchers)?
        .into_iter()
        .map(|game| {
            Ok(ScannedGame {
                id: game.id,
                name: game.name,
                launcher: serde_json::to_value(game.launcher)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "unknown".into()),
                install_dir: game.install_dir.display().to_string(),
                components: scan_components(&game.install_dir)?,
            })
        })
        .collect()
}

pub fn plan_items(
    catalog: &Catalog,
    games: &[ScannedGame],
    backup_root: &Path,
    game_filter: Option<&str>,
) -> Vec<UpdatePlanItem> {
    let mut items = Vec::new();
    for game in games
        .iter()
        .filter(|game| game_filter.is_none_or(|filter| game.id == filter))
    {
        for component in &game.components {
            let path = Path::new(&component.path);
            let Some(filename) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let vendor = family_vendor(&component.family);
            let Some(release) = catalog.find_latest_for_file(vendor, &component.family, filename)
            else {
                continue;
            };
            if component.current_version.as_deref() == Some(release.version.as_str()) {
                continue;
            }
            let relative = path.strip_prefix(&game.install_dir).unwrap_or(path);
            let id = format!(
                "{}:{}:{}",
                game.id,
                component.family,
                relative.to_string_lossy().replace('\\', "/")
            );
            items.push(UpdatePlanItem {
                id,
                game_id: game.id.clone(),
                game_name: game.name.clone(),
                dll_path: component.path.clone(),
                family: component.family.clone(),
                current_version: component.current_version.clone(),
                target_version: release.version,
                backup_path: backup_root
                    .join(&game.id)
                    .join(filename)
                    .display()
                    .to_string(),
                selected: true,
                trust: TrustEvidence {
                    source_url: release.cdn_url,
                    expected_sha256: release.sha256,
                    observed_sha256: component.sha256.clone(),
                    signature_subject: release.signature_subject,
                    signature_verified: release.signed,
                    anti_cheat_risk: None,
                },
            });
        }
    }
    items
}

fn scan_components(root: &Path) -> Result<Vec<ScannedComponent>, dll_scanner::ScanError> {
    Ok(observe_components(root).components)
}

fn observe_components_at(root: &Path, observed_at: &str) -> ComponentObservation {
    let observation = dll_scanner::observe_install(root);
    let components = observation
        .records
        .into_iter()
        .map(|record| ScannedComponent {
            family: record.family.catalog_key().into(),
            path: record.path.display().to_string(),
            current_version: record.current_version,
            sha256: record.sha256,
        })
        .collect::<Vec<_>>();
    let status = if !observation.complete {
        GameStateStatus::Unknown
    } else if components.is_empty() {
        GameStateStatus::NoComponents
    } else {
        GameStateStatus::Unchecked
    };
    ComponentObservation {
        components,
        complete: observation.complete,
        errors: observation
            .errors
            .into_iter()
            .map(|error| map_observation_error(error, observed_at))
            .collect(),
        status,
    }
}

fn map_observation_error(error: InstallObservationError, observed_at: &str) -> ObservationError {
    let code = match error.cause {
        ObservationErrorCause::Walk => ObservationErrorCode::InaccessibleDirectory,
        ObservationErrorCause::Hash => ObservationErrorCode::Unmeasured,
        ObservationErrorCause::PeParse => ObservationErrorCode::ParseFailed,
        ObservationErrorCause::AccessDenied => ObservationErrorCode::ReadDenied,
        ObservationErrorCause::Missing => ObservationErrorCode::Missing,
        ObservationErrorCause::ChangedDuringRead => ObservationErrorCode::ChangedDuringRead,
    };
    let stage = match error.stage {
        ObservationStage::Walk => "walk",
        ObservationStage::Hash => "hash",
        ObservationStage::PeParse => "pe_parse",
    };
    ObservationError {
        code,
        path: Some(error.path.display().to_string()),
        detail: format!("{stage}: {}", error.message),
        observed_at: observed_at.to_string(),
    }
}

fn stable_game_id(root: &Path) -> String {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(root.to_string_lossy().to_ascii_lowercase().as_bytes());
    format!("manual-{}", &hex::encode(digest)[..16])
}

fn family_vendor(family: &str) -> &'static str {
    dll_scanner::family_vendor(family).unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dll_scanner::{InstallObservationError, ObservationErrorCause, ObservationStage};
    use dlssync_contracts::{
        ApplicabilityStatus, ComponentStatus, GameStateStatus, ObservationErrorCode, SupportStatus,
    };
    use tempfile::tempdir;

    fn projection_game(root: &Path, _filename: &str) -> DetectedGame {
        DetectedGame {
            id: "game-a".to_string(),
            name: "Game A".to_string(),
            launcher: LauncherKind::Manual,
            install_dir: root.to_path_buf(),
            app_id: None,
            native_ids: Default::default(),
            art: Default::default(),
            image_url: None,
            size_bytes: None,
        }
    }

    fn projection_catalog(
        family: &str,
        filename: &str,
        package_version: &str,
        file_version: &str,
        version_packed: u64,
        hash: &str,
    ) -> Catalog {
        serde_json::from_value(serde_json::json!({
            "schema_version": 3,
            "generated_at": "2026-09-18T00:00:00Z",
            "vendors": {
                "nvidia": {
                    family: {
                        "latest": package_version,
                        "releases": [{
                            "artifact": {
                                "id": format!("artifact:{family}:{filename}"),
                                "family": family,
                                "filename": filename,
                                "file_version": file_version,
                                "package_version": package_version,
                                "package_id": "package-a",
                                "compatibility_line": "unclassified",
                                "architecture": "x64",
                                "hash": { "algorithm": "sha256", "digest": hash },
                                "size_bytes": "10",
                                "source_url": "https://example.invalid/a.dll",
                                "archive_entry": null,
                                "expected_publisher": null,
                                "observed_publisher": null,
                                "signature_status": "not_checked",
                                "dependencies": [],
                                "checked_at": "2026-09-18T00:00:00Z"
                            },
                            "version": package_version,
                            "version_packed": version_packed,
                            "filename": filename,
                            "sha256": hash,
                            "size_bytes": 10,
                            "signed": false,
                            "released_at": "2026-09-18T00:00:00Z",
                            "source": "fixture",
                            "cdn_url": "https://example.invalid/a.dll",
                            "channel": "stable",
                            "is_dev": false
                        }]
                    }
                }
            },
            "sources": {},
            "incompatible_games": [],
            "anti_cheat_binaries": []
        }))
        .unwrap()
    }

    fn projected_component(
        game: &DetectedGame,
        family: &str,
        filename: &str,
        version: &str,
        hash: &str,
        catalog: &Catalog,
    ) -> ComponentState {
        projected_component_with_settings(
            game,
            family,
            filename,
            version,
            hash,
            catalog,
            &ProjectionSettings::default(),
        )
    }

    fn projected_component_with_settings(
        game: &DetectedGame,
        family: &str,
        filename: &str,
        version: &str,
        hash: &str,
        catalog: &Catalog,
        settings: &ProjectionSettings,
    ) -> ComponentState {
        let component = ScannedComponent {
            family: family.to_string(),
            path: game.install_dir.join(filename).display().to_string(),
            current_version: Some(version.to_string()),
            sha256: Some(hash.to_string()),
        };
        component_state(
            game,
            &component,
            Some(catalog),
            settings,
            &ComponentObservation {
                components: vec![component.clone()],
                complete: true,
                errors: Vec::new(),
                status: GameStateStatus::Unchecked,
            },
            "2026-09-18T12:00:00Z",
        )
    }

    #[test]
    fn manual_scan_has_a_stable_non_path_id() {
        let dir = tempdir().unwrap();
        let first = scan_path(dir.path()).unwrap();
        let second = scan_path(dir.path()).unwrap();
        assert_eq!(first.id, second.id);
        assert!(first.id.starts_with("manual-"));
        assert!(!first.id.contains('\\'));
    }

    #[test]
    fn phase4_incomplete_observation_is_unknown_not_no_components() {
        let root = tempdir().unwrap();
        std::fs::write(root.path().join("nvngx_dlss.dll"), b"not a PE").unwrap();

        let partial = observe_components_at(root.path(), "2026-09-17T12:00:00Z");

        assert_eq!(partial.components.len(), 1);
        assert!(!partial.complete);
        assert_eq!(partial.status, GameStateStatus::Unknown);
        assert!(partial
            .errors
            .iter()
            .any(|error| error.code == ObservationErrorCode::ParseFailed));

        let missing = root.path().join("missing-install");
        let observation = observe_components_at(&missing, "2026-09-17T12:00:00Z");

        assert!(observation.components.is_empty());
        assert!(!observation.complete);
        assert_eq!(observation.status, GameStateStatus::Unknown);
        assert_ne!(observation.status, GameStateStatus::NoComponents);
        assert_eq!(observation.errors.len(), 1);
        assert_eq!(observation.errors[0].code, ObservationErrorCode::Missing);
        assert_eq!(
            observation.errors[0].path.as_deref(),
            Some(missing.to_string_lossy().as_ref())
        );
        assert!(observation.errors[0].detail.contains("walk"));
        assert_eq!(observation.errors[0].observed_at, "2026-09-17T12:00:00Z");
    }

    #[test]
    fn phase4_scanner_causes_map_to_runtime_error_codes() {
        let expected = [
            (
                ObservationErrorCause::Walk,
                ObservationErrorCode::InaccessibleDirectory,
            ),
            (
                ObservationErrorCause::Hash,
                ObservationErrorCode::Unmeasured,
            ),
            (
                ObservationErrorCause::PeParse,
                ObservationErrorCode::ParseFailed,
            ),
            (
                ObservationErrorCause::AccessDenied,
                ObservationErrorCode::ReadDenied,
            ),
            (
                ObservationErrorCause::Missing,
                ObservationErrorCode::Missing,
            ),
            (
                ObservationErrorCause::ChangedDuringRead,
                ObservationErrorCode::ChangedDuringRead,
            ),
        ];

        for (cause, code) in expected {
            let mapped = map_observation_error(
                InstallObservationError {
                    path: "fixture.dll".into(),
                    cause,
                    stage: ObservationStage::Hash,
                    message: "fixture cause".into(),
                },
                "2026-09-17T12:00:00Z",
            );
            assert_eq!(mapped.code, code);
            assert!(mapped.detail.contains("fixture cause"));
        }
    }

    #[test]
    fn phase4_legacy_scan_matches_complete_component_observation() {
        let root = tempdir().unwrap();
        std::fs::write(root.path().join("ordinary.txt"), b"ignored").unwrap();

        let legacy = scan_path(root.path()).unwrap();
        let observation = observe_components_at(root.path(), "2026-09-17T12:00:00Z");

        assert!(observation.complete, "{:?}", observation.errors);
        assert_eq!(observation.status, GameStateStatus::NoComponents);
        assert_eq!(legacy.components, observation.components);
        assert!(legacy.components.is_empty());
    }

    #[test]
    fn phase10_game_snapshot_preserves_incomplete_observation_errors() {
        let root = tempdir().unwrap();
        let missing = root.path().join("missing-install");
        let game = launcher_scan::DetectedGame {
            id: "game-a".to_string(),
            name: "Game A".to_string(),
            launcher: LauncherKind::Manual,
            install_dir: missing,
            app_id: None,
            native_ids: Default::default(),
            art: Default::default(),
            image_url: None,
            size_bytes: None,
        };

        let snapshot = observe_game_snapshot_at(
            &game,
            None,
            &ProjectionSettings::default(),
            "2026-09-18T12:00:00Z",
        );

        assert_eq!(snapshot.status, GameStateStatus::Unknown);
        assert!(!snapshot.observation_complete);
        assert!(snapshot.components.is_empty());
        assert_eq!(snapshot.observation_errors.len(), 1);
        assert_eq!(
            snapshot.observation_errors[0].code,
            ObservationErrorCode::Missing
        );
        assert_ne!(snapshot.status, GameStateStatus::NoComponents);

        let install = root.path().join("partial-install");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("nvngx_dlss.dll"), b"not a PE").unwrap();
        let partial_game = launcher_scan::DetectedGame {
            install_dir: install,
            ..game
        };
        let partial = observe_game_snapshot_at(
            &partial_game,
            None,
            &ProjectionSettings::default(),
            "2026-09-18T12:00:01Z",
        );
        assert_eq!(partial.status, GameStateStatus::Unknown);
        assert!(!partial.observation_complete);
        assert_eq!(partial.components.len(), 1);
        assert!(partial
            .observation_errors
            .iter()
            .any(|error| error.code == ObservationErrorCode::ParseFailed));
    }

    #[test]
    fn matching_candidate_sha_is_current_even_when_version_labels_differ() {
        let root = tempdir().unwrap();
        let game = projection_game(root.path(), "nvngx_dlss.dll");
        let hash = "ab".repeat(32);
        let catalog = projection_catalog(
            "dlss_sr",
            "nvngx_dlss.dll",
            "310.4.0",
            "310.4.0.0",
            (310_u64 << 48) | (4_u64 << 32),
            &hash,
        );

        let state = projected_component(
            &game,
            "dlss_sr",
            "nvngx_dlss.dll",
            "different-label",
            &hash.to_ascii_uppercase(),
            &catalog,
        );

        assert_eq!(state.status, ComponentStatus::Current);
    }

    #[test]
    fn observed_version_above_candidate_is_newer_not_update_available() {
        let root = tempdir().unwrap();
        let game = projection_game(root.path(), "nvngx_dlss.dll");
        let catalog = projection_catalog(
            "dlss_sr",
            "nvngx_dlss.dll",
            "310.4.0",
            "310.4.0.0",
            (310_u64 << 48) | (4_u64 << 32),
            &"ab".repeat(32),
        );

        let state = projected_component(
            &game,
            "dlss_sr",
            "nvngx_dlss.dll",
            "310.9.0.0",
            &"cd".repeat(32),
            &catalog,
        );

        assert_eq!(state.status, ComponentStatus::Newer);
    }

    #[test]
    fn streamline_cross_major_candidate_is_externally_managed() {
        let root = tempdir().unwrap();
        let game = projection_game(root.path(), "sl.dlss_g.dll");
        let catalog = projection_catalog(
            "sl_dlss_fg",
            "sl.dlss_g.dll",
            "2.11.1",
            "2.11.1.0",
            (2_u64 << 48) | (11_u64 << 32) | (1_u64 << 16),
            &"ab".repeat(32),
        );

        let state = projected_component(
            &game,
            "sl_dlss_fg",
            "sl.dlss_g.dll",
            "310.4.0.0",
            &"cd".repeat(32),
            &catalog,
        );

        assert_eq!(state.status, ComponentStatus::ExternallyManaged);
        assert!(state.owner.is_some());
    }

    #[test]
    fn catalog_presence_does_not_fabricate_compatibility_support_or_applicability() {
        let root = tempdir().unwrap();
        let game = projection_game(root.path(), "nvngx_dlss.dll");
        let catalog = projection_catalog(
            "dlss_sr",
            "nvngx_dlss.dll",
            "310.4.0",
            "310.4.0.0",
            (310_u64 << 48) | (4_u64 << 32),
            &"ab".repeat(32),
        );

        let state = projected_component(
            &game,
            "dlss_sr",
            "nvngx_dlss.dll",
            "310.1.0.0",
            &"cd".repeat(32),
            &catalog,
        );

        assert_eq!(state.compatibility.status, CompatibilityStatus::Unknown);
        assert_eq!(state.support, SupportStatus::Unknown);
        assert_eq!(state.applicability, ApplicabilityStatus::Applicable);
    }

    #[test]
    fn disabled_family_is_non_actionable_even_with_a_newer_catalog_candidate() {
        let root = tempdir().unwrap();
        let game = projection_game(root.path(), "nvngx_dlss.dll");
        let catalog = projection_catalog(
            "dlss_sr",
            "nvngx_dlss.dll",
            "310.4.0",
            "310.4.0.0",
            (310_u64 << 48) | (4_u64 << 32),
            &"ab".repeat(32),
        );
        let settings = ProjectionSettings {
            disabled_families: BTreeSet::from(["dlss_sr".to_string()]),
            pinned_versions: BTreeMap::new(),
        };

        let state = projected_component_with_settings(
            &game,
            "dlss_sr",
            "nvngx_dlss.dll",
            "310.1.0.0",
            &"cd".repeat(32),
            &catalog,
            &settings,
        );

        assert_eq!(state.status, ComponentStatus::Disabled);
    }

    #[test]
    fn unavailable_pinned_version_fails_closed_without_latest_fallback() {
        let root = tempdir().unwrap();
        let game = projection_game(root.path(), "nvngx_dlss.dll");
        let catalog = projection_catalog(
            "dlss_sr",
            "nvngx_dlss.dll",
            "310.4.0",
            "310.4.0.0",
            (310_u64 << 48) | (4_u64 << 32),
            &"ab".repeat(32),
        );
        let path = game
            .install_dir
            .join("nvngx_dlss.dll")
            .display()
            .to_string();
        let settings = ProjectionSettings {
            disabled_families: BTreeSet::new(),
            pinned_versions: BTreeMap::from([(
                ProjectionSettings::pin_key("dlss_sr", &path),
                "309.9.0".to_string(),
            )]),
        };

        let state = projected_component_with_settings(
            &game,
            "dlss_sr",
            "nvngx_dlss.dll",
            "309.1.0.0",
            &"cd".repeat(32),
            &catalog,
            &settings,
        );

        assert_eq!(state.status, ComponentStatus::Unknown);
        assert!(state.candidate.is_none());
        assert_eq!(
            state.compatibility.reason_code,
            "pinned_catalog_candidate_unavailable"
        );
    }

    #[test]
    fn explicit_pin_can_select_a_lower_catalog_release() {
        let root = tempdir().unwrap();
        let game = projection_game(root.path(), "nvngx_dlss.dll");
        let mut catalog = projection_catalog(
            "dlss_sr",
            "nvngx_dlss.dll",
            "310.4.0",
            "310.4.0.0",
            (310_u64 << 48) | (4_u64 << 32),
            &"ab".repeat(32),
        );
        let pinned: dll_catalog::Release = serde_json::from_value(serde_json::json!({
            "artifact": {
                "id": "artifact:pinned",
                "family": "dlss_sr",
                "filename": "nvngx_dlss.dll",
                "file_version": "309.2.0.0",
                "package_version": "309.2.0",
                "package_id": "package-pinned",
                "compatibility_line": "unclassified",
                "architecture": "x64",
                "hash": { "algorithm": "sha256", "digest": "ef".repeat(32) },
                "size_bytes": "10",
                "source_url": "https://example.invalid/pinned.dll",
                "archive_entry": null,
                "expected_publisher": null,
                "observed_publisher": null,
                "signature_status": "not_checked",
                "dependencies": [],
                "checked_at": "2026-09-18T00:00:00Z"
            },
            "version": "309.2.0",
            "version_packed": (309_u64 << 48) | (2_u64 << 32),
            "filename": "nvngx_dlss.dll",
            "sha256": "ef".repeat(32),
            "size_bytes": 10,
            "signed": false,
            "released_at": "2026-09-17T00:00:00Z",
            "source": "fixture",
            "cdn_url": "https://example.invalid/pinned.dll",
            "channel": "stable",
            "is_dev": false
        }))
        .unwrap();
        catalog
            .vendors
            .get_mut("nvidia")
            .unwrap()
            .get_mut("dlss_sr")
            .unwrap()
            .releases
            .push(pinned);
        let path = game
            .install_dir
            .join("nvngx_dlss.dll")
            .display()
            .to_string();
        let settings = ProjectionSettings {
            disabled_families: BTreeSet::new(),
            pinned_versions: BTreeMap::from([(
                ProjectionSettings::pin_key("dlss_sr", &path),
                "309.2.0".to_string(),
            )]),
        };

        let state = projected_component_with_settings(
            &game,
            "dlss_sr",
            "nvngx_dlss.dll",
            "310.1.0.0",
            &"cd".repeat(32),
            &catalog,
            &settings,
        );

        assert_eq!(state.status, ComponentStatus::UpdateAvailable);
        assert_eq!(
            state
                .candidate
                .as_ref()
                .map(|candidate| candidate.package_version.as_str()),
            Some("309.2.0")
        );
    }

    #[test]
    fn reprojection_uses_preserved_observation_without_reading_disk() {
        let root = tempdir().unwrap();
        let game = projection_game(root.path(), "nvngx_dlss.dll");
        let catalog = projection_catalog(
            "dlss_sr",
            "nvngx_dlss.dll",
            "310.4.0",
            "310.4.0.0",
            (310_u64 << 48) | (4_u64 << 32),
            &"ab".repeat(32),
        );
        let component = projected_component(
            &game,
            "dlss_sr",
            "nvngx_dlss.dll",
            "310.1.0.0",
            &"cd".repeat(32),
            &catalog,
        );
        let snapshot = GameSnapshot {
            id: game.id.clone(),
            name: game.name.clone(),
            install_dir: game.install_dir.display().to_string(),
            launcher: Some("manual".into()),
            checked_at: Some("2026-09-18T12:00:00Z".into()),
            observation_complete: true,
            status: GameStateStatus::UpdateAvailable,
            components: vec![component],
            ..GameSnapshot::default()
        };
        let settings = ProjectionSettings {
            disabled_families: BTreeSet::from(["dlss_sr".to_string()]),
            pinned_versions: BTreeMap::new(),
        };

        let projected = reproject_game_snapshot(&snapshot, Some(&catalog), &settings);

        assert_eq!(projected.status, GameStateStatus::NonActionable);
        assert_eq!(projected.components[0].status, ComponentStatus::Disabled);
        assert_eq!(projected.checked_at, snapshot.checked_at);
        assert_eq!(
            projected.components[0].observed_hash,
            snapshot.components[0].observed_hash
        );
    }

    #[test]
    fn normalized_file_versions_treat_missing_zero_segments_as_equal() {
        assert_eq!(
            compare_versions("310.4", "310.4.0.0", (310_u64 << 48) | (4_u64 << 32)),
            Some(Ordering::Equal)
        );
    }
}
