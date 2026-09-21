use nvapi_drs::settings::ids;
use nvapi_drs::{
    assess_override_write, AdapterClass, AdapterObservation, AdapterProvider, CapabilityEvidence,
    DrsReadbackState, DrsWriteState, FgPreset, HostPlatform, InGameBehaviorState, NrPreset,
    NvapiRuntime, NvidiaDriverVersion, PresetEvidence, ProviderDocumentationState, RrPreset,
    SrPreset, SupportState, FEATURE_SETTING_IDS,
};

#[test]
fn phase5_exact_ids_and_feature_specific_ranges() {
    assert_eq!(ids::DLSS_SR_ENABLE_OVERRIDE, 0x10E4_1E01);
    assert_eq!(ids::DLSS_SR_FORCED_PRESET, 0x10E4_1DF3);
    assert_eq!(ids::DLSS_RR_ENABLE_OVERRIDE, 0x10E4_1E02);
    assert_eq!(ids::DLSS_RR_FORCED_PRESET, 0x10E4_1DF7);
    assert_eq!(ids::DLSS_FG_ENABLE_OVERRIDE, 0x10E4_1E03);
    assert_eq!(ids::DLSS_FG_FORCED_PRESET, 0x10E4_1DF1);
    assert_eq!(ids::DLSS_NR_ENABLE_OVERRIDE, 0x10E4_1E04);
    assert_eq!(ids::DLSS_NR_FORCED_PRESET, 0x10E4_1DF8);
    assert_eq!(FEATURE_SETTING_IDS.len(), 4);
    assert_eq!(
        FEATURE_SETTING_IDS[0].override_id,
        ids::DLSS_SR_ENABLE_OVERRIDE
    );
    assert_eq!(FEATURE_SETTING_IDS[0].preset_id, ids::DLSS_SR_FORCED_PRESET);
    assert_eq!(
        FEATURE_SETTING_IDS[1].override_id,
        ids::DLSS_RR_ENABLE_OVERRIDE
    );
    assert_eq!(FEATURE_SETTING_IDS[1].preset_id, ids::DLSS_RR_FORCED_PRESET);
    assert_eq!(
        FEATURE_SETTING_IDS[2].override_id,
        ids::DLSS_FG_ENABLE_OVERRIDE
    );
    assert_eq!(FEATURE_SETTING_IDS[2].preset_id, ids::DLSS_FG_FORCED_PRESET);
    assert_eq!(
        FEATURE_SETTING_IDS[3].override_id,
        ids::DLSS_NR_ENABLE_OVERRIDE
    );
    assert_eq!(FEATURE_SETTING_IDS[3].preset_id, ids::DLSS_NR_FORCED_PRESET);

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
fn phase5_fg_default_sentinel_is_not_generic_latest() {
    const FG_DEFAULT: u32 = 0x00FF_FFFE;
    const LATEST: u32 = 0x00FF_FFFF;

    assert_eq!(FgPreset::try_from(FG_DEFAULT), Ok(FgPreset::Default));
    assert_eq!(FgPreset::try_from(LATEST), Ok(FgPreset::Latest));
    assert!(SrPreset::try_from(FG_DEFAULT).is_err());
    assert!(RrPreset::try_from(FG_DEFAULT).is_err());
    assert!(NrPreset::try_from(FG_DEFAULT).is_err());
    assert_eq!(SrPreset::try_from(LATEST), Ok(SrPreset::Latest));
    assert_eq!(RrPreset::try_from(LATEST), Ok(RrPreset::Latest));
    assert_eq!(NrPreset::try_from(LATEST), Ok(NrPreset::Latest));
}

#[test]
fn phase5_sr_and_rr_descriptions_are_distinct() {
    assert_ne!(SrPreset::D.description(), RrPreset::D.description());
    assert_ne!(SrPreset::E.description(), RrPreset::E.description());
    assert_ne!(SrPreset::F.description(), RrPreset::F.description());
    assert_ne!(SrPreset::J.description(), RrPreset::J.description());
    assert_ne!(SrPreset::K.description(), RrPreset::K.description());
    assert!(RrPreset::D.description().contains("transformer"));
    assert!(RrPreset::E.description().contains("transformer"));
    assert!(RrPreset::F.description().contains("default transformer"));
    assert!(RrPreset::J.description().contains("falls back"));
    assert!(RrPreset::K.description().contains("falls back"));
}

fn known_capability_evidence() -> CapabilityEvidence {
    CapabilityEvidence {
        host: HostPlatform::Windows,
        nvapi: NvapiRuntime::Available {
            version: "R999-test".to_string(),
        },
        adapter: AdapterObservation {
            class: AdapterClass::Dedicated,
            provider: AdapterProvider::Nvidia {
                driver_version: Some(NvidiaDriverVersion {
                    branch: 999,
                    revision: 1,
                }),
            },
            hardware_recognized: true,
        },
        provider_applicability: SupportState::Confirmed,
        game_integration: SupportState::Confirmed,
        runtime_stack: SupportState::Confirmed,
        preset_runtime_mapping: SupportState::Confirmed,
    }
}

#[test]
fn phase5_capabilities_fail_closed_without_nvapi_or_known_version() {
    let mut evidence = known_capability_evidence();
    evidence.nvapi = NvapiRuntime::Unavailable {
        reason: "nvapi64.dll missing".to_string(),
    };
    assert!(!assess_override_write(&evidence).write_eligible);

    evidence = known_capability_evidence();
    evidence.nvapi = NvapiRuntime::VersionUnknown {
        detail: "interface version query failed".to_string(),
    };
    assert!(!assess_override_write(&evidence).write_eligible);

    evidence = known_capability_evidence();
    evidence.host = HostPlatform::Other {
        name: "linux".to_string(),
    };
    assert!(!assess_override_write(&evidence).write_eligible);

    evidence = known_capability_evidence();
    evidence.adapter.hardware_recognized = false;
    assert!(!assess_override_write(&evidence).write_eligible);
}

#[test]
fn phase5_write_readback_and_documentation_do_not_imply_in_game_behavior() {
    let evidence = PresetEvidence {
        write: DrsWriteState::Accepted,
        readback: DrsReadbackState::Matched,
        provider_documentation: ProviderDocumentationState::DocumentedNumericDefinition,
        in_game_behavior: InGameBehaviorState::Unknown,
    };

    assert_eq!(evidence.write, DrsWriteState::Accepted);
    assert_eq!(evidence.readback, DrsReadbackState::Matched);
    assert_eq!(
        evidence.provider_documentation,
        ProviderDocumentationState::DocumentedNumericDefinition
    );
    assert_eq!(evidence.in_game_behavior, InGameBehaviorState::Unknown);
    assert!(!evidence.in_game_behavior.was_observed_effective());
}

#[test]
fn phase5_adapter_provider_and_class_stay_explicit() {
    let nvidia = AdapterObservation {
        class: AdapterClass::Dedicated,
        provider: AdapterProvider::Nvidia {
            driver_version: Some(NvidiaDriverVersion {
                branch: 999,
                revision: 1,
            }),
        },
        hardware_recognized: true,
    };
    let amd = AdapterObservation {
        class: AdapterClass::Integrated,
        provider: AdapterProvider::Amd {
            driver_version: Some(nvapi_drs::AmdDriverVersion {
                vendor_text: "vendor-specific-amd-version".to_string(),
            }),
        },
        hardware_recognized: true,
    };

    assert_eq!(nvidia.class, AdapterClass::Dedicated);
    assert_eq!(amd.class, AdapterClass::Integrated);
    assert!(matches!(nvidia.provider, AdapterProvider::Nvidia { .. }));
    assert!(matches!(amd.provider, AdapterProvider::Amd { .. }));

    let mut evidence = known_capability_evidence();
    evidence.adapter = amd;
    let decision = assess_override_write(&evidence);
    assert!(!decision.write_eligible);
}
