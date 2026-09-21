use dlssync_contracts::{
    AdapterClass, AdapterProvider, AmdDriverVersion, DrsReadbackState, DrsWriteState, FgPreset,
    InGameBehaviorState, IntelDriverVersion, NrPreset, NvidiaDriverVersion, PresetEvidence,
    ProviderDocumentationState, RrPreset, SrPreset, FG_PRESET_DEFAULT, PRESET_LATEST,
};

#[test]
fn phase5_ipc_presets_are_feature_specific_and_reject_foreign_values() {
    assert_eq!(SrPreset::try_from(15), Ok(SrPreset::O));
    assert!(SrPreset::try_from(16).is_err());

    assert_eq!(RrPreset::try_from(15), Ok(RrPreset::O));
    assert!(RrPreset::try_from(16).is_err());

    assert_eq!(FgPreset::try_from(26), Ok(FgPreset::Z));
    assert!(FgPreset::try_from(27).is_err());

    assert_eq!(NrPreset::try_from(4), Ok(NrPreset::D));
    assert!(NrPreset::try_from(5).is_err());
}

#[test]
fn phase5_ipc_fg_default_is_distinct_from_latest() {
    assert_eq!(FgPreset::try_from(FG_PRESET_DEFAULT), Ok(FgPreset::Default));
    assert_eq!(FgPreset::try_from(PRESET_LATEST), Ok(FgPreset::Latest));
    assert_ne!(FgPreset::Default, FgPreset::Latest);
    assert!(SrPreset::try_from(FG_PRESET_DEFAULT).is_err());
    assert!(RrPreset::try_from(FG_PRESET_DEFAULT).is_err());
    assert!(NrPreset::try_from(FG_PRESET_DEFAULT).is_err());
}

#[test]
fn phase5_ipc_preset_evidence_keeps_four_independent_states() {
    let evidence = PresetEvidence {
        write: DrsWriteState::Accepted,
        readback: DrsReadbackState::Matched,
        provider_documentation: ProviderDocumentationState::DocumentedNumericDefinition,
        in_game_behavior: InGameBehaviorState::Unknown,
    };

    let json = serde_json::to_value(&evidence).unwrap();
    assert_eq!(json["write"], "accepted");
    assert_eq!(json["readback"], "matched");
    assert_eq!(
        json["provider_documentation"],
        "documented_numeric_definition"
    );
    assert_eq!(json["in_game_behavior"], "unknown");
}

#[test]
fn phase5_ipc_provider_versions_and_gpu_class_remain_explicit() {
    let nvidia = AdapterProvider::Nvidia {
        driver_version: Some(NvidiaDriverVersion {
            branch: 610,
            revision: 47,
        }),
    };
    let amd = AdapterProvider::Amd {
        driver_version: Some(AmdDriverVersion {
            vendor_text: "25.6.1".into(),
        }),
    };
    let intel = AdapterProvider::Intel {
        driver_version: Some(IntelDriverVersion {
            vendor_text: "32.0.101.6734".into(),
        }),
    };

    assert_ne!(nvidia, amd);
    assert_ne!(amd, intel);
    assert_eq!(AdapterClass::Dedicated, AdapterClass::Dedicated);
    assert_eq!(AdapterClass::Integrated, AdapterClass::Integrated);
    assert_eq!(AdapterClass::Unknown, AdapterClass::Unknown);
}
