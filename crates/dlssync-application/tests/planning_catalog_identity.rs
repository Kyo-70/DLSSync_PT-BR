use dlssync_application::execution::{validate_prepared_plan, PreparedFile};
use dlssync_application::{
    build_verified_update_plan, plan_items, scan_path, validate_update_plan,
};
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// Minimal x64 DLL with a real RT_VERSION resource: PE file version 1.3.0.7.
fn versioned_pe(path: &Path) -> std::io::Result<()> {
    let mut bytes = vec![0u8; 1024];
    fn word(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn dword(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[..2].copy_from_slice(b"MZ");
    dword(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    word(&mut bytes, 132, 0x8664);
    word(&mut bytes, 134, 1);
    word(&mut bytes, 148, 240);
    word(&mut bytes, 150, 0x2022);
    word(&mut bytes, 152, 0x20b);
    dword(&mut bytes, 184, 4096);
    dword(&mut bytes, 188, 512);
    dword(&mut bytes, 208, 8192);
    dword(&mut bytes, 212, 512);
    dword(&mut bytes, 260, 16);
    dword(&mut bytes, 280, 4096);
    dword(&mut bytes, 284, 512);
    bytes[392..400].copy_from_slice(b".rsrc\0\0\0");
    dword(&mut bytes, 400, 512);
    dword(&mut bytes, 404, 4096);
    dword(&mut bytes, 408, 512);
    dword(&mut bytes, 412, 512);
    dword(&mut bytes, 428, 0x40000040);
    for (offset, id, next) in [(0, 16, 0x80000018), (24, 1, 0x80000030), (48, 1033, 72)] {
        word(&mut bytes, 512 + offset + 14, 1);
        dword(&mut bytes, 512 + offset + 16, id);
        dword(&mut bytes, 512 + offset + 20, next);
    }
    dword(&mut bytes, 584, 4096 + 88);
    dword(&mut bytes, 588, 92);
    word(&mut bytes, 600, 92);
    word(&mut bytes, 602, 52);
    for (index, value) in "VS_VERSION_INFO\0".encode_utf16().enumerate() {
        word(&mut bytes, 606 + index * 2, value);
    }
    dword(&mut bytes, 640, 0xfeef04bd);
    dword(&mut bytes, 644, 0x10000);
    dword(&mut bytes, 648, 0x10003);
    dword(&mut bytes, 652, 7);
    dword(&mut bytes, 656, 0x10003);
    dword(&mut bytes, 660, 7);
    std::fs::write(path, bytes)
}

#[test]
fn bundled_streamline_set_plans_common_as_a_distinct_dependency() -> TestResult {
    let root = tempfile::tempdir()?;
    for name in ["sl.dlss.dll", "sl.common.dll", "sl.interposer.dll"] {
        versioned_pe(&root.path().join(name))?;
    }
    let catalog = dll_catalog::embedded_fallback_catalog()?;
    let game = scan_path(root.path())?;
    let games = [game];
    let items = plan_items(&catalog, &games, root.path(), None)
        .into_iter()
        .filter(|item| item.family == "sl_dlss_sr" && item.target_version == "2.14.1")
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 1);
    let plan = build_verified_update_plan(&catalog, &games, items, &root.path().join("backups"))?;
    assert_eq!(plan.changes.len(), 3);
    assert!(plan
        .changes
        .iter()
        .any(|change| change.artifact.family == "streamline_common"
            && change.artifact.filename == "sl.common.dll"
            && change.added_as_dependency));
    validate_update_plan(&catalog, &plan)?;
    Ok(())
}

#[test]
fn legacy_package_label_accepts_verified_four_part_pe_and_rejects_tampering() -> TestResult {
    let root = tempfile::tempdir()?;
    let staged_dir = tempfile::tempdir()?;
    let mut catalog = dll_catalog::embedded_fallback_catalog()?;
    // Keep the bundled DirectStorage 1.3.0 package metadata, substituting only
    // fixture byte hashes/sizes. No network or proprietary DLL fixture is needed.
    catalog.vendors.retain(|vendor, _| vendor == "microsoft");
    for family in catalog
        .vendors
        .values_mut()
        .flat_map(|families| families.values_mut())
    {
        family.releases.retain(|release| release.version == "1.3.0");
        assert!(!family.releases.is_empty());
        for release in &mut family.releases {
            assert!(release.artifact.is_none());
            versioned_pe(&root.path().join(&release.filename))?;
            let staged = staged_dir.path().join(&release.filename);
            versioned_pe(&staged)?;
            release.sha256 = dll_catalog::hex_sha256_file(&staged)?;
            release.size_bytes = std::fs::metadata(&staged)?.len();
        }
    }
    let games = [scan_path(root.path())?];
    assert!(games[0]
        .components
        .iter()
        .all(|component| component.current_version.as_deref() == Some("1.3.0.7")));
    let items = plan_items(&catalog, &games, root.path(), None);
    let plan = build_verified_update_plan(&catalog, &games, items, &root.path().join("backups"))?;
    assert_eq!(plan.changes.len(), 2);
    let mut prepared = Vec::new();
    for change in &plan.changes {
        let item = plan
            .items
            .iter()
            .find(|item| item.dll_path == change.precondition.absolute_path)
            .ok_or("missing plan item")?;
        let staged = staged_dir.path().join(&change.artifact.filename);
        prepared.push(PreparedFile {
            target: item.dll_path.clone().into(),
            backup: item.backup_path.clone().into(),
            previous_sha256: change.precondition.observed_hash.digest.clone(),
            expected_sha256: dll_catalog::hex_sha256_file(&staged)?,
            expected_version: Some(pe_version::read_dll_version(&staged)?.file_version),
            staged,
        });
    }
    validate_prepared_plan(&plan, &prepared)?;
    for change in &plan.changes {
        assert_eq!(change.artifact.package_version, "1.3.0");
        assert_eq!(change.artifact.file_version, None);
    }
    let mut attested = plan.clone();
    for change in &mut attested.changes {
        change.artifact.file_version = Some("1.3.0.7".into());
    }
    validate_prepared_plan(&attested, &prepared)?;
    attested.changes[0].artifact.file_version = Some("1.3.0.8".into());
    assert!(validate_prepared_plan(&attested, &prepared).is_err());
    std::fs::write(&prepared[0].staged, b"tampered")?;
    assert!(validate_prepared_plan(&plan, &prepared).is_err());
    Ok(())
}

#[test]
fn scanner_catalog_keys_preserve_distinct_artifacts_and_unknown_vendor() {
    use dll_scanner::DllFamily;
    for (family, key) in [
        (DllFamily::Streamline, "streamline"),
        (DllFamily::StreamlineCommon, "streamline_common"),
        (DllFamily::StreamlinePcl, "streamline_pcl"),
        (DllFamily::StreamlineNis, "streamline_nis"),
        (DllFamily::StreamlineDirectSr, "streamline_direct_sr"),
        (DllFamily::XessSrDx11, "xess_sr_dx11"),
        (DllFamily::FsrLoader, "fsr_loader"),
        (DllFamily::FsrUpscaler, "fsr_upscaler"),
        (DllFamily::FsrUpscalerVk, "fsr_upscaler_vk"),
    ] {
        assert_eq!(family.catalog_key(), key);
        assert_eq!(dll_scanner::family_vendor(key), Some(family.vendor()));
    }
    assert_eq!(dll_scanner::family_vendor("unknown_family"), None);
}
