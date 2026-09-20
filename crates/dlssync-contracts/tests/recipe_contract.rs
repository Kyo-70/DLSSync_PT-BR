use dlssync_contracts::{
    RecipeCompatibilityState, RecipeObservationState, RecipeOwnershipState, RecipeState,
    RecipeSupportState,
};

#[test]
fn recipe_contract_keeps_installed_detected_tested_and_support_independent() {
    let state = RecipeState {
        ownership: RecipeOwnershipState::InstalledByDlssync {
            receipt_id: "receipt-1".into(),
        },
        observation: RecipeObservationState::FilesDetected {
            method: "hash_match".into(),
            paths: vec!["dxgi.dll".into()],
            hashes: vec!["a".repeat(64)],
            observed_at: "2026-09-17T00:00:00Z".into(),
        },
        compatibility: RecipeCompatibilityState::DocumentedOnly {
            evidence_ids: vec!["game-docs".into()],
        },
        support: RecipeSupportState::Official {
            evidence_ids: vec!["author-docs".into()],
        },
    };

    let json = serde_json::to_value(state).unwrap();
    assert_eq!(json["ownership"]["status"], "installed_by_dlssync");
    assert_eq!(json["observation"]["status"], "files_detected");
    assert_eq!(json["compatibility"]["status"], "documented_only");
    assert_eq!(json["support"]["status"], "official");

    let tested = RecipeState {
        ownership: RecipeOwnershipState::None,
        observation: RecipeObservationState::NotObserved,
        compatibility: RecipeCompatibilityState::TestedPass {
            evidence_id: "game-test".into(),
        },
        support: RecipeSupportState::Unknown,
    };
    let tested_json = serde_json::to_value(tested).unwrap();
    assert_eq!(tested_json["ownership"]["status"], "none");
    assert_eq!(tested_json["observation"]["status"], "not_observed");
    assert_eq!(tested_json["compatibility"]["status"], "tested_pass");
    assert_eq!(tested_json["support"]["status"], "unknown");
}
