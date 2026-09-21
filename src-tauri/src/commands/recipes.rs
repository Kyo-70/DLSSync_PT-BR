use crate::error::{AppError, AppResult};
use crate::state::AppState;
use dlssync_application::recipes::{
    finalize_removal, parse_recipe_json, validate_file_claims, verify_restoration, EvidenceKind,
    EvidenceResult, FileRole, OfficialFileClaim, RecipeConflictError, RecipeFileClaim, RecipeId,
    RecipeValidationError, RelativePath, RemovalFailure, RemovalOutcome, Sha256,
};
use dlssync_contracts::{
    ExistingRecipeFileClaim, OfficialComponentClaim, OperationActor, OperationKind,
    OperationRecord, OperationStatus, RecipeCatalog, RecipeCompatibilityState, RecipeConflict,
    RecipeConflictKind, RecipeConflictPreviewRequest, RecipeConflictPreviewResult,
    RecipeDescriptor, RecipeFileRole, RecipeIssue, RecipeObservationState, RecipeOwnershipState,
    RecipeRemovalFailure, RecipeRemovalRequest, RecipeRemovalResult, RecipeRemovalStatus,
    RecipeState, RecipeSupportState, RecipeValidationRequest, RecipeValidationResult,
    RecipeVerifiedRestoration,
};
use operation_journal::JournalStore;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;
use tauri::State;

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub fn list_known_recipes() -> RecipeCatalog {
    RecipeCatalog {
        schema_version: 1,
        acquisition_requires_user_action: true,
        recipes: Vec::new(),
    }
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub fn validate_recipe(request: RecipeValidationRequest) -> RecipeValidationResult {
    validate_recipe_contract(request)
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub fn preview_recipe_conflicts(
    request: RecipeConflictPreviewRequest,
) -> RecipeConflictPreviewResult {
    preview_recipe_conflicts_contract(request)
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub fn remove_recipe(
    state: State<'_, AppState>,
    request: RecipeRemovalRequest,
) -> AppResult<RecipeRemovalResult> {
    let started = Instant::now();
    let result = remove_recipe_verified_contract(request.clone());
    let journal = state
        .journal
        .read()
        .clone()
        .ok_or_else(|| AppError::Other("operation journal is unavailable".into()))?;
    append_removal_record(&journal, &request, &result, started.elapsed())?;
    Ok(result)
}

pub fn validate_recipe_contract(request: RecipeValidationRequest) -> RecipeValidationResult {
    match validated_recipe(&request.recipe_json, &request.expected_sha256) {
        Ok((recipe, digest)) => RecipeValidationResult {
            valid: true,
            recipe: Some(descriptor(&recipe, digest)),
            issue: None,
        },
        Err(issue) => RecipeValidationResult {
            valid: false,
            recipe: None,
            issue: Some(issue),
        },
    }
}

pub fn preview_recipe_conflicts_contract(
    request: RecipeConflictPreviewRequest,
) -> RecipeConflictPreviewResult {
    let (recipe, _) = match validated_recipe(&request.recipe_json, &request.expected_sha256) {
        Ok(validated) => validated,
        Err(issue) => {
            return RecipeConflictPreviewResult {
                allowed: false,
                conflicts: Vec::new(),
                issue: Some(issue),
            };
        }
    };

    let mut claims = match request
        .existing_recipe_claims
        .iter()
        .map(contract_recipe_claim)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(claims) => claims,
        Err(issue) => {
            return RecipeConflictPreviewResult {
                allowed: false,
                conflicts: Vec::new(),
                issue: Some(issue),
            };
        }
    };
    claims.extend(
        recipe.installed_files.iter().map(|file| {
            RecipeFileClaim::new(recipe.id.clone(), file.destination.clone(), file.role)
        }),
    );
    let official = match request
        .official_components
        .iter()
        .map(contract_official_claim)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(claims) => claims,
        Err(issue) => {
            return RecipeConflictPreviewResult {
                allowed: false,
                conflicts: Vec::new(),
                issue: Some(issue),
            };
        }
    };

    match validate_file_claims(&claims, &official) {
        Ok(()) => RecipeConflictPreviewResult {
            allowed: true,
            conflicts: Vec::new(),
            issue: None,
        },
        Err(error) => RecipeConflictPreviewResult {
            allowed: false,
            conflicts: vec![conflict_contract(error)],
            issue: None,
        },
    }
}

pub fn remove_recipe_verified_contract(request: RecipeRemovalRequest) -> RecipeRemovalResult {
    let mut verifications = Vec::new();
    let mut contract_failures = Vec::new();
    for restoration in &request.restorations {
        match Sha256::parse(&restoration.expected_sha256) {
            Ok(expected) => {
                verifications.push(verify_restoration(Path::new(&restoration.path), &expected));
            }
            Err(error) => contract_failures.push(RecipeRemovalFailure {
                code: "invalid_expected_sha256".into(),
                message: error.to_string(),
                path: Some(restoration.path.clone()),
                expected_sha256: Some(restoration.expected_sha256.clone()),
                observed_sha256: None,
            }),
        }
    }

    let (mut status, verified, failures) = match finalize_removal(verifications) {
        RemovalOutcome::Removed {
            verified_restorations,
        } => (
            RecipeRemovalStatus::Removed,
            verified_restorations,
            Vec::new(),
        ),
        RemovalOutcome::Failed {
            verified_restorations,
            failures,
        } => (RecipeRemovalStatus::Failed, verified_restorations, failures),
    };
    if !contract_failures.is_empty() {
        status = RecipeRemovalStatus::Failed;
    }
    let mut failures: Vec<_> = failures.into_iter().map(removal_failure_contract).collect();
    failures.extend(contract_failures);
    RecipeRemovalResult {
        operation_id: request.operation_id,
        recipe_id: request.recipe_id,
        receipt_id: request.receipt_id,
        status,
        verified_restorations: verified
            .into_iter()
            .map(|entry| RecipeVerifiedRestoration {
                path: entry.path.to_string_lossy().into_owned(),
                observed_sha256: entry.observed_sha256.to_string(),
            })
            .collect(),
        failures,
    }
}

fn validated_recipe(
    recipe_json: &str,
    expected_sha256: &str,
) -> Result<(dlssync_application::recipes::RecipeV1, String), RecipeIssue> {
    if expected_sha256.trim().is_empty() {
        return Err(issue(
            "missing_expected_integrity",
            "An expected recipe SHA-256 is required.",
        ));
    }
    let expected = Sha256::parse(expected_sha256)
        .map_err(|error| issue("invalid_expected_sha256", error.to_string()))?;
    let observed = Sha256::digest(recipe_json.as_bytes());
    if observed != expected {
        return Err(RecipeIssue {
            code: "integrity_mismatch".into(),
            message: "Recipe bytes do not match the expected SHA-256.".into(),
            context: BTreeMap::from([
                ("expected_sha256".into(), expected.to_string()),
                ("observed_sha256".into(), observed.to_string()),
            ]),
        });
    }
    parse_recipe_json(recipe_json)
        .map(|recipe| (recipe, observed.to_string()))
        .map_err(validation_issue)
}

pub(crate) fn descriptor(
    recipe: &dlssync_application::recipes::RecipeV1,
    manifest_sha256: String,
) -> RecipeDescriptor {
    let compatibility = recipe
        .compatibility_evidence
        .iter()
        .find_map(|wanted| {
            recipe
                .evidence
                .iter()
                .find(|evidence| evidence.id == *wanted)
        })
        .map_or(RecipeCompatibilityState::Unknown, |evidence| {
            match (evidence.kind, evidence.result) {
                (EvidenceKind::GameTest, Some(EvidenceResult::Pass)) => {
                    RecipeCompatibilityState::TestedPass {
                        evidence_id: evidence.id.as_str().into(),
                    }
                }
                (EvidenceKind::GameTest, Some(EvidenceResult::Fail)) => {
                    RecipeCompatibilityState::TestedFail {
                        evidence_id: evidence.id.as_str().into(),
                    }
                }
                _ => RecipeCompatibilityState::DocumentedOnly {
                    evidence_ids: recipe
                        .compatibility_evidence
                        .iter()
                        .map(|id| id.as_str().to_string())
                        .collect(),
                },
            }
        });
    RecipeDescriptor {
        schema_version: recipe.schema_version,
        id: recipe.id.to_string(),
        revision: recipe.revision,
        upstream_version: recipe.upstream_version.clone(),
        experimental: recipe.experimental,
        manifest_sha256,
        state: RecipeState {
            ownership: RecipeOwnershipState::None,
            observation: RecipeObservationState::NotObserved,
            compatibility,
            support: if recipe.experimental {
                RecipeSupportState::Experimental
            } else {
                RecipeSupportState::Unknown
            },
        },
    }
}

fn validation_issue(error: RecipeValidationError) -> RecipeIssue {
    let mut context = BTreeMap::new();
    if let Some(version) = error.schema_version() {
        context.insert("observed_schema_version".into(), version.to_string());
    }
    RecipeIssue {
        code: error.code().into(),
        message: error.to_string(),
        context,
    }
}

fn contract_recipe_claim(claim: &ExistingRecipeFileClaim) -> Result<RecipeFileClaim, RecipeIssue> {
    let recipe_id = RecipeId::parse(&claim.recipe_id)
        .map_err(|error| issue("invalid_recipe_id", error.to_string()))?;
    let path = RelativePath::parse(&claim.path)
        .map_err(|error| issue("invalid_relative_path", error.to_string()))?;
    Ok(RecipeFileClaim::new(
        recipe_id,
        path,
        match claim.role {
            RecipeFileRole::Proxy => FileRole::Proxy,
            RecipeFileRole::Addon => FileRole::Addon,
            RecipeFileRole::Shader => FileRole::Shader,
            RecipeFileRole::Data => FileRole::Data,
            RecipeFileRole::Notice => FileRole::Notice,
        },
    ))
}

fn contract_official_claim(
    claim: &OfficialComponentClaim,
) -> Result<OfficialFileClaim, RecipeIssue> {
    RelativePath::parse(&claim.path)
        .map(|path| OfficialFileClaim::new(claim.component_id.clone(), path))
        .map_err(|error| issue("invalid_relative_path", error.to_string()))
}

fn conflict_contract(error: RecipeConflictError) -> RecipeConflict {
    let message = error.to_string();
    match error {
        RecipeConflictError::RecipePathConflict {
            first_recipe,
            second_recipe,
            path,
        } => RecipeConflict {
            kind: RecipeConflictKind::RecipePath,
            recipe_id: second_recipe.to_string(),
            other_party: first_recipe.to_string(),
            path: path.to_string(),
            message,
        },
        RecipeConflictError::OfficialComponentPathConflict {
            recipe_id,
            component_id,
            path,
        } => RecipeConflict {
            kind: RecipeConflictKind::OfficialComponentPath,
            recipe_id: recipe_id.to_string(),
            other_party: component_id,
            path: path.to_string(),
            message,
        },
        RecipeConflictError::UnsupportedProxyFilename { recipe_id, path } => RecipeConflict {
            kind: RecipeConflictKind::UnsupportedProxyFilename,
            recipe_id: recipe_id.to_string(),
            other_party: "proxy_policy".into(),
            path: path.to_string(),
            message,
        },
        RecipeConflictError::UnsupportedProxyChain {
            first_recipe,
            first_slot,
            second_recipe,
            second_slot,
            directory,
        } => RecipeConflict {
            kind: RecipeConflictKind::UnsupportedProxyChain,
            recipe_id: second_recipe.to_string(),
            other_party: first_recipe.to_string(),
            path: format!("{directory}/{first_slot} + {directory}/{second_slot}"),
            message,
        },
    }
}

fn removal_failure_contract(failure: RemovalFailure) -> RecipeRemovalFailure {
    match failure {
        RemovalFailure::ReadFailed { path, reason } => RecipeRemovalFailure {
            code: "read_failed".into(),
            message: reason,
            path: Some(path.to_string_lossy().into_owned()),
            expected_sha256: None,
            observed_sha256: None,
        },
        RemovalFailure::HashMismatch {
            path,
            expected,
            observed,
        } => RecipeRemovalFailure {
            code: "hash_mismatch".into(),
            message: "Restored bytes do not match the expected SHA-256.".into(),
            path: Some(path.to_string_lossy().into_owned()),
            expected_sha256: Some(expected.to_string()),
            observed_sha256: Some(observed.to_string()),
        },
        RemovalFailure::MissingVerification => RecipeRemovalFailure {
            code: "missing_verification".into(),
            message: "Removal cannot complete without restoration verification.".into(),
            path: None,
            expected_sha256: None,
            observed_sha256: None,
        },
    }
}

fn append_removal_record(
    journal: &JournalStore,
    request: &RecipeRemovalRequest,
    result: &RecipeRemovalResult,
    duration: std::time::Duration,
) -> AppResult<()> {
    journal.append(&OperationRecord {
        id: request.operation_id.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        actor: OperationActor::Gui,
        kind: OperationKind::RecipeRemoval,
        status: if result.status == RecipeRemovalStatus::Removed {
            OperationStatus::Succeeded
        } else {
            OperationStatus::Failed
        },
        target: Some(request.recipe_id.clone()),
        summary: if result.status == RecipeRemovalStatus::Removed {
            "Recipe removal verified".into()
        } else {
            "Recipe removal verification failed".into()
        },
        details: BTreeMap::from([
            ("recipe_id".into(), request.recipe_id.clone()),
            ("receipt_id".into(), request.receipt_id.clone()),
            (
                "verified_restorations".into(),
                result.verified_restorations.len().to_string(),
            ),
            ("failures".into(), result.failures.len().to_string()),
        ]),
        duration_ms: Some(duration.as_millis().min(u128::from(u32::MAX)) as u32),
        backup_id: None,
        error: result
            .failures
            .first()
            .map(|failure| failure.message.clone()),
    })?;
    Ok(())
}

fn issue(code: impl Into<String>, message: impl Into<String>) -> RecipeIssue {
    RecipeIssue {
        code: code.into(),
        message: message.into(),
        context: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dlssync_contracts::{RecipeRestorationCheck, RecipeValidationRequest};
    use tempfile::tempdir;

    fn valid_recipe_json(destination: &str) -> String {
        serde_json::json!({
            "schema_version": 1,
            "kind": "recipe",
            "id": "renodx/example-game",
            "revision": 1,
            "upstream_version": "v1.0.0",
            "experimental": true,
            "origin": {
                "project_home": "https://example.com/project",
                "source_repository": "https://example.com/source",
                "release_channel": "https://example.com/releases",
                "author_identity": "example author",
                "observed_at": "2026-09-17T00:00:00Z"
            },
            "license": {
                "license_id": "MIT",
                "text_url": "https://example.com/license",
                "text_revision": "1",
                "observed_at": "2026-09-17T00:00:00Z",
                "redistribution": "permitted_with_conditions",
                "conditions": [],
                "permission_evidence": [],
                "acquisition_policy": "explicit_original_only"
            },
            "target": {
                "game_ids": ["example-game"],
                "executable_relative_path": "game.exe",
                "executable_sha256": "a".repeat(64),
                "game_build": "1.0",
                "architecture": "x64",
                "graphics_api": "dx12",
                "launch_mode": "standard",
                "required_capabilities": []
            },
            "artifacts": [{
                "id": "addon",
                "upstream_release_id": "release-1",
                "source_url": "https://example.com/addon.zip",
                "allowed_redirect_hosts": [],
                "sha256": "b".repeat(64),
                "size_bytes": 1,
                "format": "zip",
                "max_expanded_bytes": 1,
                "license_evidence": "license"
            }],
            "dependencies": [],
            "conflicts": [],
            "installed_files": [{
                "artifact_id": "addon",
                "archive_entry": "addon.dll",
                "destination": destination,
                "sha256": "c".repeat(64),
                "size_bytes": 1,
                "role": "data",
                "architecture": "x64",
                "install_rule": "create_only",
                "remove_rule": "undo_owned_if_hash_matches"
            }],
            "config_keys": [],
            "removal": {
                "require_committed_receipt": true,
                "require_current_hash": true,
                "retain_modified": true,
                "recursive_delete": false,
                "retain_undo_backups": true,
                "shared_dependency": "retain_while_referenced",
                "config_policy": "three_way_key_restore"
            },
            "evidence": [{
                "id": "game-test",
                "url": "https://example.com/test",
                "observed_at": "2026-09-17T00:00:00Z",
                "source_revision": "1",
                "claim": "fixture passed",
                "kind": "game_test",
                "environment": "fixture",
                "result": "pass"
            }],
            "compatibility_evidence": ["game-test"]
        })
        .to_string()
    }

    fn validation_request(json: String) -> RecipeValidationRequest {
        RecipeValidationRequest {
            expected_sha256: Sha256::digest(json.as_bytes()).to_string(),
            recipe_json: json,
        }
    }

    #[test]
    fn recipe_ipc_unknown_schema_has_machine_readable_reason() {
        let json = r#"{"schema_version":99}"#.to_string();
        let result = validate_recipe_contract(validation_request(json));
        assert!(!result.valid);
        assert_eq!(
            result.issue.as_ref().map(|issue| issue.code.as_str()),
            Some("unsupported_schema_version")
        );
    }

    #[test]
    fn recipe_ipc_requires_expected_integrity() {
        let result = validate_recipe_contract(RecipeValidationRequest {
            recipe_json: valid_recipe_json("addon.dll"),
            expected_sha256: String::new(),
        });
        assert_eq!(
            result.issue.as_ref().map(|issue| issue.code.as_str()),
            Some("missing_expected_integrity")
        );
    }

    #[test]
    fn recipe_ipc_official_component_conflict_names_both_parties() {
        let json = valid_recipe_json("nvngx_dlssd.dll");
        let result = preview_recipe_conflicts_contract(RecipeConflictPreviewRequest {
            recipe_json: json.clone(),
            expected_sha256: Sha256::digest(json.as_bytes()).to_string(),
            official_components: vec![OfficialComponentClaim {
                component_id: "dlss_rr".into(),
                path: "nvngx_dlssd.dll".into(),
            }],
            existing_recipe_claims: Vec::new(),
        });
        assert!(!result.allowed);
        assert_eq!(result.conflicts[0].recipe_id, "renodx/example-game");
        assert_eq!(result.conflicts[0].other_party, "dlss_rr");
        assert_eq!(result.conflicts[0].path, "nvngx_dlssd.dll");
    }

    #[test]
    fn recipe_ipc_removal_hash_mismatch_is_failed() {
        let dir = tempdir().unwrap();
        let restored = dir.path().join("restored.dll");
        std::fs::write(&restored, b"changed").unwrap();
        let result = remove_recipe_verified_contract(RecipeRemovalRequest {
            operation_id: "remove-1".into(),
            recipe_id: "renodx/example-game".into(),
            receipt_id: "receipt-1".into(),
            restorations: vec![RecipeRestorationCheck {
                path: restored.to_string_lossy().into_owned(),
                expected_sha256: Sha256::digest(b"original").to_string(),
            }],
        });
        assert_eq!(result.status, RecipeRemovalStatus::Failed);
        assert_eq!(result.failures[0].code, "hash_mismatch");
    }

    #[test]
    fn recipe_removal_result_is_appended_to_durable_journal() {
        let dir = tempdir().unwrap();
        let restored = dir.path().join("restored.dll");
        let changed = dir.path().join("changed.dll");
        std::fs::write(&restored, b"original").unwrap();
        std::fs::write(&changed, b"changed").unwrap();
        let request = RecipeRemovalRequest {
            operation_id: "remove-durable".into(),
            recipe_id: "renodx/example-game".into(),
            receipt_id: "receipt-1".into(),
            restorations: vec![
                RecipeRestorationCheck {
                    path: restored.to_string_lossy().into_owned(),
                    expected_sha256: Sha256::digest(b"original").to_string(),
                },
                RecipeRestorationCheck {
                    path: changed.to_string_lossy().into_owned(),
                    expected_sha256: Sha256::digest(b"original").to_string(),
                },
            ],
        };
        let result = remove_recipe_verified_contract(request.clone());
        assert_eq!(result.status, RecipeRemovalStatus::Failed);
        assert_eq!(result.verified_restorations.len(), 1);
        assert_eq!(result.failures.len(), 1);
        let journal = JournalStore::open(dir.path().join("operations.sqlite3")).unwrap();
        append_removal_record(&journal, &request, &result, std::time::Duration::ZERO).unwrap();
        let rows = journal
            .list(&dlssync_contracts::JournalFilter::default())
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, OperationKind::RecipeRemoval);
        assert_eq!(rows[0].status, OperationStatus::Failed);
        assert_eq!(rows[0].id, "remove-durable");
        assert_eq!(
            rows[0]
                .details
                .get("verified_restorations")
                .map(String::as_str),
            Some("1")
        );
        assert_eq!(
            rows[0].details.get("failures").map(String::as_str),
            Some("1")
        );
    }
}
