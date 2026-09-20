use crate::error::{AppError, AppResult};
use crate::state::AppState;
use dll_catalog::{
    fetch_verified_generation, manifest_public_key_fingerprint, Catalog, Release,
    VerifiedFetchOutcome, VerifiedFetchRequest,
};
use dlssync_application::product_config;
use dlssync_contracts::{
    ApiError, CatalogDelta, CatalogProvenance, CatalogRefreshResult, CatalogRefreshTrigger,
    CatalogRemoteResult, CatalogSource, CatalogState, CatalogStatus, OperationActor, OperationKind,
    OperationRecord, OperationStatus, StateDelta,
};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::io::Write as _;
use std::path::Path;
use std::time::Instant;
use tauri::State;

#[derive(Debug, Serialize, specta::Type)]
pub struct CatalogSummary {
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub vendors: Vec<VendorSummary>,
    pub incompatible_games: Vec<String>,
    pub sources: BTreeMap<String, dll_catalog::SourceHealth>,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct VendorSummary {
    pub vendor: String,
    pub families: Vec<FamilySummary>,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct FamilySummary {
    pub family: String,
    pub latest: String,
    pub release_count: usize,
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn refresh_catalog(
    state: State<'_, AppState>,
    trigger: Option<CatalogRefreshTrigger>,
) -> AppResult<CatalogRefreshResult> {
    let started = Instant::now();
    let trigger = trigger.unwrap_or(CatalogRefreshTrigger::Automatic);
    let policy = *state.distribution_policy.read();
    let current = state.catalog.read().clone();
    let config = product_config().map_err(|error| AppError::Other(error.to_string()))?;

    if !policy.permits_catalog_refresh(trigger) {
        let provenance =
            state.catalog_provenance.read().clone().unwrap_or_else(|| {
                provenance_for_current(&config, current.as_ref(), trigger, None)
            });
        let result = CatalogRefreshResult {
            refreshed: false,
            blocked_by_policy: true,
            provenance,
            delta: empty_delta(),
        };
        append_refresh_journal(&state, trigger, &result, started.elapsed(), None)?;
        return Ok(result);
    }

    let cache_path = state
        .catalog_cache_path
        .read()
        .clone()
        .ok_or_else(|| AppError::Other("catalog cache path is unavailable".into()))?;
    let outcome = fetch_verified_generation(VerifiedFetchRequest {
        client: &state.http_catalog,
        cache_path: &cache_path,
        url: &config.catalog.canonical_manifest,
        minimum_generated_at: current.as_ref().map(|catalog| catalog.generated_at),
    })
    .await;
    let (fetched, remote_result, refreshed) = match outcome {
        VerifiedFetchOutcome::Modified(generation) => {
            (generation.catalog, CatalogRemoteResult::Modified, true)
        }
        VerifiedFetchOutcome::NotModified(generation) => {
            (generation.catalog, CatalogRemoteResult::NotModified, false)
        }
        VerifiedFetchOutcome::Failed { error, retained } => {
            let retained_catalog = retained.map(|generation| generation.catalog);
            let published_catalog = current.as_ref().or(retained_catalog.as_ref());
            let provenance = state.catalog_provenance.read().clone().unwrap_or_else(|| {
                provenance_for_current(&config, published_catalog, trigger, None)
            });
            if current.is_none() {
                let _projection_guard = dlssync_application::scan::projection_guard();
                if let Some(catalog) = retained_catalog.as_ref() {
                    let mut live_catalog = state.catalog.write();
                    if live_catalog.is_none() {
                        *live_catalog = Some(catalog.clone());
                    }
                }
                let mut live_provenance = state.catalog_provenance.write();
                if live_provenance.is_none() {
                    *live_provenance = Some(provenance.clone());
                }
                drop(live_provenance);
                let live_catalog = state.catalog.read().clone();
                publish_catalog_projection_locked(
                    &state,
                    live_catalog.as_ref(),
                    Some(&provenance),
                    CatalogSource::Cache,
                    CatalogRemoteResult::Failed,
                    Some(error.to_string()),
                );
            } else {
                publish_catalog_projection(
                    &state,
                    published_catalog,
                    Some(&provenance),
                    CatalogSource::Cache,
                    CatalogRemoteResult::Failed,
                    Some(error.to_string()),
                );
            }
            let failed = CatalogRefreshResult {
                refreshed: false,
                blocked_by_policy: false,
                provenance,
                delta: empty_delta(),
            };
            append_refresh_journal(
                &state,
                trigger,
                &failed,
                started.elapsed(),
                Some(error.to_string()),
            )?;
            return Err(error.into());
        }
    };
    let delta = if refreshed {
        catalog_delta(current.as_ref(), &fetched)
    } else {
        empty_delta()
    };
    let provenance = if refreshed {
        provenance_for_current(&config, Some(&fetched), trigger, None)
    } else {
        state
            .catalog_provenance
            .read()
            .clone()
            .unwrap_or_else(|| provenance_for_current(&config, Some(&fetched), trigger, None))
    };
    if refreshed {
        let _projection_guard = dlssync_application::scan::projection_guard();
        persist_provenance(&state, &provenance)?;
        *state.catalog.write() = Some(fetched.clone());
        *state.catalog_provenance.write() = Some(provenance.clone());
        publish_catalog_projection_locked(
            &state,
            Some(&fetched),
            Some(&provenance),
            CatalogSource::Remote,
            remote_result,
            None,
        );
    } else {
        publish_catalog_projection(
            &state,
            Some(&fetched),
            Some(&provenance),
            CatalogSource::Remote,
            remote_result,
            None,
        );
    }

    let result = CatalogRefreshResult {
        refreshed,
        blocked_by_policy: false,
        provenance,
        delta,
    };
    append_refresh_journal(&state, trigger, &result, started.elapsed(), None)?;
    Ok(result)
}

pub fn publish_catalog_projection(
    state: &AppState,
    catalog: Option<&Catalog>,
    provenance: Option<&CatalogProvenance>,
    source: CatalogSource,
    remote_result: CatalogRemoteResult,
    error: Option<String>,
) {
    let _projection_guard = dlssync_application::scan::projection_guard();
    publish_catalog_projection_locked(state, catalog, provenance, source, remote_result, error);
}

fn publish_catalog_projection_locked(
    state: &AppState,
    catalog: Option<&Catalog>,
    provenance: Option<&CatalogProvenance>,
    source: CatalogSource,
    remote_result: CatalogRemoteResult,
    error: Option<String>,
) {
    let ticket = state
        .authoritative_state
        .begin_observation(dlssync_application::scan::GAME_PROJECTION_SCOPE);
    let previous_snapshot = state.authoritative_state.snapshot();
    let previous = previous_snapshot.catalog.clone();
    let now = chrono::Utc::now().to_rfc3339();
    let policy = *state.distribution_policy.read();
    let catalog_state = catalog_state_after_attempt(
        &previous,
        catalog,
        provenance,
        source,
        remote_result,
        error,
        &now,
        policy.automatic_catalog_refresh,
        policy.manual_catalog_refresh,
        policy.app_updates,
    );
    let settings = state.settings.read().clone();
    let games = previous_snapshot
        .games
        .iter()
        .map(|game| {
            dlssync_application::scan::reproject_game_snapshot(
                game,
                catalog,
                &crate::commands::settings::projection_settings(&settings, &game.id),
            )
        })
        .collect::<Vec<_>>();
    let affected_game_ids = games.iter().map(|game| game.id.clone()).collect();
    if let Ok(receipt) = state.authoritative_state.commit_observation(
        ticket,
        dlssync_application::state::StateCommit {
            delta: StateDelta {
                catalog: Some(catalog_state),
                affected_game_ids,
                games,
                ..StateDelta::default()
            },
            ..dlssync_application::state::StateCommit::default()
        },
    ) {
        if let Some(error) = receipt.delivery_error {
            tracing::warn!(%error, "catalog state event delivery failed; snapshot retained");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn catalog_state_after_attempt(
    previous: &CatalogState,
    catalog: Option<&Catalog>,
    provenance: Option<&CatalogProvenance>,
    source: CatalogSource,
    remote_result: CatalogRemoteResult,
    error: Option<String>,
    attempted_at: &str,
    automatic_refresh_enabled: bool,
    manual_refresh_enabled: bool,
    app_updates_enabled: bool,
) -> CatalogState {
    let replace_content = matches!(
        remote_result,
        CatalogRemoteResult::Never | CatalogRemoteResult::Modified
    ) || previous.content_revision.is_none();
    let mut next = if replace_content {
        CatalogState {
            content_revision: catalog.map(dlssync_application::planning::catalog_revision),
            generated_at: catalog.map(|catalog| catalog.generated_at.to_rfc3339()),
            source: if catalog.is_some() {
                source
            } else {
                previous.source
            },
            signature_verified: provenance.is_some_and(|value| value.signature_verified),
            public_key_fingerprint: provenance.map(|value| value.public_key_fingerprint.clone()),
            last_remote_attempt_at: previous.last_remote_attempt_at.clone(),
            last_remote_success_at: previous.last_remote_success_at.clone(),
            ..CatalogState::default()
        }
    } else {
        previous.clone()
    };
    next.automatic_refresh_enabled = automatic_refresh_enabled;
    next.manual_refresh_enabled = manual_refresh_enabled;
    next.app_updates_enabled = app_updates_enabled;

    match remote_result {
        CatalogRemoteResult::Never => {
            next.last_remote_result = CatalogRemoteResult::Never;
            next.error = None;
        }
        CatalogRemoteResult::Modified => {
            next.last_remote_attempt_at = Some(attempted_at.to_string());
            next.last_remote_success_at = Some(attempted_at.to_string());
            next.last_remote_result = CatalogRemoteResult::Modified;
            next.error = None;
        }
        CatalogRemoteResult::NotModified => {
            next.last_remote_attempt_at = Some(attempted_at.to_string());
            next.last_remote_result = CatalogRemoteResult::NotModified;
            next.error = None;
        }
        CatalogRemoteResult::Failed => {
            next.last_remote_attempt_at = Some(attempted_at.to_string());
            next.last_remote_result = CatalogRemoteResult::Failed;
            next.error = error.map(|message| ApiError {
                code: "catalog_refresh_failed".into(),
                message,
                retryable: true,
                context: BTreeMap::new(),
            });
        }
    }
    next
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn catalog_status(state: State<'_, AppState>) -> AppResult<CatalogStatus> {
    let policy = *state.distribution_policy.read();
    let current = state.catalog.read();
    let config = product_config().map_err(|error| AppError::Other(error.to_string()))?;
    let provenance = state.catalog_provenance.read().clone().unwrap_or_else(|| {
        provenance_for_current(
            &config,
            current.as_ref(),
            CatalogRefreshTrigger::Automatic,
            None,
        )
    });
    Ok(CatalogStatus {
        distribution: policy.channel,
        install_mode: policy.install_mode,
        automatic_refresh_enabled: policy.automatic_catalog_refresh,
        manual_refresh_enabled: policy.manual_catalog_refresh,
        app_updates_enabled: policy.app_updates,
        provenance,
    })
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn catalog_summary(state: State<'_, AppState>) -> AppResult<CatalogSummary> {
    let guard = state.catalog.read();
    let catalog = guard
        .as_ref()
        .ok_or_else(|| AppError::Other("catalog not loaded".into()))?;
    let vendors = catalog
        .vendors
        .iter()
        .map(|(vendor, families)| VendorSummary {
            vendor: vendor.clone(),
            families: families
                .iter()
                .map(|(family, entry)| FamilySummary {
                    family: family.clone(),
                    latest: entry.latest.clone(),
                    release_count: entry.releases.len(),
                })
                .collect(),
        })
        .collect();
    Ok(CatalogSummary {
        sources: catalog.sources.clone(),
        generated_at: catalog.generated_at,
        vendors,
        incompatible_games: catalog.incompatible_games.clone(),
    })
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn list_releases(
    state: State<'_, AppState>,
    vendor: String,
    family: String,
) -> AppResult<Vec<Release>> {
    let guard = state.catalog.read();
    let catalog = guard
        .as_ref()
        .ok_or_else(|| AppError::Other("catalog not loaded".into()))?;
    Ok(catalog.releases(&vendor, &family))
}

pub fn shas_key(vendor: &str, family: &str, filename: &str) -> String {
    format!(
        "{}::{}::{}",
        vendor.to_ascii_lowercase(),
        family.to_ascii_lowercase(),
        filename.to_ascii_lowercase()
    )
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn catalog_latest_shas(state: State<'_, AppState>) -> AppResult<HashMap<String, String>> {
    let guard = state.catalog.read();
    let catalog = guard
        .as_ref()
        .ok_or_else(|| AppError::Other("catalog not loaded".into()))?;
    let mut out: HashMap<String, (u64, String)> = HashMap::new();
    for (vendor, families) in &catalog.vendors {
        for (family, entry) in families {
            for release in &entry.releases {
                let key = shas_key(vendor, family, &release.filename);
                let candidate = (release.version_packed, release.sha256.clone());
                match out.get(&key) {
                    Some(existing) if existing.0 >= release.version_packed => {}
                    _ => {
                        out.insert(key, candidate);
                    }
                }
            }
        }
    }
    Ok(out.into_iter().map(|(key, value)| (key, value.1)).collect())
}

pub fn load_persisted_provenance(path: &Path) -> Option<CatalogProvenance> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub fn provenance_for_current(
    config: &dlssync_application::ProductConfig,
    catalog: Option<&Catalog>,
    trigger: CatalogRefreshTrigger,
    source_commit: Option<String>,
) -> CatalogProvenance {
    let now = chrono::Utc::now().to_rfc3339();
    CatalogProvenance {
        manifest_url: config.catalog.canonical_manifest.clone(),
        manifest_repository: config.product.manifest_repository.clone(),
        generated_at: catalog
            .map(|value| value.generated_at.to_rfc3339())
            .unwrap_or_else(|| now.clone()),
        checked_at: now,
        signature_verified: catalog.is_some(),
        public_key_fingerprint: manifest_public_key_fingerprint(),
        source_commit,
        trigger,
    }
}

fn persist_provenance(state: &AppState, provenance: &CatalogProvenance) -> AppResult<()> {
    let path = state
        .paths
        .read()
        .as_ref()
        .map(|paths| paths.catalog_metadata.clone())
        .ok_or_else(|| AppError::Other("catalog metadata path is unavailable".into()))?;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Other("catalog metadata path has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged
        .write_all(&serde_json::to_vec_pretty(provenance).map_err(|error| {
            AppError::Other(format!("serialize catalog provenance: {error}"))
        })?)?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn append_refresh_journal(
    state: &AppState,
    trigger: CatalogRefreshTrigger,
    result: &CatalogRefreshResult,
    duration: std::time::Duration,
    error: Option<String>,
) -> AppResult<()> {
    let actor = match trigger {
        CatalogRefreshTrigger::Automatic => OperationActor::Background,
        CatalogRefreshTrigger::ManualUser => OperationActor::Gui,
    };
    let status = if error.is_some() {
        OperationStatus::Failed
    } else {
        OperationStatus::Succeeded
    };
    let details = BTreeMap::from([
        ("trigger".into(), format!("{trigger:?}")),
        (
            "blocked_by_policy".into(),
            result.blocked_by_policy.to_string(),
        ),
        (
            "signature_verified".into(),
            result.provenance.signature_verified.to_string(),
        ),
        ("added".into(), result.delta.added.to_string()),
        ("updated".into(), result.delta.updated.to_string()),
        ("removed".into(), result.delta.removed.to_string()),
    ]);
    if let Some(journal) = state.journal.read().as_ref() {
        journal.append(&OperationRecord {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            actor,
            kind: OperationKind::CatalogRefresh,
            status,
            target: Some(result.provenance.manifest_repository.clone()),
            summary: if result.blocked_by_policy {
                "Catalog refresh skipped by distribution policy".into()
            } else if result.refreshed {
                "Signed catalog refreshed".into()
            } else {
                "Signed catalog checked; no changes".into()
            },
            details,
            duration_ms: Some(duration.as_millis().min(u128::from(u32::MAX)) as u32),
            backup_id: None,
            error,
        })?;
    }
    Ok(())
}

fn latest_release_versions(catalog: &Catalog) -> BTreeMap<String, u64> {
    let mut versions: BTreeMap<String, u64> = BTreeMap::new();
    for (vendor, families) in &catalog.vendors {
        for (family, entry) in families {
            for release in &entry.releases {
                let key = shas_key(vendor, family, &release.filename);
                versions
                    .entry(key)
                    .and_modify(|current| *current = (*current).max(release.version_packed))
                    .or_insert(release.version_packed);
            }
        }
    }
    versions
}

fn catalog_delta(before: Option<&Catalog>, after: &Catalog) -> CatalogDelta {
    let before = before.map(latest_release_versions).unwrap_or_default();
    let after = latest_release_versions(after);
    CatalogDelta {
        added: after
            .keys()
            .filter(|key| !before.contains_key(*key))
            .count() as u32,
        updated: after
            .iter()
            .filter(|(key, version)| before.get(*key).is_some_and(|old| old != *version))
            .count() as u32,
        removed: before
            .keys()
            .filter(|key| !after.contains_key(*key))
            .count() as u32,
    }
}

const fn empty_delta() -> CatalogDelta {
    CatalogDelta {
        added: 0,
        updated: 0,
        removed: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dll_catalog::{FamilyEntry, Release};

    fn release(filename: &str, version_packed: u64) -> Release {
        Release {
            artifact: None,
            version: version_packed.to_string(),
            version_packed,
            filename: filename.into(),
            sha256: "a".repeat(64),
            size_bytes: 1,
            signed: true,
            released_at: chrono::Utc::now(),
            source: "https://vendor.example/file".into(),
            cdn_url: "https://vendor.example/file".into(),
            release_notes: None,
            signature_subject: None,
            channel: "stable".into(),
            is_dev: false,
            min_driver: None,
            hash_algorithm: "sha256".into(),
            zip_entry: None,
        }
    }

    fn catalog(files: &[(&str, u64)]) -> Catalog {
        Catalog {
            sources: Default::default(),
            schema_version: 1,
            generated_at: chrono::Utc::now(),
            vendors: BTreeMap::from([(
                "nvidia".into(),
                BTreeMap::from([(
                    "dlss_sr".into(),
                    FamilyEntry {
                        latest: files
                            .last()
                            .map(|(_, version)| version.to_string())
                            .unwrap_or_default(),
                        releases: files
                            .iter()
                            .map(|(filename, version)| release(filename, *version))
                            .collect(),
                    },
                )]),
            )]),
            incompatible_games: Vec::new(),
            anticheat: None,
            anti_cheat_binaries: Vec::new(),
        }
    }

    #[test]
    fn delta_distinguishes_added_updated_and_removed_files() {
        let before = catalog(&[("old.dll", 1), ("updated.dll", 1)]);
        let after = catalog(&[("updated.dll", 2), ("new.dll", 1)]);
        assert_eq!(
            catalog_delta(Some(&before), &after),
            CatalogDelta {
                added: 1,
                updated: 1,
                removed: 1,
            }
        );
    }

    fn prior_verified_catalog_state() -> CatalogState {
        CatalogState {
            content_revision: Some("verified-revision".into()),
            generated_at: Some("2026-09-16T12:00:00Z".into()),
            source: CatalogSource::Remote,
            signature_verified: true,
            public_key_fingerprint: Some("verified-key".into()),
            automatic_refresh_enabled: true,
            manual_refresh_enabled: true,
            app_updates_enabled: true,
            last_remote_attempt_at: Some("2026-09-16T13:00:00Z".into()),
            last_remote_success_at: Some("2026-09-16T13:00:00Z".into()),
            last_remote_result: CatalogRemoteResult::Modified,
            error: None,
        }
    }

    #[test]
    fn phase4_not_modified_preserves_verified_generation_and_marks_attempt() {
        let previous = prior_verified_catalog_state();
        let next = catalog_state_after_attempt(
            &previous,
            None,
            None,
            CatalogSource::Remote,
            CatalogRemoteResult::NotModified,
            None,
            "2026-09-17T12:00:00Z",
            true,
            true,
            true,
        );

        assert_eq!(next.content_revision, previous.content_revision);
        assert_eq!(next.generated_at, previous.generated_at);
        assert_eq!(next.source, previous.source);
        assert_eq!(next.signature_verified, previous.signature_verified);
        assert_eq!(next.public_key_fingerprint, previous.public_key_fingerprint);
        assert_eq!(next.last_remote_success_at, previous.last_remote_success_at);
        assert_eq!(
            next.last_remote_attempt_at.as_deref(),
            Some("2026-09-17T12:00:00Z")
        );
        assert_eq!(next.last_remote_result, CatalogRemoteResult::NotModified);
        assert!(next.error.is_none());
    }

    #[test]
    fn phase4_failed_attempt_preserves_verified_catalog_and_marks_failure() {
        let previous = prior_verified_catalog_state();
        let next = catalog_state_after_attempt(
            &previous,
            None,
            None,
            CatalogSource::Cache,
            CatalogRemoteResult::Failed,
            Some("remote verification failed".into()),
            "2026-09-17T12:00:00Z",
            true,
            true,
            true,
        );

        assert_eq!(next.content_revision, previous.content_revision);
        assert_eq!(next.generated_at, previous.generated_at);
        assert_eq!(next.source, previous.source);
        assert_eq!(next.signature_verified, previous.signature_verified);
        assert_eq!(next.public_key_fingerprint, previous.public_key_fingerprint);
        assert_eq!(next.last_remote_success_at, previous.last_remote_success_at);
        assert_eq!(
            next.last_remote_attempt_at.as_deref(),
            Some("2026-09-17T12:00:00Z")
        );
        assert_eq!(next.last_remote_result, CatalogRemoteResult::Failed);
        assert_eq!(
            next.error.as_ref().map(|error| error.message.as_str()),
            Some("remote verification failed")
        );
    }
}
