#[cfg(windows)]
fn main() -> Result<(), String> {
    let runtime = nvapi_drs::ffi::profile_runtime()?;
    println!(
        "NVAPI: {}\nDriver: {} ({})",
        runtime.interface_version, runtime.driver_version, runtime.driver_branch
    );
    println!(
        "Supported DLSS IDs: {:?}",
        nvapi_drs::settings::RESETTABLE_IDS
            .iter()
            .filter(|id| runtime.supported_setting_ids.contains(id))
            .collect::<Vec<_>>()
    );
    if let Some(nonce) = std::env::args().nth(1) {
        let settings = [
            nvapi_drs::DrsSetting::sr_preset(nvapi_drs::SrPreset::Latest),
            nvapi_drs::DrsSetting::rr_preset(nvapi_drs::RrPreset::Latest),
            nvapi_drs::DrsSetting::fg_preset(nvapi_drs::FgPreset::Latest),
        ];
        let observed = nvapi_drs::ffi::validate_isolated_profile(&nonce, &settings)?;
        println!("Persisted readback: {observed:?}\nValidation profile absence verified.");
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("NVAPI validation requires Windows.");
}
