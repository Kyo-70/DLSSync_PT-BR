use dlssync_contracts::{
    OfficialComponentClaim, RecipeConflictPreviewRequest, RecipeRemovalRequest,
    RecipeRestorationCheck, RecipeValidationRequest,
};
use dlssync_lib::{
    preview_recipe_conflicts_contract, remove_recipe_verified_contract, validate_recipe_contract,
};

fn unknown_schema_json() -> String {
    r#"{"schema_version":99}"#.to_string()
}

#[test]
fn recipe_ipc_unknown_schema_has_machine_readable_reason() {
    let input = unknown_schema_json();
    let result = validate_recipe_contract(RecipeValidationRequest {
        expected_sha256: dlssync_application::recipes::Sha256::digest(input.as_bytes()).to_string(),
        recipe_json: input,
    });
    assert!(!result.valid);
    assert_eq!(
        result.issue.as_ref().map(|issue| issue.code.as_str()),
        Some("unsupported_schema_version")
    );
}

#[test]
fn recipe_ipc_official_component_conflict_names_both_parties() {
    let fixture = include_str!("fixtures/recipe-v1.json");
    let result = preview_recipe_conflicts_contract(RecipeConflictPreviewRequest {
        recipe_json: fixture.into(),
        expected_sha256: dlssync_application::recipes::Sha256::digest(fixture.as_bytes())
            .to_string(),
        official_components: vec![OfficialComponentClaim {
            component_id: "dlss_rr".into(),
            path: "nvngx_dlssd.dll".into(),
        }],
        existing_recipe_claims: Vec::new(),
    });
    assert!(!result.allowed);
    let conflict = &result.conflicts[0];
    assert_eq!(conflict.recipe_id, "renodx/example-game");
    assert_eq!(conflict.other_party, "dlss_rr");
    assert_eq!(conflict.path, "nvngx_dlssd.dll");
}

#[test]
fn recipe_ipc_removal_hash_mismatch_is_failed() {
    let dir = tempfile::tempdir().unwrap();
    let restored = dir.path().join("restored.dll");
    std::fs::write(&restored, b"changed").unwrap();
    let result = remove_recipe_verified_contract(RecipeRemovalRequest {
        operation_id: "remove-1".into(),
        recipe_id: "renodx/example-game".into(),
        receipt_id: "receipt-1".into(),
        restorations: vec![RecipeRestorationCheck {
            path: restored.to_string_lossy().into_owned(),
            expected_sha256: dlssync_application::recipes::Sha256::digest(b"original").to_string(),
        }],
    });
    assert_eq!(result.status.as_str(), "failed");
    assert_eq!(result.failures[0].code, "hash_mismatch");
}
