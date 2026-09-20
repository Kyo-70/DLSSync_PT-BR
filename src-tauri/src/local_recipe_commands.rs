//! Local recipe adapter. The frontend receives opaque, expiring, one-use preview IDs.
//! Targets and rollback bytes always come from the durable installation owner.
use crate::{
    error::{AppError, AppResult},
    state::AppState,
};
use dlssync_application::recipes::{self as domain, RecipeStore, Sha256};
use dlssync_contracts::*;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};
use tauri::State;

struct Preview {
    game_id: String,
    root: PathBuf,
    data_root: PathBuf,
    expires: chrono::DateTime<chrono::Utc>,
    intent: RecipePreviewIntent,
    recipe: domain::RecipeV1,
    prepared: Vec<domain::PreparedRecipeMutation>,
    _staging: tempfile::TempDir,
}
static PREVIEWS: Lazy<Mutex<HashMap<String, Preview>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn err(error: impl std::fmt::Display) -> AppError {
    AppError::Other(error.to_string())
}
fn issue(message: impl std::fmt::Display) -> RecipeIssue {
    RecipeIssue {
        code: "local_recipe_rejected".into(),
        message: message.to_string(),
        context: BTreeMap::new(),
    }
}
fn context(
    state: &AppState,
    game_id: &str,
) -> AppResult<(
    GameSnapshot,
    PathBuf,
    RecipeStore,
    operation_journal::JournalStore,
)> {
    let game = state
        .authoritative_state
        .snapshot()
        .games
        .into_iter()
        .find(|game| game.id == game_id)
        .ok_or_else(|| err("Game is not in the current library. Scan the library first."))?;
    let data_root = state
        .paths
        .read()
        .as_ref()
        .ok_or_else(|| err("Application paths are unavailable"))?
        .root
        .join("Recipes");
    let store = RecipeStore::open_for_install(&data_root, &game.install_dir).map_err(err)?;
    let journal = state
        .journal
        .read()
        .clone()
        .ok_or_else(|| err("Operation journal is unavailable"))?;
    Ok((game, data_root, store, journal))
}
fn safety(game: &GameSnapshot, store: &RecipeStore) -> AppResult<()> {
    if !game.observation_complete {
        return Err(err(
            "Game observation is incomplete. Scan again before changing mods.",
        ));
    }
    if let Some(name) = crate::commands::apply::detect_running_game(store.installation_root()) {
        return Err(err(format!("Close {name} before changing mods.")));
    }
    let protections = anticheat_detect::detect_protections(
        store.installation_root(),
        None,
        anticheat_detect::DEFAULT_SCAN_DEPTH,
        &[],
    );
    if !protections.is_empty() {
        return Err(err(
            "Protected game files were detected. Local mod changes are not available.",
        ));
    }
    Ok(())
}
fn bounded_read(path: &Path, limit: u64) -> AppResult<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(err)?;
    if file.metadata().map_err(err)?.len() > limit {
        return Err(err("Selected file exceeds the size limit"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(err)?;
    if bytes.len() as u64 > limit {
        return Err(err("Selected file changed while reading"));
    }
    Ok(bytes)
}
fn validate_target(
    game: &GameSnapshot,
    store: &RecipeStore,
    recipe: &domain::RecipeV1,
) -> AppResult<()> {
    safety(game, store)?;
    if !recipe.target.game_ids.contains(&game.id) {
        return Err(err("This mod file does not target the selected game."));
    }
    let executable = store
        .target(&recipe.target.executable_relative_path)
        .map_err(err)?;
    if Sha256::digest(&bounded_read(&executable, 512 * 1024 * 1024)?)
        != recipe.target.executable_sha256
    {
        return Err(err(
            "The game executable differs from the version required by this mod.",
        ));
    }
    if !recipe.dependencies.is_empty()
        || !recipe.target.required_capabilities.is_empty()
        || !recipe.conflicts.is_empty()
    {
        return Err(err(
            "This local recipe requires checks that this adapter does not support.",
        ));
    }
    for path in recipe
        .installed_files
        .iter()
        .map(|file| &file.destination)
        .chain(recipe.config_keys.iter().map(|edit| &edit.path))
    {
        if dll_scanner::known_dll_family(path.as_str().rsplit('/').next().unwrap_or_default())
            .is_some()
        {
            return Err(err(format!(
                "Mod target conflicts with a catalog-owned DLL: {path}"
            )));
        }
    }
    let mut claims = Vec::new();
    for receipt in store.active_receipts().map_err(err)? {
        if receipt.recipe_id == recipe.id {
            continue;
        }
        for member in receipt.members {
            let relative = member
                .target
                .strip_prefix(store.installation_root())
                .map_err(err)?;
            claims.push(domain::RecipeFileClaim::new(
                receipt.recipe_id.clone(),
                domain::RelativePath::parse(relative.to_string_lossy().as_ref()).map_err(err)?,
                if matches!(
                    relative.file_name().and_then(|name| name.to_str()),
                    Some("dxgi.dll" | "dinput8.dll" | "version.dll")
                ) {
                    domain::FileRole::Proxy
                } else {
                    domain::FileRole::Data
                },
            ));
        }
    }
    claims.extend(recipe.installed_files.iter().map(|file| {
        domain::RecipeFileClaim::new(recipe.id.clone(), file.destination.clone(), file.role)
    }));
    let official = game
        .components
        .iter()
        .map(|component| {
            domain::RelativePath::parse(&component.identity.relative_path).map(|path| {
                domain::OfficialFileClaim::new(component.identity.filename.clone(), path)
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    domain::validate_file_claims(&claims, &official).map_err(err)?;
    for config in &recipe.config_keys {
        let target = store.target(&config.path).map_err(err)?;
        let config_key = config.path.comparison_key();
        if official
            .iter()
            .any(|file| file.path.comparison_key() == config_key)
            || claims
                .iter()
                .any(|file| file.path.comparison_key() == config_key)
        {
            return Err(err(format!(
                "Configuration conflicts with an owned component: {}",
                target.display()
            )));
        }
    }
    Ok(())
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn list_owned_recipes(
    state: State<'_, AppState>,
    request: OwnedRecipeListRequest,
) -> AppResult<OwnedRecipeListResult> {
    let (game, _, store, journal) = context(&state, &request.game_id)?;
    safety(&game, &store)?;
    domain::recover_recipe_transactions(&store, &journal).map_err(err)?;
    let mut recipes = Vec::new();
    for receipt in store.active_receipts().map_err(err)? {
        let mut modified = Vec::new();
        let mut missing = Vec::new();
        for member in &receipt.members {
            match std::fs::read(&member.target) {
                Ok(bytes) if Sha256::digest(&bytes) == member.owned_sha256 => {}
                Ok(_) => modified.push(member.target.to_string_lossy().into_owned()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    missing.push(member.target.to_string_lossy().into_owned())
                }
                Err(error) => return Err(err(error)),
            }
        }
        let ownership = if !modified.is_empty() {
            RecipeOwnershipState::OwnedModified { paths: modified }
        } else if !missing.is_empty() {
            RecipeOwnershipState::OwnedMissing { paths: missing }
        } else {
            RecipeOwnershipState::InstalledByDlssync {
                receipt_id: receipt.receipt_id.clone(),
            }
        };
        recipes.push(OwnedRecipeReceipt {
            installation_id: receipt.installation_id.clone(),
            created_at: receipt.created_at.clone(),
            members: receipt
                .members
                .iter()
                .map(|member| OwnedRecipeMember {
                    relative_path: member
                        .target
                        .strip_prefix(store.installation_root())
                        .unwrap_or(&member.target)
                        .to_string_lossy()
                        .into_owned(),
                    action: match member.kind {
                        domain::RecipeMutationKind::Create => RecipePreviewAction::Create,
                        domain::RecipeMutationKind::Configure => RecipePreviewAction::Configure,
                        _ => RecipePreviewAction::Replace,
                    },
                    owned_sha256: member.owned_sha256.to_string(),
                    configuration_key: member
                        .config
                        .as_ref()
                        .map(|config| config.selector.key.clone()),
                })
                .collect(),
            recipe_id: receipt.recipe_id.to_string(),
            receipt_id: receipt.receipt_id,
            upstream_version: receipt.upstream_version,
            revision: receipt.revision,
            state: RecipeState {
                ownership,
                observation: RecipeObservationState::NotObserved,
                compatibility: RecipeCompatibilityState::Unknown,
                support: RecipeSupportState::Unknown,
            },
        });
    }
    Ok(OwnedRecipeListResult {
        game_id: request.game_id,
        recipes,
    })
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn preview_local_recipe(
    state: State<'_, AppState>,
    request: RecipeLocalPreviewRequest,
) -> AppResult<RecipeLocalPreviewResult> {
    preview_local_recipe_impl(&state, request)
}

fn preview_local_recipe_impl(
    state: &AppState,
    request: RecipeLocalPreviewRequest,
) -> AppResult<RecipeLocalPreviewResult> {
    let expires = chrono::Utc::now() + chrono::Duration::minutes(5);
    let mut result = RecipeLocalPreviewResult {
        preview_id: String::new(),
        expires_at: expires.to_rfc3339(),
        allowed: false,
        recipe: None,
        files: Vec::new(),
        config_edits: Vec::new(),
        issues: Vec::new(),
    };
    let prepared = (|| -> AppResult<Preview> {
        let (game, data_root, store, journal) = context(state, &request.game_id)?;
        safety(&game, &store)?;
        domain::recover_recipe_transactions(&store, &journal).map_err(err)?;
        let bytes = bounded_read(Path::new(&request.recipe_path), 2 * 1024 * 1024)?;
        let recipe =
            domain::parse_recipe_json(std::str::from_utf8(&bytes).map_err(err)?).map_err(err)?;
        validate_target(&game, &store, &recipe)?;
        let mut descriptor =
            crate::recipe_commands::descriptor(&recipe, Sha256::digest(&bytes).to_string());
        // A local manifest's self-declared game test is not verified compatibility evidence.
        descriptor.state.compatibility = RecipeCompatibilityState::Unknown;
        descriptor.state.support = RecipeSupportState::Unknown;
        result.recipe = Some(descriptor);
        let staging = tempfile::Builder::new()
            .prefix("preview-")
            .tempdir_in(&data_root)
            .map_err(err)?;
        let mut prepared = Vec::new();
        if request.intent == RecipePreviewIntent::Apply {
            let source = PathBuf::from(
                request
                    .source_directory
                    .as_ref()
                    .ok_or_else(|| err("Select the folder containing the extracted mod files."))?,
            )
            .canonicalize()
            .map_err(err)?;
            let previous = store.active_receipt(&recipe.id).ok();
            for (index, file) in recipe.installed_files.iter().enumerate() {
                let target = store.target(&file.destination).map_err(err)?;
                let relative = file.archive_entry.as_ref().unwrap_or(&file.destination);
                let source_file = source.join(relative.as_str()).canonicalize().map_err(err)?;
                if !source_file.starts_with(&source) {
                    return Err(err("Source file escapes the selected folder"));
                }
                let bytes = bounded_read(&source_file, 512 * 1024 * 1024)?;
                if bytes.len() as u64 != file.size_bytes || Sha256::digest(&bytes) != file.sha256 {
                    return Err(err(format!(
                        "File does not match the mod's size and SHA-256: {relative}"
                    )));
                }
                let before = match std::fs::read(&target) {
                    Ok(bytes) => domain::RecipeFilePresence::Present {
                        sha256: Sha256::digest(&bytes),
                        undo_backup: None,
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        domain::RecipeFilePresence::Absent
                    }
                    Err(error) => return Err(err(error)),
                };
                let kind = match (&before, file.install_rule) {
                    (domain::RecipeFilePresence::Absent, domain::InstallRule::CreateOnly) => {
                        domain::RecipeMutationKind::Create
                    }
                    (
                        domain::RecipeFilePresence::Present { sha256, .. },
                        domain::InstallRule::ReplaceOwned,
                    ) if previous.as_ref().is_some_and(|receipt| {
                        receipt
                            .members
                            .iter()
                            .any(|member| member.target == target && &member.owned_sha256 == sha256)
                    }) =>
                    {
                        domain::RecipeMutationKind::Replace
                    }
                    _ => {
                        return Err(err(format!(
                            "Target is not eligible for this mod action: {}",
                            file.destination
                        )))
                    }
                };
                let staged = staging.path().join(format!("{index}.bin"));
                std::fs::write(&staged, bytes).map_err(err)?;
                result.files.push(RecipePreviewFile {
                    relative_path: file.destination.to_string(),
                    sha256: file.sha256.to_string(),
                    size_bytes: file.size_bytes,
                    role: serde_json::from_value(serde_json::to_value(file.role).map_err(err)?)
                        .map_err(err)?,
                    action: if kind == domain::RecipeMutationKind::Create {
                        RecipePreviewAction::Create
                    } else {
                        RecipePreviewAction::Replace
                    },
                });
                prepared.push(domain::PreparedRecipeMutation {
                    relative_path: file.destination.clone(),
                    artifact_id: Some(file.artifact_id.clone()),
                    archive_entry: file.archive_entry.clone(),
                    config: None,
                    target,
                    staged,
                    undo_backup: if kind == domain::RecipeMutationKind::Replace {
                        Some(
                            store
                                .root()
                                .join("undo")
                                .join(uuid::Uuid::new_v4().to_string()),
                        )
                    } else {
                        None
                    },
                    expected_before: before,
                    expected_after: file.sha256.clone(),
                    kind,
                });
            }
        }
        result.config_edits = recipe
            .config_keys
            .iter()
            .map(|edit| RecipePreviewConfigEdit {
                relative_path: edit.path.to_string(),
                section: edit.selector.section.clone(),
                key: edit.selector.key.clone(),
                desired: edit.desired.clone(),
            })
            .collect();
        Ok(Preview {
            game_id: request.game_id,
            root: store.installation_root().to_path_buf(),
            data_root,
            expires,
            intent: request.intent,
            recipe,
            prepared,
            _staging: staging,
        })
    })();
    match prepared {
        Ok(preview) => {
            let id = uuid::Uuid::new_v4().to_string();
            let mut previews = PREVIEWS.lock();
            previews.retain(|_, value| value.expires > chrono::Utc::now());
            if previews.len() >= 16 {
                return Err(err(
                    "Too many pending mod previews. Wait for an earlier preview to expire.",
                ));
            }
            previews.insert(id.clone(), preview);
            result.preview_id = id;
            result.allowed = true;
        }
        Err(error) => result.issues.push(issue(error)),
    }
    Ok(result)
}

fn write_preview(
    state: &AppState,
    request: RecipeApplyRequest,
    intent: RecipePreviewIntent,
) -> AppResult<RecipeOperationResult> {
    let preview = PREVIEWS
        .lock()
        .remove(&request.preview_id)
        .ok_or_else(|| err("Mod check expired or was already used. Check again."))?;
    if preview.expires <= chrono::Utc::now() || preview.intent != intent {
        return Err(err("Mod check expired or belongs to a different action."));
    }
    let (game, data_root, store, journal) = context(state, &preview.game_id)?;
    if preview.root != store.installation_root() || preview.data_root != data_root {
        return Err(err("The game installation changed. Check again."));
    }
    validate_target(&game, &store, &preview.recipe)?;
    let operation_id = uuid::Uuid::new_v4().to_string();
    let receipt = if intent == RecipePreviewIntent::Apply {
        domain::apply_recipe_durable(
            &store,
            &journal,
            &operation_id,
            &preview.recipe,
            &preview.prepared,
        )
    } else {
        domain::configure_recipe_durable(&store, &journal, &operation_id, &preview.recipe)
    }
    .map_err(err)?;
    let result = RecipeOperationResult {
        operation_id,
        game_id: preview.game_id,
        recipe_id: preview.recipe.id.to_string(),
        receipt_id: Some(receipt.receipt_id),
        status: RecipeOperationStatus::Completed,
        changed_paths: receipt
            .members
            .iter()
            .map(|member| member.target.to_string_lossy().into_owned())
            .collect(),
        retained_paths: Vec::new(),
        issues: Vec::new(),
    };
    record_result(state, &journal, &result, OperationKind::RecipeApply)?;
    Ok(result)
}
#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn apply_local_recipe(
    state: State<'_, AppState>,
    request: RecipeApplyRequest,
) -> AppResult<RecipeOperationResult> {
    write_preview(&state, request, RecipePreviewIntent::Apply)
}
#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn configure_local_recipe(
    state: State<'_, AppState>,
    request: RecipeConfigureRequest,
) -> AppResult<RecipeOperationResult> {
    write_preview(
        &state,
        RecipeApplyRequest {
            preview_id: request.preview_id,
        },
        RecipePreviewIntent::Configure,
    )
}
#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn remove_owned_recipe(
    state: State<'_, AppState>,
    request: RecipeDurableRemovalRequest,
) -> AppResult<RecipeOperationResult> {
    remove_owned_recipe_impl(&state, request)
}

fn remove_owned_recipe_impl(
    state: &AppState,
    request: RecipeDurableRemovalRequest,
) -> AppResult<RecipeOperationResult> {
    let (game, _, store, journal) = context(state, &request.game_id)?;
    safety(&game, &store)?;
    domain::recover_recipe_transactions(&store, &journal).map_err(err)?;
    let id = domain::RecipeId::parse(&request.recipe_id).map_err(err)?;
    let receipt = store.active_receipt(&id).map_err(err)?;
    let operation_id = uuid::Uuid::new_v4().to_string();
    let outcome =
        domain::remove_recipe_durable(&store, &journal, &operation_id, &id).map_err(err)?;
    let completed = matches!(outcome, domain::RemovalOutcome::Removed { .. });
    let paths: Vec<_> = receipt
        .members
        .iter()
        .map(|member| member.target.to_string_lossy().into_owned())
        .collect();
    let result = RecipeOperationResult {
        operation_id,
        game_id: request.game_id,
        recipe_id: request.recipe_id,
        receipt_id: Some(receipt.receipt_id),
        status: if completed {
            RecipeOperationStatus::Completed
        } else {
            RecipeOperationStatus::Failed
        },
        changed_paths: if completed { paths.clone() } else { Vec::new() },
        retained_paths: if completed { Vec::new() } else { paths },
        issues: if completed {
            Vec::new()
        } else {
            vec![issue(format!("{outcome:?}"))]
        },
    };
    record_result(state, &journal, &result, OperationKind::RecipeRemoval)?;
    Ok(result)
}

fn record_result(
    state: &AppState,
    journal: &operation_journal::JournalStore,
    result: &RecipeOperationResult,
    kind: OperationKind,
) -> AppResult<()> {
    journal
        .append(&OperationRecord {
            id: result.operation_id.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
            actor: OperationActor::Gui,
            kind,
            status: if result.status == RecipeOperationStatus::Completed {
                OperationStatus::Succeeded
            } else {
                OperationStatus::Failed
            },
            target: Some(result.recipe_id.clone()),
            summary: "Local mod action".into(),
            details: BTreeMap::from([
                ("game_id".into(), result.game_id.clone()),
                (
                    "changed_files".into(),
                    result.changed_paths.len().to_string(),
                ),
                (
                    "retained_files".into(),
                    result.retained_paths.len().to_string(),
                ),
            ]),
            duration_ms: None,
            backup_id: result.receipt_id.clone(),
            error: result.issues.first().map(|issue| issue.message.clone()),
        })
        .map_err(err)?;
    crate::commands::runtime::refresh_persisted_state(state).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, AppState, RecipeLocalPreviewRequest) {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        let source = root.path().join("source");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(game.join("game.exe"), b"fixture executable").unwrap();
        std::fs::write(source.join("addon.dll"), b"local mod").unwrap();
        let mut recipe: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/recipe-v1.json")).unwrap();
        recipe["target"]["executable_sha256"] =
            Sha256::digest(b"fixture executable").to_string().into();
        recipe["installed_files"][0]["destination"] = "addon.dll".into();
        recipe["installed_files"][0]["sha256"] = Sha256::digest(b"local mod").to_string().into();
        recipe["installed_files"][0]["size_bytes"] = 9.into();
        let path = root.path().join("recipe.json");
        std::fs::write(&path, recipe.to_string()).unwrap();
        let state = AppState::new();
        *state.paths.write() = Some(crate::paths::AppPaths::from_root(
            root.path().join("profile"),
        ));
        *state.journal.write() = Some(
            operation_journal::JournalStore::open(root.path().join("operations.sqlite3")).unwrap(),
        );
        state
            .authoritative_state
            .commit(dlssync_application::state::StateCommit {
                delta: StateDelta {
                    games: vec![GameSnapshot {
                        id: "example-game".into(),
                        name: "Recipe fixture".into(),
                        install_dir: game.to_string_lossy().into_owned(),
                        observation_complete: true,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                ..Default::default()
            })
            .unwrap();
        let request = RecipeLocalPreviewRequest {
            game_id: "example-game".into(),
            recipe_path: path.to_string_lossy().into_owned(),
            source_directory: Some(source.to_string_lossy().into_owned()),
            intent: RecipePreviewIntent::Apply,
        };
        (root, state, request)
    }
    #[test]
    fn local_preview_applies_once_and_removal_preserves_external_bytes() {
        let (root, state, request) = fixture();
        let preview = preview_local_recipe_impl(&state, request).unwrap();
        assert!(preview.allowed, "{:?}", preview.issues);
        assert_eq!(
            preview.recipe.unwrap().state.compatibility,
            RecipeCompatibilityState::Unknown
        );
        let write = RecipeApplyRequest {
            preview_id: preview.preview_id,
        };
        let result = write_preview(&state, write.clone(), RecipePreviewIntent::Apply).unwrap();
        assert_eq!(result.status, RecipeOperationStatus::Completed);
        assert_eq!(
            std::fs::read(root.path().join("game/addon.dll")).unwrap(),
            b"local mod"
        );
        assert!(write_preview(&state, write, RecipePreviewIntent::Apply).is_err());
        std::fs::write(root.path().join("game/addon.dll"), b"external").unwrap();
        let result = remove_owned_recipe_impl(
            &state,
            RecipeDurableRemovalRequest {
                game_id: "example-game".into(),
                recipe_id: "renodx/example-game".into(),
            },
        )
        .unwrap();
        assert_eq!(result.status, RecipeOperationStatus::Failed);
        assert_eq!(
            std::fs::read(root.path().join("game/addon.dll")).unwrap(),
            b"external"
        );
        assert!(!state.authoritative_state.snapshot().history.is_empty());
    }
    #[test]
    fn local_preview_rejects_wrong_bytes_and_rechecks_game_at_write() {
        let (root, state, request) = fixture();
        std::fs::write(root.path().join("source/addon.dll"), b"wrong").unwrap();
        assert!(
            !preview_local_recipe_impl(&state, request.clone())
                .unwrap()
                .allowed
        );
        std::fs::write(root.path().join("source/addon.dll"), b"local mod").unwrap();
        let preview = preview_local_recipe_impl(&state, request).unwrap();
        assert!(preview.allowed);
        std::fs::write(root.path().join("game/game.exe"), b"new game build").unwrap();
        assert!(write_preview(
            &state,
            RecipeApplyRequest {
                preview_id: preview.preview_id
            },
            RecipePreviewIntent::Apply
        )
        .is_err());
        assert!(!root.path().join("game/addon.dll").exists());
    }
    #[test]
    fn local_preview_expiry_and_wrong_intent_never_write() {
        let (root, state, request) = fixture();
        let preview = preview_local_recipe_impl(&state, request.clone()).unwrap();
        assert!(preview.allowed);
        PREVIEWS
            .lock()
            .get_mut(&preview.preview_id)
            .unwrap()
            .expires = chrono::Utc::now() - chrono::Duration::seconds(1);
        assert!(write_preview(
            &state,
            RecipeApplyRequest {
                preview_id: preview.preview_id
            },
            RecipePreviewIntent::Apply
        )
        .is_err());
        let preview = preview_local_recipe_impl(&state, request).unwrap();
        assert!(write_preview(
            &state,
            RecipeApplyRequest {
                preview_id: preview.preview_id
            },
            RecipePreviewIntent::Configure
        )
        .is_err());
        assert!(!root.path().join("game/addon.dll").exists());
    }
    #[test]
    fn local_preview_cannot_claim_catalog_owned_filenames() {
        let (_root, state, request) = fixture();
        let mut recipe: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&request.recipe_path).unwrap()).unwrap();
        recipe["installed_files"][0]["destination"] = "NVNGX_DLSS.DLL".into();
        std::fs::write(&request.recipe_path, recipe.to_string()).unwrap();
        let result = preview_local_recipe_impl(&state, request).unwrap();
        assert!(!result.allowed);
        assert!(result.issues[0].message.contains("catalog-owned"));
    }
}
