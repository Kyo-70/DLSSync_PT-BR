use crate::commands::apply_decisions::{
    classify_error, enrich_signature_error, failure_outcome, group_id_for, streamline_block_reason,
    version_major,
};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use backup_store::BackupEntry;
use dll_catalog::{DownloadOptions, DownloadProgress, Release};
use dlssync_contracts::ApplyStage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{mpsc, Semaphore};
use tokio_util::sync::CancellationToken;

pub const EVENT_APPLY_PROGRESS: &str = "apply_progress";
pub const EVENT_DOWNLOAD_PROGRESS: &str = "download_progress";
pub const EVENT_APPLY_INFLIGHT: &str = "apply_inflight";

pub const STAGE_DOWNLOAD: ApplyStage = ApplyStage::Download;
pub const STAGE_VERIFY_SHA: ApplyStage = ApplyStage::VerifySha;
pub const STAGE_VERIFY_SIGNATURE: ApplyStage = ApplyStage::VerifySignature;
pub const STAGE_BACKUP: ApplyStage = ApplyStage::Backup;
pub const STAGE_REPLACE: ApplyStage = ApplyStage::Replace;
pub const STAGE_VERIFY_POST: ApplyStage = ApplyStage::VerifyPost;
pub const STAGE_COMPLETE: ApplyStage = ApplyStage::Complete;
pub const STAGE_FAILED: ApplyStage = ApplyStage::Failed;
pub const STAGE_CANCELLED: ApplyStage = ApplyStage::Cancelled;

#[derive(Debug, Deserialize, Clone, specta::Type)]
pub struct ApplyRequest {
    pub apply_id: String,
    pub game_id: String,
    pub dll_path: String,
    pub vendor: String,
    pub family: String,
    pub target_version: String,
    #[serde(default)]
    pub game_label: Option<String>,
    /// Absolute install root for the game owning this DLL. Used to detect whether
    /// the game is currently running before we waste a download on a locked file.
    /// When absent, the running-game check falls back to the DLL's parent folder.
    #[serde(default)]
    pub install_dir: Option<String>,
    #[serde(default)]
    pub observed_sha256: Option<String>,
}

