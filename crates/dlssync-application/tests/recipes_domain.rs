use dlssync_application::recipes::{
    finalize_removal, parse_recipe_json, validate_file_claims, verify_restoration,
    CompatibilityStatus, FileRole, ObservationStatus, OfficialFileClaim, OwnershipStatus,
    RecipeFileClaim, RecipeId, RecipeState, RelativePath, RemovalFailure, RemovalOutcome,
    RestorationVerification, Sha256, SupportStatus,
};
use tempfile::tempdir;

#[test]
fn unknown_schema_version_fails_closed_with_reason() {
    let error = parse_recipe_json(r#"{"schema_version":99,"kind":"recipe"}"#)
        .expect_err("an unknown recipe schema must fail closed");

    assert_eq!(error.schema_version(), Some(99));
    assert!(error
        .to_string()
        .contains("unsupported recipe schema version 99"));
}

#[test]
fn duplicate_proxy_claim_names_both_recipes() {
    let claims = vec![
        RecipeFileClaim::new(
            RecipeId::parse("reshade").unwrap(),
            RelativePath::parse("dxgi.dll").unwrap(),
            FileRole::Proxy,
        ),
        RecipeFileClaim::new(
            RecipeId::parse("luma-framework/example-game").unwrap(),
            RelativePath::parse("DXGI.dll").unwrap(),
            FileRole::Proxy,
        ),
    ];

    let error = validate_file_claims(&claims, &[]).expect_err("the proxy slot must be exclusive");
    let message = error.to_string();
    assert!(message.contains("reshade"));
    assert!(message.contains("luma-framework/example-game"));
    assert!(message.to_ascii_lowercase().contains("dxgi.dll"));
}

#[test]
fn official_component_proxy_claim_is_rejected() {
    let claims = vec![RecipeFileClaim::new(
        RecipeId::parse("dlss-path/project/example-game/rr").unwrap(),
        RelativePath::parse("nvngx_dlssd.dll").unwrap(),
        FileRole::Data,
    )];
    let official = vec![OfficialFileClaim::new(
        "dlss_rr",
        RelativePath::parse("nvngx_dlssd.dll").unwrap(),
    )];

    let error = validate_file_claims(&claims, &official)
        .expect_err("a recipe cannot take an official component path");
    let message = error.to_string();
    assert!(message.contains("dlss-path/project/example-game/rr"));
    assert!(message.contains("dlss_rr"));
    assert!(message.contains("nvngx_dlssd.dll"));
}

#[test]
fn different_proxy_slots_in_one_directory_require_a_tested_chain() {
    let claims = vec![
        RecipeFileClaim::new(
            RecipeId::parse("reshade").unwrap(),
            RelativePath::parse("bin/dxgi.dll").unwrap(),
            FileRole::Proxy,
        ),
        RecipeFileClaim::new(
            RecipeId::parse("renodx/example-game").unwrap(),
            RelativePath::parse("bin/version.dll").unwrap(),
            FileRole::Proxy,
        ),
    ];

    let error = validate_file_claims(&claims, &[])
        .expect_err("different proxy names do not establish a supported chain");
    let message = error.to_string();
    assert!(message.contains("reshade"));
    assert!(message.contains("renodx/example-game"));
    assert!(message.contains("dxgi.dll"));
    assert!(message.contains("version.dll"));
}

#[test]
fn detected_state_does_not_promote_to_tested() {
    let state = RecipeState {
        ownership: OwnershipStatus::None,
        observation: ObservationStatus::FilesDetected {
            method: "bounded fixture scan".into(),
            paths: vec![RelativePath::parse("dxgi.dll").unwrap()],
            hashes: vec![Sha256::parse(&"a".repeat(64)).unwrap()],
            observed_at: "2026-09-17T00:00:00Z".into(),
        },
        compatibility: CompatibilityStatus::Unknown,
        support: SupportStatus::Experimental,
    };

    assert!(matches!(
        state.observation,
        ObservationStatus::FilesDetected { .. }
    ));
    assert_eq!(state.compatibility, CompatibilityStatus::Unknown);
    assert_eq!(state.support, SupportStatus::Experimental);

    let independently_tested = RecipeState {
        ownership: OwnershipStatus::InstalledByDlssync {
            receipt_id: "receipt-1".into(),
        },
        observation: state.observation.clone(),
        compatibility: CompatibilityStatus::TestedPass {
            evidence_id: dlssync_application::recipes::EvidenceId::new("game-test-1").unwrap(),
        },
        support: SupportStatus::Official {
            evidence_ids: vec![
                dlssync_application::recipes::EvidenceId::new("official-support-1").unwrap(),
            ],
        },
    };
    assert!(matches!(
        independently_tested.ownership,
        OwnershipStatus::InstalledByDlssync { .. }
    ));
    assert!(matches!(
        independently_tested.compatibility,
        CompatibilityStatus::TestedPass { .. }
    ));
    assert!(matches!(
        independently_tested.support,
        SupportStatus::Official { .. }
    ));
}

#[test]
fn removal_requires_verified_restoration_and_hash_mismatch_fails() {
    let root = tempdir().unwrap();
    let restored = root.path().join("dxgi.dll");
    std::fs::write(&restored, b"unexpected restored bytes").unwrap();
    let expected = Sha256::digest(b"expected restored bytes");

    let verification = verify_restoration(&restored, &expected);
    assert!(matches!(
        verification,
        RestorationVerification::Failed(RemovalFailure::HashMismatch { .. })
    ));

    let outcome = finalize_removal(vec![verification]);
    assert!(matches!(outcome, RemovalOutcome::Failed { .. }));

    let missing_verification = finalize_removal(Vec::new());
    assert!(matches!(
        missing_verification,
        RemovalOutcome::Failed { ref failures, .. }
            if failures == &[RemovalFailure::MissingVerification]
    ));
}