#[derive(Debug, Deserialize, specta::Type)]
pub struct ApplyBatchRequest {
    pub items: Vec<ApplyRequest>,
    #[serde(default)]
    pub plan: Option<dlssync_contracts::UpdatePlan>,
    #[serde(default)]
    pub actor: Option<dlssync_contracts::OperationActor>,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct ApplyResult {
    pub apply_id: String,
    pub backup_id: String,
    pub previous_version: Option<String>,
    pub new_version: String,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct ApplyBatchResult {
    pub outcomes: Vec<ApplyOutcome>,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct ApplyOutcome {
    pub apply_id: String,
    pub success: bool,
    pub backup_id: Option<String>,
    pub previous_version: Option<String>,
    pub new_version: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ApplyProgress {
    pub apply_id: String,
    pub group_id: String,
    pub stage: ApplyStage,
    pub message: String,
    pub progress: Option<f64>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_class: Option<dlssync_contracts::ApplyErrorClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct GroupDownloadProgress {
    pub group_id: String,
    pub url: String,
    pub bytes_downloaded: u64,
    pub bytes_total: Option<u64>,
    pub bytes_per_sec: f64,
    pub attempt: u32,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct InflightSnapshot {
    pub in_flight: usize,
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn apply_update(
    handle: AppHandle,
    state: State<'_, AppState>,
    request: ApplyRequest,
) -> AppResult<ApplyResult> {
    let registry = state.apply_registry.clone();
    let cancel = registry.register(&request.apply_id);
    emit_inflight(&handle, registry.in_flight());
    let handles = state.inner().clone_handles();
    let result = apply_single_item(&handle, &handles, &request, cancel).await;
    registry.release(&request.apply_id);
    emit_inflight(&handle, registry.in_flight());
    match result {
        Ok(outcome) if outcome.success => Ok(ApplyResult {
            apply_id: outcome.apply_id,
            backup_id: outcome
                .backup_id
                .ok_or_else(|| AppError::Other("missing backup id on success".into()))?,
            previous_version: outcome.previous_version,
            new_version: outcome
                .new_version
                .ok_or_else(|| AppError::Other("missing new version on success".into()))?,
        }),
        Ok(outcome) => Err(AppError::Other(
            outcome.error.unwrap_or_else(|| "apply failed".into()),
        )),
        Err(e) => Err(e),
    }
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn apply_update_batch(
    handle: AppHandle,
    state: State<'_, AppState>,
    request: ApplyBatchRequest,
) -> AppResult<ApplyBatchResult> {
    if request.items.is_empty() {
        return Ok(ApplyBatchResult { outcomes: vec![] });
    }
    let actor = request
        .actor
        .unwrap_or(dlssync_contracts::OperationActor::Gui);
    if !matches!(
        actor,
        dlssync_contracts::OperationActor::Gui | dlssync_contracts::OperationActor::Background
    ) {
        return Err(AppError::Validation(
            "invalid WebView operation actor".into(),
        ));
    }
    // Register before any file/catalog inspection. The UI can request cancellation
    // while those checks are running, before a download task exists.
    let registration =
        BatchRegistration::new(&handle, state.apply_registry.clone(), &request.items);
    let tokens = &registration.tokens;
    if let Some(plan) = &request.plan {
        let catalog = state.catalog.read();
        let catalog = catalog
            .as_ref()
            .ok_or_else(|| AppError::Other("catalog unavailable".into()))?;
        dlssync_application::validate_update_plan(catalog, plan)
            .map_err(|error| AppError::Other(error.to_string()))?;
    }
    let mut by_group: HashMap<String, Vec<ApplyRequest>> = HashMap::new();
    for item in &request.items {
        by_group
            .entry(item.game_id.clone())
            .or_default()
            .push(item.clone());
    }
    let concurrency = state.settings.read().effective_apply_concurrency() as usize;
    let semaphore = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut group_tasks = Vec::with_capacity(by_group.len());
    for (_gid, items) in by_group.into_iter() {
        let reviewed = request
            .plan
            .as_ref()
            .map(|plan| {
                dlssync_application::planning::subset_update_plan(plan, &[items[0].game_id.clone()])
            })
            .transpose()
            .map_err(|error| AppError::Other(error.to_string()))?;
        let handle_c = handle.clone();
        let state_c = state.inner().clone_handles();
        let sem = semaphore.clone();
        let tokens_c = tokens.clone();
        let apply_ids_for_group: Vec<String> = items.iter().map(|r| r.apply_id.clone()).collect();
        let task = tokio::spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            let ids: Vec<_> = items.iter().map(|item| item.apply_id.clone()).collect();
            match apply_item_group(
                &handle_c,
                &state_c,
                &items,
                &tokens_c,
                reviewed.as_ref(),
                actor,
            )
            .await
            {
                Ok(outcomes) => outcomes
                    .into_iter()
                    .map(|outcome| (outcome.apply_id.clone(), Ok(outcome)))
                    .collect::<Vec<(String, AppResult<ApplyOutcome>)>>(),
                Err(error) => ids
                    .into_iter()
                    .map(|id| (id, Err(AppError::Other(error.to_string()))))
                    .collect::<Vec<(String, AppResult<ApplyOutcome>)>>(),
            }
        });
        group_tasks.push((apply_ids_for_group, task));
    }
    let mut outcomes = Vec::with_capacity(request.items.len());
    for (group_apply_ids, task) in group_tasks {
        match task.await {
            Ok(group_outcomes) => {
                for (apply_id, o) in group_outcomes {
                    match o {
                        Ok(out) => outcomes.push(out),
                        Err(e) => outcomes.push(ApplyOutcome {
                            apply_id,
                            success: false,
                            backup_id: None,
                            previous_version: None,
                            new_version: None,
                            error: Some(e.to_string()),
                        }),
                    }
                }
            }
            Err(join_err) => {
                for apply_id in group_apply_ids {
                    outcomes.push(ApplyOutcome {
                        apply_id,
                        success: false,
                        backup_id: None,
                        previous_version: None,
                        new_version: None,
                        error: Some(format!("task join failed: {join_err}")),
                    });
                }
            }
        }
    }
    Ok(ApplyBatchResult { outcomes })
}

pub(crate) struct BatchRegistration {
    handle: AppHandle,
    registry: Arc<crate::state::ApplyRegistry>,
    pub(crate) tokens: HashMap<String, CancellationToken>,
}

impl BatchRegistration {
    pub(crate) fn new(
        handle: &AppHandle,
        registry: Arc<crate::state::ApplyRegistry>,
        items: &[ApplyRequest],
    ) -> Self {
        let tokens = items
            .iter()
            .map(|item| (item.apply_id.clone(), registry.register(&item.apply_id)))
            .collect();
        emit_inflight(handle, registry.in_flight());
        Self {
            handle: handle.clone(),
            registry,
            tokens,
        }
    }
}

impl Drop for BatchRegistration {
    fn drop(&mut self) {
        for id in self.tokens.keys() {
            self.registry.release(id);
        }
        emit_inflight(&self.handle, self.registry.in_flight());
    }
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn cancel_apply(state: State<'_, AppState>, apply_id: String) -> AppResult<bool> {
    Ok(state.apply_registry.cancel(&apply_id))
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn cancel_all_applies(state: State<'_, AppState>) -> AppResult<usize> {
    Ok(state.apply_registry.cancel_all())
}

pub(crate) async fn lookup_release(
    state: &StateHandles,
    request: &ApplyRequest,
) -> AppResult<Release> {
    let guard = state.catalog.read();
    let catalog = guard
        .as_ref()
        .ok_or_else(|| AppError::Other("catalog not loaded".into()))?;
    let filename = std::path::Path::new(&request.dll_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    catalog
        .find_file(
            &request.vendor,
            &request.family,
            &request.target_version,
            filename,
        )
        .ok_or_else(|| {
            AppError::Other(format!(
                "release {}::{}::{} not in catalog",
                request.vendor, request.family, request.target_version
            ))
        })
}

struct PreparedGuiItem {
    file: dlssync_application::execution::PreparedFile,
    request: ApplyRequest,
    group_id: String,
    backup_id: String,
    previous_version: Option<String>,
    new_version: String,
    game_executable: Option<String>,
    cancel: CancellationToken,
    _staging: tempfile::TempDir,
}

pub(crate) async fn apply_single_item(
    handle: &AppHandle,
    state: &StateHandles,
    request: &ApplyRequest,
    cancel: CancellationToken,
) -> AppResult<ApplyOutcome> {
    let tokens = HashMap::from([(request.apply_id.clone(), cancel)]);
    let outcomes = apply_item_group(
        handle,
        state,
        std::slice::from_ref(request),
        &tokens,
        None,
        dlssync_contracts::OperationActor::Gui,
    )
    .await?;
    outcomes
        .into_iter()
        .find(|outcome| outcome.apply_id == request.apply_id)
        .ok_or_else(|| AppError::Other("transaction returned no requested outcome".into()))
}

fn group_failure_progress(request: &ApplyRequest, reason: &str) -> ApplyProgress {
    let cancelled = classify_error(reason) == "cancelled";
    ApplyProgress {
        apply_id: request.apply_id.clone(),
        group_id: request.game_id.clone(),
        stage: if cancelled {
            STAGE_CANCELLED
        } else {
            STAGE_FAILED
        },
        message: if cancelled {
            "Cancelled"
        } else {
            "Update could not finish"
        }
        .into(),
        progress: None,
        error: Some(reason.to_string()),
        error_class: Some(
            serde_json::from_value(serde_json::json!(classify_error(reason)))
                .unwrap_or(dlssync_contracts::ApplyErrorClass::Other),
        ),
        attempt: None,
    }
}

fn group_failure(handle: &AppHandle, requests: &[ApplyRequest], reason: &str) -> Vec<ApplyOutcome> {
    requests
        .iter()
        .map(|request| {
            let _ = handle.emit(
                EVENT_APPLY_PROGRESS,
                group_failure_progress(request, reason),
            );
            failure_outcome(request, &request.game_id, reason.to_string())
        })
        .collect()
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn preview_update_plan(
    state: State<'_, AppState>,
    items: Vec<ApplyRequest>,
    baseline: Option<dlssync_contracts::UpdatePlan>,
) -> AppResult<dlssync_contracts::UpdatePlan> {
    if let Some(plan) = baseline {
        let catalog = state.catalog.read();
        let catalog = catalog
            .as_ref()
            .ok_or_else(|| AppError::Other("catalog unavailable".into()))?;
        dlssync_application::validate_update_plan(catalog, &plan)
            .map_err(|error| AppError::Other(error.to_string()))?;
    }
    let (_, plan, _) = plan_for_requests(&state.inner().clone_handles(), &items).await?;
    Ok(plan)
}

async fn plan_for_requests(
    state: &StateHandles,
    requests: &[ApplyRequest],
) -> AppResult<(dll_catalog::Catalog, dlssync_contracts::UpdatePlan, PathBuf)> {
    let catalog = state
        .catalog
        .read()
        .clone()
        .ok_or_else(|| AppError::Other("catalog unavailable".into()))?;
    let root = state
        .backups
        .read()
        .as_ref()
        .map(|store| store.root_dir.clone())
        .ok_or_else(|| AppError::Other("backup store unavailable".into()))?;
    let mut games = Vec::new();
    for request in requests {
        if games
            .iter()
            .any(|game: &dlssync_contracts::ScannedGame| game.id == request.game_id)
        {
            continue;
        }
        let install_root = request
            .install_dir
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| {
                PathBuf::from(&request.dll_path)
                    .parent()
                    .map(std::path::Path::to_path_buf)
            })
            .ok_or_else(|| AppError::Other("game root missing".into()))?;
        crate::paths::PathGuard::assert_safe_scan_dir(&install_root)
            .map_err(|error| AppError::Validation(error.to_string()))?;
        let mut game =
            tokio::task::spawn_blocking(move || dlssync_application::scan_path(&install_root))
                .await
                .map_err(|error| AppError::Other(error.to_string()))?
                .map_err(|error| AppError::Other(error.to_string()))?;
        game.id = request.game_id.clone();
        if let Some(label) = &request.game_label {
            game.name = label.clone();
        }
        games.push(game);
    }
    let mut items = Vec::with_capacity(requests.len());
    for request in requests {
        let path = PathBuf::from(&request.dll_path)
            .canonicalize()
            .map_err(|error| AppError::Other(error.to_string()))?;
        let game = games
            .iter()
            .find(|game| game.id == request.game_id)
            .ok_or_else(|| AppError::Other("game missing from plan".into()))?;
        let component = game
            .components
            .iter()
            .find(|component| {
                PathBuf::from(&component.path).canonicalize().ok().as_ref() == Some(&path)
            })
            .ok_or_else(|| {
                AppError::Other(format!(
                    "requested DLL not observed in game: {}",
                    path.display()
                ))
            })?;
        if component.family != request.family {
            return Err(AppError::Other(
                "requested DLL family differs from observation".into(),
            ));
        }
        if request.observed_sha256.as_ref().is_some_and(|expected| {
            component
                .sha256
                .as_ref()
                .is_none_or(|observed| !observed.eq_ignore_ascii_case(expected))
        }) {
            return Err(AppError::Other(
                "installed bytes changed since review; scan again".into(),
            ));
        }
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| AppError::Other("invalid DLL filename".into()))?;
        let release = catalog
            .find_file(
                &request.vendor,
                &request.family,
                &request.target_version,
                filename,
            )
            .ok_or_else(|| AppError::Other("requested catalog artifact missing".into()))?;
        items.push(dlssync_contracts::UpdatePlanItem {
            id: request.apply_id.clone(),
            game_id: game.id.clone(),
            game_name: game.name.clone(),
            dll_path: component.path.clone(),
            family: component.family.clone(),
            current_version: component.current_version.clone(),
            target_version: release.version,
            backup_path: String::new(),
            selected: true,
            trust: dlssync_contracts::TrustEvidence {
                source_url: release.cdn_url,
                expected_sha256: release.sha256,
                observed_sha256: component.sha256.clone(),
                signature_subject: release.signature_subject,
                signature_verified: release.signed,
                anti_cheat_risk: None,
            },
        });
    }
    let plan = dlssync_application::build_verified_update_plan(&catalog, &games, items, &root)
        .map_err(|error| AppError::Other(error.to_string()))?;
    Ok((catalog, plan, root))
}

pub(crate) async fn apply_item_group(
    handle: &AppHandle,
    state: &StateHandles,
    requests: &[ApplyRequest],
    tokens: &HashMap<String, CancellationToken>,
    reviewed: Option<&dlssync_contracts::UpdatePlan>,
    actor: dlssync_contracts::OperationActor,
) -> AppResult<Vec<ApplyOutcome>> {
    if requests.is_empty() {
        return Ok(vec![]);
    }
    let planned = if let Some(plan) = reviewed {
        let catalog = state
            .catalog
            .read()
            .clone()
            .ok_or_else(|| AppError::Other("catalog unavailable".into()))?;
        let root = state
            .backups
            .read()
            .as_ref()
            .map(|store| store.root_dir.clone())
            .ok_or_else(|| AppError::Other("backup store unavailable".into()))?;
        dlssync_application::validate_update_plan(&catalog, plan)
            .map_err(|error| AppError::Other(error.to_string()))?;
        for request in requests {
            let path = PathBuf::from(&request.dll_path)
                .canonicalize()
                .map_err(|error| AppError::Other(error.to_string()))?;
            if !plan.items.iter().any(|item| {
                std::path::Path::new(&item.dll_path) == path
                    && item.game_id == request.game_id
                    && item.target_version == request.target_version
                    && item.family == request.family
            }) {
                return Ok(group_failure(
                    handle,
                    requests,
                    "request differs from reviewed plan",
                ));
            }
        }
        for change in plan
            .changes
            .iter()
            .filter(|change| !change.added_as_dependency)
        {
            if !requests.iter().any(|request| {
                PathBuf::from(&request.dll_path).canonicalize().ok()
                    == Some(PathBuf::from(&change.precondition.absolute_path))
            }) {
                return Ok(group_failure(
                    handle,
                    requests,
                    "reviewed plan includes an unrequested member",
                ));
            }
        }
        Ok((catalog, plan.clone(), root))
    } else {
        plan_for_requests(state, requests).await
    };
    let (catalog, plan, root) = match planned {
        Ok(plan) => plan,
        Err(error) => return Ok(group_failure(handle, requests, &error.to_string())),
    };
    let hardware = crate::system_info::collect();
    let policy = dlssync_application::policy::ApplyPolicy {
        fsr4_capable: hardware.gpus.iter().any(|gpu| gpu.fsr4_capable),
        allow_streamline: state.settings.read().update_prefs.update_streamline,
    };
    if let Err(error) = validate_batch_policy(&plan, &policy)
        .and_then(|()| dlssync_application::policy::validate_plan_backups(&root, &plan))
    {
        return Ok(group_failure(handle, requests, &error.to_string()));
    }
    let game_roots = dlssync_application::transaction::plan_game_roots(&plan)
        .map_err(|error| AppError::Other(error.to_string()))?;
    let _preparation_lock = match dlssync_application::transaction::lock_preparation(&game_roots) {
        Ok(lock) => lock,
        Err(error) => return Ok(group_failure(handle, requests, &error.to_string())),
    };
    let mut expanded = Vec::with_capacity(plan.items.len());
    let mut expanded_tokens = cancellation_tokens_for_group(requests, tokens);
    for item in &plan.items {
        let source = requests.iter().find(|request| {
            PathBuf::from(&request.dll_path).canonicalize().ok()
                == Some(PathBuf::from(&item.dll_path))
        });
        let template = source
            .or_else(|| {
                requests
                    .iter()
                    .find(|request| request.game_id == item.game_id)
            })
            .ok_or_else(|| AppError::Other("dependency has no game request".into()))?;
        let mut request = template.clone();
        if source.is_none() {
            request.apply_id = uuid::Uuid::new_v4().to_string();
            if let Some(token) = tokens.get(&template.apply_id) {
                expanded_tokens.insert(request.apply_id.clone(), token.clone());
            }
        }
        request.dll_path = item.dll_path.clone();
        request.family = item.family.clone();
        request.target_version = item.target_version.clone();
        request.vendor = dll_scanner::family_vendor(&item.family)
            .ok_or_else(|| {
                AppError::Validation(format!("unknown component family: {}", item.family))
            })?
            .into();
        expanded.push(request);
    }
    let mut prepared = Vec::with_capacity(expanded.len());
    for (request, item) in expanded.iter().zip(&plan.items) {
        if expanded_tokens
            .values()
            .any(CancellationToken::is_cancelled)
        {
            return Ok(group_failure(handle, &expanded, "cancelled"));
        }
        let cancel = expanded_tokens
            .get(&request.apply_id)
            .cloned()
            .unwrap_or_default();
        let backup_path =
            dlssync_application::policy::derived_backup_destination(&root, &plan, item)
                .map_err(|error| AppError::Other(error.to_string()))?;
        match prepare_single_item(handle, state, request, cancel, item, backup_path).await {
            Ok(file) => prepared.push(file),
            Err(outcome) => {
                return Ok(group_failure(
                    handle,
                    &expanded,
                    outcome.error.as_deref().unwrap_or("preparation failed"),
                ))
            }
        }
    }
    let files: Vec<_> = prepared.iter().map(|item| item.file.clone()).collect();
    for item in &prepared {
        let ctx = StageContext {
            handle: handle.clone(),
            apply_id: item.request.apply_id.clone(),
            group_id: item.group_id.clone(),
        };
        ctx.stage(STAGE_REPLACE, "Installing updates", None, None);
    }
    let result = (|| -> Result<usize, dlssync_application::ExecutionError> {
        let current = state.catalog.read().clone().ok_or_else(|| {
            dlssync_application::ExecutionError::Stale("catalog unavailable".into())
        })?;
        dlssync_application::validate_update_plan(&current, &plan)?;
        dlssync_application::execution::validate_prepared_plan(&plan, &files)?;
        let store_guard = state.backups.read();
        let store = store_guard.as_ref().ok_or_else(|| {
            dlssync_application::ExecutionError::Integrity("backup store unavailable".into())
        })?;
        let journal = dlssync_application::transaction::recovery_journal(store)?;
        let mut game_ids: Vec<String> = expanded
            .iter()
            .map(|request| request.game_id.clone())
            .collect();
        game_ids.sort();
        game_ids.dedup();
        let operation_id = plan.id.clone();
        let scoped_game_id = (game_ids.len() == 1).then(|| game_ids[0].clone());
        dlssync_application::transaction::apply_durable_observed(
            &journal,
            &root,
            &plan.id,
            actor,
            &files,
            |file| {
                if prepared.iter().any(|item| item.cancel.is_cancelled()) {
                    return Err(dlssync_application::ExecutionError::Cancelled);
                }
                if state.catalog.read().as_ref().is_none_or(|current| {
                    dlssync_application::planning::catalog_revision(current)
                        != dlssync_application::planning::catalog_revision(&catalog)
                }) {
                    return Err(dlssync_application::ExecutionError::Stale(
                        "catalog changed during transaction".into(),
                    ));
                }
                pe_version::require_x64_dll_pair(&file.target, &file.staged)?;
                if let Some(item) = prepared.iter().find(|item| item.file.target == file.target) {
                    if let Some(executable) = &item.game_executable {
                        pe_version::require_x64_executable(std::path::Path::new(executable))?;
                    }
                }
                Ok(())
            },
            |_, stage| {
                let operation = dlssync_contracts::OperationSnapshot {
                    id: operation_id.clone(),
                    plan_id: plan.id.clone(),
                    actor,
                    kind: dlssync_contracts::OperationKind::DllApply,
                    game_ids: game_ids.clone(),
                    parent_operation_id: None,
                    started_at: Some(plan.created_at.clone()),
                    sequence: dlssync_contracts::Counter::default(),
                    stage,
                    progress: dlssync_contracts::MeasuredProgress {
                        files_verified: prepared
                            .iter()
                            .filter(|item| {
                                dll_catalog::hex_sha256_file(&item.file.target).is_ok_and(|hash| {
                                    hash.eq_ignore_ascii_case(&item.file.expected_sha256)
                                })
                            })
                            .count() as u32,
                        files_total: Some(prepared.len() as u32),
                        measurement_basis: dlssync_contracts::MeasurementBasis::VerifiedFiles,
                        ..dlssync_contracts::MeasuredProgress::default()
                    },
                    results: Vec::new(),
                    cancel_requested: prepared.iter().any(|item| item.cancel.is_cancelled()),
                    error: None,
                    state_revision: String::new(),
                    updated_at: chrono::Utc::now().to_rfc3339(),
                };
                let receipt = state
                    .authoritative_state
                    .commit(dlssync_application::state::StateCommit {
                        operation_id: Some(operation_id.clone()),
                        game_id: scoped_game_id.clone(),
                        delta: dlssync_contracts::StateDelta {
                            affected_game_ids: game_ids.clone(),
                            operations: vec![operation],
                            ..dlssync_contracts::StateDelta::default()
                        },
                    })
                    .map_err(|error| error.to_string())?;
                receipt.delivery_error.map_or(Ok(()), Err)
            },
        )
    })();
    if let Err(error) = result {
        if let Some(store) = state.backups.read().as_ref() {
            if let Err(sync_error) =
                dlssync_application::transaction::sync_restore_verifications(store)
            {
                tracing::warn!(error = %sync_error, "restore verification projection after failed apply was incomplete");
            }
        }
        refresh_apply_persisted_state(state);
        return Ok(group_failure(handle, &expanded, &error.to_string()));
    }
    let catalog = state.catalog.read().clone();
    let previous_games = state.authoritative_state.snapshot().games;
    let mut post_apply_games: HashMap<String, (String, PathBuf)> = HashMap::new();
    for request in &expanded {
        let root = request
            .install_dir
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| PathBuf::from(&request.dll_path).parent().map(PathBuf::from));
        if let Some(root) = root {
            post_apply_games
                .entry(request.game_id.clone())
                .or_insert_with(|| {
                    (
                        request
                            .game_label
                            .clone()
                            .unwrap_or_else(|| request.game_id.clone()),
                        root,
                    )
                });
        }
    }
    for (game_id, (name, install_dir)) in post_apply_games {
        let _projection_guard = dlssync_application::scan::projection_guard();
        let ticket = state
            .authoritative_state
            .begin_observation(dlssync_application::scan::GAME_PROJECTION_SCOPE);
        let launcher = previous_games
            .iter()
            .find(|game| game.id == game_id)
            .and_then(|game| game.launcher.as_deref())
            .and_then(|value| {
                serde_json::from_value::<launcher_scan::LauncherKind>(serde_json::Value::String(
                    value.to_string(),
                ))
                .ok()
            })
            .unwrap_or(launcher_scan::LauncherKind::Manual);
        let detected = launcher_scan::DetectedGame {
            id: game_id.clone(),
            name,
            launcher,
            install_dir,
            app_id: None,
            native_ids: Default::default(),
            art: Default::default(),
            image_url: None,
            size_bytes: None,
        };
        let snapshot = dlssync_application::scan::observe_game_snapshot(
            &detected,
            catalog.as_ref(),
            &crate::commands::settings::projection_settings(&state.settings.read(), &game_id),
        );
        match state.authoritative_state.commit_observation(
            ticket,
            dlssync_application::state::StateCommit {
                game_id: Some(game_id.clone()),
                delta: dlssync_contracts::StateDelta {
                    affected_game_ids: vec![game_id],
                    games: vec![snapshot],
                    ..dlssync_contracts::StateDelta::default()
                },
                ..dlssync_application::state::StateCommit::default()
            },
        ) {
            Ok(receipt) => {
                if let Some(error) = receipt.delivery_error {
                    tracing::warn!(%error, "post-apply observation event delivery failed");
                }
            }
            Err(error) => tracing::warn!(%error, "post-apply observation commit was stale"),
        }
    }
    refresh_apply_persisted_state(state);
    Ok(prepared
        .into_iter()
        .map(|item| {
            let ctx = StageContext {
                handle: handle.clone(),
                apply_id: item.request.apply_id.clone(),
                group_id: item.group_id,
            };
            ctx.stage(
                STAGE_VERIFY_POST,
                &format!("Installed version: {}", item.new_version),
                None,
                None,
            );
            ctx.stage(STAGE_COMPLETE, "Update installed", Some(1.0), None);
            ApplyOutcome {
                apply_id: item.request.apply_id,
                success: true,
                backup_id: Some(item.backup_id),
                previous_version: item.previous_version,
                new_version: Some(item.new_version),
                error: None,
            }
        })
        .collect())
}

fn validate_batch_policy(
    plan: &dlssync_contracts::UpdatePlan,
    policy: &dlssync_application::policy::ApplyPolicy,
) -> Result<(), dlssync_application::ExecutionError> {
    dlssync_application::policy::evaluate_plan(plan, policy)
}

fn cancellation_tokens_for_group(
    requests: &[ApplyRequest],
    tokens: &HashMap<String, CancellationToken>,
) -> HashMap<String, CancellationToken> {
    requests
        .iter()
        .filter_map(|request| {
            tokens
                .get(&request.apply_id)
                .map(|token| (request.apply_id.clone(), token.clone()))
        })
        .collect()
}

async fn prepare_single_item(
    handle: &AppHandle,
    state: &StateHandles,
    request: &ApplyRequest,
    cancel: CancellationToken,
    plan_item: &dlssync_contracts::UpdatePlanItem,
    backup_path: PathBuf,
) -> Result<PreparedGuiItem, Box<ApplyOutcome>> {
    let failure_outcome = |request: &ApplyRequest, group_id: &str, error: String| {
        Box::new(failure_outcome(request, group_id, error))
    };
    let release_result = {
        let catalog_guard = state.catalog.read();
        match catalog_guard.as_ref() {
            Some(catalog) => {
                dlssync_application::planning::resolve_planned_release(catalog, plan_item)
            }
            None => Err(dlssync_application::ExecutionError::Stale(
                "catalog unavailable".into(),
            )),
        }
    };
    let release = match release_result {
        Ok(release) => release,
        Err(error) => return Err(failure_outcome(request, "_", error.to_string())),
    };
    let group_id = group_id_for(&release.cdn_url);
    let ctx = StageContext {
        handle: handle.clone(),
        apply_id: request.apply_id.clone(),
        group_id: group_id.clone(),
    };

    if let Some(minimum) = release.min_driver.as_deref() {
        let installed = crate::system_info::collect()
            .gpus
            .into_iter()
            .find(|gpu| gpu_vendor_matches(gpu.vendor, &request.vendor))
            .map(|gpu| gpu.driver_version);
        if !installed
            .as_deref()
            .is_some_and(|version| driver_meets_minimum(&request.vendor, version, minimum))
        {
            let installed = installed.unwrap_or_else(|| "unknown".into());
            let reason = format!(
                "{} {} requires {} driver {minimum} or newer; installed: {installed}",
                request.vendor, release.version, request.vendor
            );
            ctx.fail(
                "Driver requirement not met",
                reason.clone(),
                Some("driver_too_old"),
            );
            return Err(failure_outcome(request, &group_id, reason));
        }
    }

    let dll_path = PathBuf::from(&request.dll_path);
    if let Err(guard_err) = crate::paths::PathGuard::assert_dll_ext(&dll_path)
        .and_then(|()| crate::paths::PathGuard::deny_system_dir(&dll_path))
    {
        let reason = guard_err.to_string();
        ctx.fail("Unsafe target path", reason.clone(), Some("permission"));
        return Err(failure_outcome(request, &group_id, reason));
    }
    if !dll_path.exists() {
        ctx.fail(
            "DLL file disappeared",
            "missing".to_string(),
            Some("missing"),
        );
        return Err(failure_outcome(
            request,
            &group_id,
            format!("dll not found: {}", dll_path.display()),
        ));
    }

    if let Some(reason) = streamline_guard(state, &dll_path, &request.target_version) {
        ctx.fail(
            "Streamline plugin skipped",
            reason.clone(),
            Some("streamline_locked"),
        );
        return Err(failure_outcome(request, &group_id, reason));
    }
    if let Err(reason) = ensure_writable(&dll_path) {
        let class = if reason.contains("locked") {
            "lock"
        } else if reason.contains("access denied") {
            "permission"
        } else {
            "other"
        };
        ctx.fail("DLL is locked", reason.clone(), Some(class));
        return Err(failure_outcome(request, &group_id, reason));
    }

    let game_root = request
        .install_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| dll_path.parent().map(PathBuf::from).unwrap_or_default());
    if let Some(running_exe) = detect_running_game(&game_root) {
        let label = request
            .game_label
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&request.game_id);
        let err = format!("Close {label} before updating its DLLs (running: {running_exe})");
        ctx.fail("Game is running", err.clone(), Some("game_running"));
        return Err(failure_outcome(request, &group_id, err));
    }

    let game_executable =
        match super::dlss_profile::find_game_executable(game_root.to_string_lossy().into_owned())
            .await
        {
            Ok(executable) => executable,
            Err(error) => {
                ctx.fail(
                    "Game executable could not be inspected",
                    error.to_string(),
                    Some("architecture"),
                );
                return Err(failure_outcome(request, &group_id, error.to_string()));
            }
        };
    if let Some(executable) = &game_executable {
        if let Err(error) = pe_version::require_x64_executable(std::path::Path::new(executable)) {
            ctx.fail(
                "Incompatible game architecture",
                error.to_string(),
                Some("architecture"),
            );
            return Err(failure_outcome(request, &group_id, error.to_string()));
        }
    }
    let backup_root = match state.backups.read().as_ref().map(|s| s.root_dir.clone()) {
        Some(p) => p,
        None => {
            let err = "backup store not initialized".to_string();
            ctx.fail(&err, err.clone(), Some("backup"));
            return Err(failure_outcome(request, &group_id, err));
        }
    };

    ctx.stage(
        STAGE_DOWNLOAD,
        &format!("Downloading {} v{}", release.filename, release.version),
        Some(0.0),
        None,
    );
    let staging = match tempfile::tempdir_in(&backup_root) {
        Ok(t) => t,
        Err(e) => {
            ctx.fail("Staging dir failed", e.to_string(), Some("other"));
            return Err(failure_outcome(request, &group_id, e.to_string()));
        }
    };
    let staged_dll =
        match stage_download(state, &release, staging.path(), &ctx, cancel.clone()).await {
            Ok(p) => p,
            Err(err) => {
                let class = classify_error(&err);
                ctx.fail("Download failed", err.clone(), Some(class));
                return Err(failure_outcome(request, &group_id, err));
            }
        };

    if let Err(error) = pe_version::require_x64_dll_pair(&dll_path, &staged_dll) {
        ctx.fail(
            "Incompatible binary architecture",
            error.to_string(),
            Some("architecture"),
        );
        return Err(failure_outcome(request, &group_id, error.to_string()));
    }

    let algo = dll_catalog::HashAlgo::from_hex_len(&release.sha256)
        .unwrap_or(dll_catalog::HashAlgo::Sha256);
    let algo_label = match algo {
        dll_catalog::HashAlgo::Sha256 => "SHA-256",
        dll_catalog::HashAlgo::Md5 => "MD5",
    };
    ctx.stage(
        STAGE_VERIFY_SHA,
        &format!("Verifying {algo_label}"),
        None,
        None,
    );
    let new_hash = match tokio::task::spawn_blocking({
        let staged = staged_dll.clone();
        move || dll_catalog::hash_file_with(&staged, algo)
    })
    .await
    {
        Ok(Ok(h)) => h,
        Ok(Err(e)) => {
            ctx.fail("Hash failed", e.to_string(), Some("hash"));
            return Err(failure_outcome(request, &group_id, e.to_string()));
        }
        Err(e) => {
            ctx.fail("Hash task failed", e.to_string(), Some("other"));
            return Err(failure_outcome(request, &group_id, e.to_string()));
        }
    };
    if !new_hash.eq_ignore_ascii_case(&release.sha256) {
        let err = format!(
            "{algo_label} mismatch: expected {} got {}",
            release.sha256, new_hash
        );
        ctx.fail("Integrity check failed", err.clone(), Some("hash"));
        return Err(failure_outcome(request, &group_id, err));
    }
    ctx.stage(STAGE_VERIFY_SHA, &format!("{algo_label} OK"), None, None);

    let expected_sha256 = match dll_catalog::hex_sha256_file(&staged_dll) {
        Ok(hash) => hash,
        Err(error) => {
            ctx.fail("Staged hash unreadable", error.to_string(), Some("hash"));
            return Err(failure_outcome(request, &group_id, error.to_string()));
        }
    };

    let allow_unsigned = state.settings.read().advanced.allow_unsigned_dlls;
    ctx.stage(
        STAGE_VERIFY_SIGNATURE,
        "Reading Authenticode signature",
        None,
        None,
    );
    let auth_info = match tokio::task::spawn_blocking({
        let staged = staged_dll.clone();
        move || pe_version::read_authenticode(&staged)
    })
    .await
    {
        Ok(info) => info,
        Err(e) => {
            ctx.fail("Signature task failed", e.to_string(), Some("other"));
            return Err(failure_outcome(request, &group_id, e.to_string()));
        }
    };
    match auth_info {
        Some(info) => match pe_version::enforce_subject(&info, &request.vendor) {
            Ok(()) if !info.trusted && !allow_unsigned => {
                // The subject CN matched the vendor allowlist, but WinVerifyTrust
                // rejected the chain — the binary's digest was never cryptographically
                // verified, so a self-signed DLL bearing "NVIDIA Corporation" would
                // otherwise slip through. Fail closed unless the user opted into
                // unsigned mode.
                let err = format!(
                    "Authenticode chain is untrusted for {} — refusing to apply. \
                     Enable 'Allow unsigned DLLs' in Settings → Advanced to override.",
                    info.subject_cn.as_deref().unwrap_or("?")
                );
                ctx.fail("Untrusted signature chain", err.clone(), Some("signature"));
                return Err(failure_outcome(request, &group_id, err));
            }
            Ok(()) => {
                let trust_tag = if !info.trusted {
                    "untrusted-chain (allowed)"
                } else if info.revocation_bypassed {
                    tracing::warn!(
                        subject = ?info.subject_cn,
                        dll = %dll_path.display(),
                        "applying with revocation status unverifiable — chain trusted without a revocation check"
                    );
                    "trusted (revocation unchecked)"
                } else {
                    "trusted"
                };
                ctx.stage(
                    STAGE_VERIFY_SIGNATURE,
                    &format!(
                        "Signed by {} ({trust_tag})",
                        info.subject_cn.as_deref().unwrap_or("?")
                    ),
                    None,
                    None,
                );
            }
            Err(reason) if allow_unsigned => {
                tracing::warn!("signature gate bypassed (allow_unsigned_dlls=true): {reason}");
                ctx.stage(
                    STAGE_VERIFY_SIGNATURE,
                    &format!("Signature mismatch ignored: {reason}"),
                    None,
                    None,
                );
            }
            Err(reason) => {
                let with_hint = enrich_signature_error(&reason);
                ctx.fail("Signature rejected", with_hint.clone(), Some("signature"));
                return Err(failure_outcome(request, &group_id, with_hint));
            }
        },
        None if allow_unsigned => {
            ctx.stage(
                STAGE_VERIFY_SIGNATURE,
                "No signature data (unsigned mode enabled)",
                None,
                None,
            );
        }
        None => {
            let err = "Authenticode signature could not be read — try enabling \
                       'Allow unsigned DLLs' in Settings → Advanced if this vendor \
                       ships unsigned binaries"
                .to_string();
            ctx.fail("Signature unreadable", err.clone(), Some("signature"));
            return Err(failure_outcome(request, &group_id, err));
        }
    }

    if cancel.is_cancelled() {
        ctx.cancelled();
        return Err(failure_outcome(request, &group_id, "cancelled".into()));
    }

    let previous_sha = match tokio::task::spawn_blocking({
        let dll_path = dll_path.clone();
        move || dll_catalog::hex_sha256_file(&dll_path)
    })
    .await
    {
        Ok(Ok(h)) => h,
        Ok(Err(e)) => {
            ctx.fail("Hash old DLL failed", e.to_string(), Some("hash"));
            return Err(failure_outcome(request, &group_id, e.to_string()));
        }
        Err(e) => {
            ctx.fail("Hash task failed", e.to_string(), Some("other"));
            return Err(failure_outcome(request, &group_id, e.to_string()));
        }
    };
    let previous_version = tokio::task::spawn_blocking({
        let dll_path = dll_path.clone();
        move || pe_version::read_dll_version(&dll_path).ok()
    })
    .await
    .ok()
    .flatten()
    .map(|v| v.file_version);

    ctx.stage(STAGE_BACKUP, "Backing up current DLL", None, None);
    let entry_id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now();
    let filename = dll_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown.dll")
        .to_string();
    if plan_item.trust.observed_sha256.as_deref() != Some(previous_sha.as_str()) {
        let message = "installed bytes changed since plan creation".to_string();
        ctx.fail("Stale plan", message.clone(), Some("hash"));
        return Err(failure_outcome(request, &group_id, message));
    }
    if cancel.is_cancelled() {
        ctx.cancelled();
        return Err(failure_outcome(request, &group_id, "cancelled".into()));
    }
    let copy_src = dll_path.clone();
    let copy_dst = backup_path.clone();
    let copy_expected = previous_sha.clone();
    let backup_space = std::fs::metadata(&dll_path)
        .map_err(dll_catalog::CatalogError::from)
        .and_then(|metadata| dll_catalog::ensure_available_space(&backup_path, metadata.len()));
    if let Err(error) = backup_space {
        ctx.fail(
            "Backup storage unavailable",
            error.to_string(),
            Some("backup"),
        );
        return Err(failure_outcome(request, &group_id, error.to_string()));
    }
    match tokio::task::spawn_blocking(move || {
        dlssync_application::policy::create_contained_backup(
            &backup_root,
            &copy_src,
            &copy_dst,
            &copy_expected,
        )
    })
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            ctx.fail("Backup copy failed", e.to_string(), Some("backup"));
            return Err(failure_outcome(request, &group_id, e.to_string()));
        }
        Err(e) => {
            ctx.fail("Backup copy task failed", e.to_string(), Some("other"));
            return Err(failure_outcome(request, &group_id, e.to_string()));
        }
    }
    let entry = BackupEntry {
        id: entry_id.clone(),
        game_id: request.game_id.clone(),
        dll_family: request.family.clone(),
        dll_filename: filename.clone(),
        original_path: dll_path.clone(),
        backup_path: backup_path.clone(),
        previous_version: previous_version.clone(),
        previous_sha256: Some(previous_sha.clone()),
        created_at,
        restored_at: None,
        size_bytes: std::fs::metadata(&backup_path).ok().map(|m| m.len()),
        backup_type: "dll".to_string(),
        device_class: None,
        hardware_id: None,
        driver_provider: None,
    };
    if let Err(e) = state
        .backups
        .read()
        .as_ref()
        .map(|store| store.insert(&entry))
        .transpose()
    {
        ctx.fail("Backup insert failed", e.to_string(), Some("backup"));
        return Err(failure_outcome(request, &group_id, e.to_string()));
    }
    ctx.stage(STAGE_BACKUP, "Backup created", None, None);

    if let Ok(meta) = std::fs::symlink_metadata(&dll_path) {
        if meta.file_type().is_symlink() {
            let err = format!("refusing to replace symlink: {}", dll_path.display());
            ctx.fail("Symlink detected", err.clone(), Some("permission"));
            return Err(failure_outcome(request, &group_id, err));
        }
    }
    if cancel.is_cancelled() {
        ctx.cancelled();
        return Err(failure_outcome(request, &group_id, "cancelled".into()));
    }
    let identity_check = pe_version::require_x64_dll_pair(&dll_path, &staged_dll).and_then(|()| {
        game_executable
            .as_ref()
            .map(|exe| pe_version::require_x64_executable(std::path::Path::new(exe)))
            .transpose()
            .map(|_| ())
    });
    if let Err(error) = identity_check {
        ctx.fail(
            "Binary architecture changed before installation",
            error.to_string(),
            Some("architecture"),
        );
        return Err(failure_outcome(request, &group_id, error.to_string()));
    }
    let target_space = std::fs::metadata(&staged_dll)
        .map_err(dll_catalog::CatalogError::from)
        .and_then(|metadata| dll_catalog::ensure_available_space(&dll_path, metadata.len()));
    if let Err(error) = target_space {
        ctx.fail(
            "Game storage unavailable",
            error.to_string(),
            Some("backup"),
        );
        return Err(failure_outcome(request, &group_id, error.to_string()));
    }
    let new_version = match pe_version::read_dll_version(&staged_dll) {
        Ok(version) => version.file_version,
        Err(error) => {
            let message = format!("candidate version unreadable: {error}");
            ctx.fail(
                "Candidate identity unreadable",
                message.clone(),
                Some("hash"),
            );
            return Err(failure_outcome(request, &group_id, message));
        }
    };
    Ok(PreparedGuiItem {
        file: dlssync_application::execution::PreparedFile {
            target: dll_path,
            staged: staged_dll,
            backup: backup_path,
            previous_sha256: previous_sha,
            expected_sha256,
            expected_version: Some(new_version.clone()),
        },
        request: request.clone(),
        group_id,
        backup_id: entry_id,
        previous_version,
        new_version,
        game_executable,
        cancel,
        _staging: staging,
    })
}

async fn stage_download(
    state: &StateHandles,
    release: &Release,
    staging_dir: &std::path::Path,
    ctx: &StageContext,
    cancel: CancellationToken,
) -> Result<PathBuf, String> {
    let _download_slot = tokio::select! {
        permit = dlssync_application::transaction::acquire_download_slot() => permit,
        () = cancel.cancelled() => return Err("cancelled".into()),
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<DownloadProgress>();
    let net = state.settings.read().network.clone();
    let opts = DownloadOptions {
        max_retries: net.retry_attempts.max(1),
        chunk_timeout: Duration::from_secs(net.chunk_timeout_secs.max(5)),
        progress_tx: Some(tx),
        cancel: Some(cancel.clone()),
        ..Default::default()
    };
    let pump_handle = ctx.handle.clone();
    let pump_group = ctx.group_id.clone();
    let pump_url = release.cdn_url.clone();
    let pump = tokio::spawn(async move {
        while let Some(p) = rx.recv().await {
            let _ = pump_handle.emit(
                EVENT_DOWNLOAD_PROGRESS,
                GroupDownloadProgress {
                    group_id: pump_group.clone(),
                    url: pump_url.clone(),
                    bytes_downloaded: p.bytes_downloaded,
                    bytes_total: p.bytes_total,
                    bytes_per_sec: p.bytes_per_sec,
                    attempt: p.attempt,
                },
            );
        }
    });

    let client = state.http_downloads.read().clone();
    let result = dll_catalog::download_and_extract_dll_cached(
        &state.download_cache,
        &client,
        release,
        staging_dir,
        opts,
    )
    .await;

    drop(pump);
    match result {
        Ok(path) => {
            ctx.stage(STAGE_DOWNLOAD, "Downloaded", Some(1.0), None);
            Ok(path)
        }
        Err(e) => Err(e.to_string()),
    }
}

struct StageContext {
    handle: AppHandle,
    apply_id: String,
    group_id: String,
}

impl StageContext {
    fn stage(&self, stage: ApplyStage, message: &str, progress: Option<f64>, attempt: Option<u32>) {
        let _ = self.handle.emit(
            EVENT_APPLY_PROGRESS,
            ApplyProgress {
                apply_id: self.apply_id.clone(),
                group_id: self.group_id.clone(),
                stage,
                message: message.to_string(),
                progress,
                error: None,
                error_class: None,
                attempt,
            },
        );
    }

    fn fail(&self, message: &str, error: String, error_class: Option<&str>) {
        let _ = self.handle.emit(
            EVENT_APPLY_PROGRESS,
            ApplyProgress {
                apply_id: self.apply_id.clone(),
                group_id: self.group_id.clone(),
                stage: STAGE_FAILED,
                message: message.to_string(),
                progress: None,
                error: Some(error),
                error_class: error_class.map(|code| {
                    serde_json::from_value(serde_json::json!(code))
                        .unwrap_or(dlssync_contracts::ApplyErrorClass::Other)
                }),
                attempt: None,
            },
        );
    }

    fn cancelled(&self) {
        let _ = self.handle.emit(
            EVENT_APPLY_PROGRESS,
            ApplyProgress {
                apply_id: self.apply_id.clone(),
                group_id: self.group_id.clone(),
                stage: STAGE_CANCELLED,
                message: "Cancelled".to_string(),
                progress: None,
                error: Some("cancelled".to_string()),
                error_class: Some(dlssync_contracts::ApplyErrorClass::Cancelled),
                attempt: None,
            },
        );
    }
}

pub(crate) fn emit_inflight(handle: &AppHandle, in_flight: usize) {
    let _ = handle.emit(EVENT_APPLY_INFLIGHT, InflightSnapshot { in_flight });
    crate::tray::update_inflight(handle, in_flight);
}

/// Authoritative crash-safety gate for the NVIDIA Streamline version-locked set.
/// Returns `Some(reason)` when an `sl.*` plugin must NOT be swapped: the user has
/// not opted in, or the target crosses the installed Streamline MAJOR (v1↔v2 is
/// the real cause of the launch crashes — older games ship SL 1.x and break when
/// newer 2.x plugins are swapped in). A same-major swap is allowed even when a
/// DLSS Enabler is present, because the Enabler requires Streamline >= 2.11 yet
/// does not update it, so the user must update the set themselves. NGX DLLs
/// (`nvngx_*.dll`) are never gated — the driver loads them independently and they
/// are safe to swap on their own.
pub(crate) fn streamline_guard(
    state: &StateHandles,
    dll_path: &std::path::Path,
    target_version: &str,
) -> Option<String> {
    let filename = dll_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if !dll_scanner::is_streamline_plugin(filename) {
        return None;
    }
    let allow_streamline = state.settings.read().update_prefs.update_streamline;
    let installed_major = pe_version::read_dll_version(dll_path)
        .ok()
        .and_then(|v| version_major(&v.file_version));
    let target_major = version_major(target_version);
    streamline_block_reason(filename, allow_streamline, installed_major, target_major)
}

const WINDOWS_EXE_EXTENSION: &str = "exe";

/// Best-effort pre-flight: returns the file name of a running executable whose
/// image path lives under `game_root`, signalling the game is open and its DLLs
/// are likely locked. Returns `None` when nothing matches OR when process
/// enumeration is unavailable — detection failure must never block an apply, so
/// the caller proceeds and falls back to the on-disk lock/replace error path.
pub(crate) fn detect_running_game(game_root: &std::path::Path) -> Option<String> {
    let root = normalize_for_match(game_root)?;
    let mut sys = sysinfo::System::new();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::nothing(),
    );
    for process in sys.processes().values() {
        let Some(exe) = process.exe() else { continue };
        if !is_executable_image(exe) {
            continue;
        }
        if exe_is_under_root(exe, &root) {
            return exe
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());
        }
    }
    None
}

/// True when `exe` is a Windows executable image. Guards against matching, e.g.,
/// a stray `.dll`-hosted process record and keeps the gate to actual game binaries.
fn is_executable_image(exe: &std::path::Path) -> bool {
    exe.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(WINDOWS_EXE_EXTENSION))
}

/// Lowercased, separator-normalized string form of a path for case-insensitive
/// prefix comparison on Windows. Returns `None` for an empty path so an absent
/// install dir never matches every running process.
fn normalize_for_match(path: &std::path::Path) -> Option<String> {
    let s = path.to_str()?;
    if s.is_empty() {
        return None;
    }
    Some(s.replace('\\', "/").trim_end_matches('/').to_lowercase())
}

/// True when `exe` resides inside the `root` directory subtree. Compares on
/// normalized lowercase strings and requires a path-boundary match (`root` itself
/// or `root/...`) so `C:/Games/Halo2` does not spuriously match `C:/Games/Halo`.
fn exe_is_under_root(exe: &std::path::Path, root: &str) -> bool {
    let Some(exe_norm) = normalize_for_match(exe) else {
        return false;
    };
    if exe_norm == root {
        return true;
    }
    exe_norm
        .strip_prefix(root)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn gpu_vendor_matches(vendor: crate::system_info::GpuVendor, requested: &str) -> bool {
    matches!(
        (vendor, requested.to_ascii_lowercase().as_str()),
        (crate::system_info::GpuVendor::Nvidia, "nvidia")
            | (crate::system_info::GpuVendor::Amd, "amd")
            | (crate::system_info::GpuVendor::Intel, "intel")
    )
}

fn version_numbers(raw: &str) -> Vec<u32> {
    raw.split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn driver_meets_minimum(vendor: &str, installed: &str, minimum: &str) -> bool {
    let mut installed = version_numbers(installed);
    if vendor.eq_ignore_ascii_case("nvidia")
        && installed.len() >= 4
        && installed[0] >= 30
        && installed[2] >= 10
    {
        installed = vec![
            (installed[2] - 10) * 100 + installed[3] / 100,
            installed[3] % 100,
        ];
    }
    installed.as_slice() >= version_numbers(minimum).as_slice()
}

fn ensure_writable(path: &std::path::Path) -> Result<(), String> {
    use std::fs::OpenOptions;
    match OpenOptions::new().read(true).write(true).open(path) {
        Ok(_) => Ok(()),
        Err(e) => {
            let code = e.raw_os_error().unwrap_or(0);
            if code == 32 || code == 33 {
                Err(format!(
                    "file is locked by another process ({})",
                    path.display()
                ))
            } else if code == 5 {
                Err(format!(
                    "access denied to {} (try running as administrator)",
                    path.display()
                ))
            } else {
                Err(format!("cannot open {} for writing: {}", path.display(), e))
            }
        }
    }
}

pub(crate) struct StateHandles {
    pub catalog: Arc<parking_lot::RwLock<Option<dll_catalog::Catalog>>>,
    pub backups: Arc<parking_lot::RwLock<Option<backup_store::BackupStore>>>,
    pub journal: Arc<parking_lot::RwLock<Option<operation_journal::JournalStore>>>,
    pub settings: Arc<parking_lot::RwLock<crate::commands::settings::AppSettings>>,
    pub http_downloads: Arc<parking_lot::RwLock<reqwest::Client>>,
    pub download_cache: Arc<dll_catalog::DownloadCache>,
    pub authoritative_state: Arc<dlssync_application::state::StateCoordinator>,
}

impl AppState {
    pub(crate) fn clone_handles(&self) -> StateHandles {
        StateHandles {
            catalog: self.catalog.clone(),
            backups: self.backups.clone(),
            journal: self.journal.clone(),
            settings: self.settings.clone(),
            http_downloads: self.http_downloads.clone(),
            download_cache: self.download_cache.clone(),
            authoritative_state: self.authoritative_state.clone(),
        }
    }
}

fn refresh_apply_persisted_state(state: &StateHandles) {
    let backups = state.backups.read();
    let history = state.journal.read();
    if let (Some(backups), Some(history)) = (backups.as_ref(), history.as_ref()) {
        if let Err(error) =
            dlssync_application::transaction::project_recovery_history(backups, history)
        {
            tracing::warn!(%error, "post-apply recovery History projection failed");
        }
    }
    match state
        .authoritative_state
        .refresh_persisted_views(backups.as_ref(), history.as_ref())
    {
        Ok(Some(receipt)) => {
            if let Some(error) = receipt.delivery_error {
                tracing::warn!(%error, "post-apply persisted-state event delivery failed");
            }
        }
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "post-apply persisted-state projection failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn batch_policy_rejects_automatically_expanded_fsr4() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = tempfile::tempdir()?;
        let catalog = dll_catalog::embedded_fallback_catalog()?;
        let mut json = serde_json::to_value(catalog)?;
        let releases = json["vendors"]["amd"]["fsr_upscaler"]["releases"]
            .as_array_mut()
            .ok_or("missing FSR releases")?;
        for release in releases {
            if release["filename"] == "amd_fidelityfx_upscaler_dx12.dll" {
                release["version"] = serde_json::json!("4.1.0");
            }
        }
        let catalog: dll_catalog::Catalog = serde_json::from_value(json)?;
        let mut components = Vec::new();
        for (family, filename) in [
            ("fsr_loader", "amd_fidelityfx_loader_dx12.dll"),
            ("fsr_upscaler", "amd_fidelityfx_upscaler_dx12.dll"),
        ] {
            let path = temp.path().join(filename);
            let mut bytes = vec![0; 128];
            bytes[..2].copy_from_slice(b"MZ");
            bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
            bytes[64..68].copy_from_slice(b"PE\0\0");
            bytes[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
            bytes[84..86].copy_from_slice(&2u16.to_le_bytes());
            bytes[86..88].copy_from_slice(&0x2000u16.to_le_bytes());
            bytes[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
            std::fs::write(&path, bytes)?;
            components.push(dlssync_contracts::ScannedComponent {
                family: family.into(),
                path: path.display().to_string(),
                current_version: None,
                sha256: Some(dll_catalog::hex_sha256_file(&path)?),
            });
        }
        let games = [dlssync_contracts::ScannedGame {
            id: "game".into(),
            name: "Game".into(),
            launcher: "manual".into(),
            install_dir: temp.path().display().to_string(),
            components,
        }];
        let items = dlssync_application::plan_items(&catalog, &games, temp.path(), None)
            .into_iter()
            .filter(|item| item.family == "fsr_loader")
            .collect();
        let plan =
            dlssync_application::build_verified_update_plan(&catalog, &games, items, temp.path())?;
        assert!(plan.changes.iter().any(|change| change.added_as_dependency));
        assert!(plan
            .items
            .iter()
            .any(|item| item.family == "fsr_upscaler" && item.target_version == "4.1.0"));
        assert!(
            validate_batch_policy(&plan, &dlssync_application::policy::ApplyPolicy::default())
                .is_err_and(|error| error.to_string().contains("RDNA4"))
        );
        Ok(())
    }

    #[test]
    fn group_failure_preserves_recovery_diagnostic_in_event_and_result(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let request: ApplyRequest = serde_json::from_value(serde_json::json!({
            "apply_id": "a", "game_id": "game", "dll_path": "a.dll",
            "vendor": "nvidia", "family": "dlss_sr", "target_version": "1"
        }))?;
        for reason in [
            "cancelled",
            "rolled_back: cancelled",
            "rollback_failed: cancelled",
        ] {
            let event = group_failure_progress(&request, reason);
            let outcome = failure_outcome(&request, "game", reason.into());
            assert_eq!(event.error.as_deref(), Some(reason));
            assert_eq!(outcome.error.as_deref(), Some(reason));
            assert_eq!(
                event.stage,
                if reason == "cancelled" {
                    STAGE_CANCELLED
                } else {
                    STAGE_FAILED
                }
            );
        }
        Ok(())
    }

    #[test]
    fn cancelling_another_game_does_not_cancel_this_group() {
        let request: ApplyRequest = serde_json::from_value(serde_json::json!({
            "apply_id": "a", "game_id": "game-a", "dll_path": "a.dll",
            "vendor": "nvidia", "family": "dlss_sr", "target_version": "1"
        }))
        .unwrap();
        let a = CancellationToken::new();
        let b = CancellationToken::new();
        let tokens = HashMap::from([("a".into(), a.clone()), ("b".into(), b.clone())]);
        let group = cancellation_tokens_for_group(&[request], &tokens);
        b.cancel();
        assert!(!group.values().any(CancellationToken::is_cancelled));
        a.cancel();
        assert!(group.values().any(CancellationToken::is_cancelled));
    }

    fn under(exe: &str, root: &str) -> bool {
        let root_norm = normalize_for_match(Path::new(root)).expect("root normalizes");
        exe_is_under_root(Path::new(exe), &root_norm)
    }

    #[test]
    fn exe_directly_in_root_matches() {
        assert!(under(r"C:\Games\Halo\halo.exe", r"C:\Games\Halo"));
    }

    #[test]
    fn exe_in_nested_subdir_matches() {
        assert!(under(r"C:\Games\Halo\bin\x64\halo.exe", r"C:\Games\Halo"));
    }

    #[test]
    fn root_path_itself_matches() {
        // A process whose image path equals the root (degenerate but possible).
        assert!(under(r"C:\Games\Halo", r"C:\Games\Halo"));
    }

    #[test]
    fn sibling_prefix_does_not_match_boundary() {
        // C:\Games\Halo must not match a sibling that merely shares the prefix.
        assert!(!under(r"C:\Games\Halo2\halo2.exe", r"C:\Games\Halo"));
    }

    #[test]
    fn comparison_is_case_insensitive() {
        assert!(under(r"c:\games\HALO\Halo.EXE", r"C:\Games\Halo"));
    }

    #[test]
    fn forward_and_back_slashes_are_equivalent() {
        assert!(under("C:/Games/Halo/bin/halo.exe", r"C:\Games\Halo"));
        assert!(under(r"C:\Games\Halo\bin\halo.exe", "C:/Games/Halo"));
    }

    #[test]
    fn trailing_separator_on_root_is_ignored() {
        assert!(under(r"C:\Games\Halo\halo.exe", r"C:\Games\Halo\"));
    }

    #[test]
    fn unrelated_path_does_not_match() {
        assert!(!under(r"C:\Windows\explorer.exe", r"C:\Games\Halo"));
    }

    #[test]
    fn empty_root_normalizes_to_none() {
        // An empty/missing install dir must never produce a matchable root,
        // otherwise every running process would falsely match.
        assert!(normalize_for_match(Path::new("")).is_none());
    }

    #[test]
    fn non_exe_image_is_rejected() {
        assert!(!is_executable_image(Path::new(
            r"C:\Games\Halo\sl.dlss.dll"
        )));
        assert!(is_executable_image(Path::new(r"C:\Games\Halo\halo.exe")));
        assert!(is_executable_image(Path::new(r"C:\Games\Halo\Halo.EXE")));
    }

    #[test]
    fn minimum_driver_comparison_normalizes_nvidia_wmi_versions() {
        assert!(driver_meets_minimum("nvidia", "32.0.15.9174", "535.98"));
        assert!(!driver_meets_minimum("nvidia", "32.0.15.2240", "535.98"));
        assert!(driver_meets_minimum("amd", "32.0.21040.7000", "22.7.1"));
        assert!(!driver_meets_minimum(
            "intel",
            "31.0.101.2115",
            "31.0.101.4255"
        ));
    }
}
