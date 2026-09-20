use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const SHA_SKIP_THRESHOLD_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ObservationErrorCause {
    Walk,
    Hash,
    PeParse,
    AccessDenied,
    Missing,
    ChangedDuringRead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStage {
    Walk,
    Hash,
    PeParse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct InstallObservationError {
    pub path: PathBuf,
    pub cause: ObservationErrorCause,
    pub stage: ObservationStage,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct InstallObservation {
    pub records: Vec<DllRecord>,
    pub complete: bool,
    pub errors: Vec<InstallObservationError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DllFamily {
    DlssSr,
    DlssFg,
    DlssRr,
    SlDlssSr,
    SlDlssFg,
    SlDlssRr,
    Streamline,
    StreamlineCommon,
    StreamlinePcl,
    StreamlineNis,
    StreamlineDirectSr,
    Reflex,
    XessSr,
    XessSrDx11,
    XessFg,
    Xell,
    FsrUpscaler,
    FsrUpscalerVk,
    FsrFg,
    FsrLoader,
    FsrDenoiser,
    DirectStorage,
    DirectStorageCore,
}

impl DllFamily {
    pub fn vendor(&self) -> &'static str {
        match self {
            DllFamily::DlssSr
            | DllFamily::DlssFg
            | DllFamily::DlssRr
            | DllFamily::SlDlssSr
            | DllFamily::SlDlssFg
            | DllFamily::SlDlssRr
            | DllFamily::Streamline
            | DllFamily::StreamlineCommon
            | DllFamily::StreamlinePcl
            | DllFamily::StreamlineNis
            | DllFamily::StreamlineDirectSr
            | DllFamily::Reflex => "nvidia",
            DllFamily::XessSr | DllFamily::XessSrDx11 | DllFamily::XessFg | DllFamily::Xell => {
                "intel"
            }
            DllFamily::FsrUpscaler
            | DllFamily::FsrUpscalerVk
            | DllFamily::FsrFg
            | DllFamily::FsrLoader
            | DllFamily::FsrDenoiser => "amd",
            DllFamily::DirectStorage | DllFamily::DirectStorageCore => "microsoft",
        }
    }

    pub fn catalog_key(&self) -> &'static str {
        match self {
            DllFamily::DlssSr => "dlss_sr",
            DllFamily::DlssFg => "dlss_fg",
            DllFamily::DlssRr => "dlss_rr",
            DllFamily::SlDlssSr => "sl_dlss_sr",
            DllFamily::SlDlssFg => "sl_dlss_fg",
            DllFamily::SlDlssRr => "sl_dlss_rr",
            // Package membership does not make these distinct DLL identities aliases.
            DllFamily::Streamline => "streamline",
            DllFamily::StreamlineCommon => "streamline_common",
            DllFamily::StreamlinePcl => "streamline_pcl",
            DllFamily::StreamlineNis => "streamline_nis",
            DllFamily::StreamlineDirectSr => "streamline_direct_sr",
            DllFamily::Reflex => "reflex",
            DllFamily::XessSr => "xess_sr",
            DllFamily::XessSrDx11 => "xess_sr_dx11",
            DllFamily::XessFg => "xess_fg",
            DllFamily::Xell => "xell",
            DllFamily::FsrUpscaler => "fsr_upscaler",
            DllFamily::FsrUpscalerVk => "fsr_upscaler_vk",
            DllFamily::FsrLoader => "fsr_loader",
            DllFamily::FsrFg => "fsr_fg",
            DllFamily::FsrDenoiser => "fsr_denoiser",
            DllFamily::DirectStorage => "direct_storage",
            DllFamily::DirectStorageCore => "direct_storage_core",
        }
    }
}

/// Resolve both catalog keys and scanner-family keys through the same vendor table.
pub fn family_vendor(key: &str) -> Option<&'static str> {
    DllFamily::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new(key))
        .ok()
        .map(|family| family.vendor())
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct DllRecord {
    pub family: DllFamily,
    pub path: PathBuf,
    pub current_version: Option<String>,
    pub file_description: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
}

/// Marker DLLs that signal a DLSS/FG injector mod (DLSS Enabler, OptiScaler,
/// dlssg-to-fsr3) owns the Streamline set in this game tree. Matched
/// case-insensitively by EXACT file name — a bare `nvngx.dll` is an injector
/// proxy (real games ship `nvngx_dlss.dll` with the underscore), so generic
/// loaders like `dxgi.dll`/`version.dll` are deliberately excluded to keep
/// false positives at zero.
pub const DLSS_ENABLER_MARKERS: &[&str] = &[
    "dlss-enabler.dll",
    "dlss-enabler-upscaler.dll",
    "nvngx-wrapper.dll",
    "nvngx.dll",
    "optiscaler.dll",
    "dlssg_to_fsr3_amd_is_better.dll",
    "dlss-enabler.log",
    "dlss-enabler.asi",
];

/// Bounded depth for the DLSS Enabler scan. The enabler DLLs sit near the game
/// root, so a shallow early-exit walk is far cheaper than the full DLL scan and
/// keeps enabler detection a separate, non-breaking command from `scan_install`.
const DLSS_ENABLER_SCAN_DEPTH: usize = 4;

/// Whether DLSS Enabler is installed in this game tree (bounded, early-exit walk).
/// Kept separate from `scan_install` so the DLL-detection contract stays a plain
/// `Vec<DllRecord>` — a partially-updated frontend/backend can never blank the
/// whole library on a shape mismatch.
pub fn detect_dlss_enabler(root: &Path) -> bool {
    enabler_present_within(root, 0)
}

fn enabler_present_within(dir: &Path, depth: usize) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_file() {
            if is_dlss_enabler_marker(&entry.file_name().to_string_lossy()) {
                return true;
            }
        } else if file_type.is_dir() && depth < DLSS_ENABLER_SCAN_DEPTH {
            let name = entry.file_name();
            if !SKIP_DIRS.contains(name.to_string_lossy().as_ref()) {
                subdirs.push(entry.path());
            }
        }
    }
    subdirs.iter().any(|d| enabler_present_within(d, depth + 1))
}

/// An NVIDIA Streamline plugin/interposer DLL (`sl.*.dll`). These form a single
/// version-locked set: swapping one component (e.g. `sl.dlss_g.dll`) without the
/// matching `sl.interposer.dll` mismatches Streamline's ABI struct versions and
/// crashes the game on launch. Distinct from the NGX runtime DLLs
/// (`nvngx_dlss.dll`), which the driver loads independently and are safe to swap.
pub fn is_streamline_plugin(filename: &str) -> bool {
    let lower = filename.to_ascii_lowercase();
    lower.starts_with("sl.") && lower.ends_with(".dll")
}

/// Whether a single file name is a DLSS Enabler marker.
pub fn is_dlss_enabler_marker(filename: &str) -> bool {
    let lower = filename.to_ascii_lowercase();
    DLSS_ENABLER_MARKERS.iter().any(|m| lower == *m)
}

/// Look for DLSS Enabler markers in `start` and up to `max_ancestors` parent
/// directories. Streamline plugins live in deeply nested engine plugin folders
/// while the enabler DLL sits near the game root, so the apply-time guard walks
/// upward from the DLL being replaced.
pub fn dlss_enabler_present_near(start: &Path, max_ancestors: usize) -> bool {
    let mut dir = Some(start);
    let mut hops = 0usize;
    while let Some(current) = dir {
        if dir_has_dlss_enabler(current) {
            return true;
        }
        if hops >= max_ancestors {
            break;
        }
        hops += 1;
        dir = current.parent();
    }
    false
}

fn dir_has_dlss_enabler(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        if is_dlss_enabler_marker(&entry.file_name().to_string_lossy()) {
            return true;
        }
    }
    false
}

static KNOWN_DLLS: Lazy<Vec<(&'static str, DllFamily)>> = Lazy::new(|| {
    vec![
        ("nvngx_dlss.dll", DllFamily::DlssSr),
        ("nvngx_dlssg.dll", DllFamily::DlssFg),
        ("nvngx_dlssd.dll", DllFamily::DlssRr),
        ("sl.dlss.dll", DllFamily::SlDlssSr),
        ("sl.dlss_g.dll", DllFamily::SlDlssFg),
        ("sl.dlss_d.dll", DllFamily::SlDlssRr),
        ("sl.interposer.dll", DllFamily::Streamline),
        ("sl.common.dll", DllFamily::StreamlineCommon),
        ("sl.pcl.dll", DllFamily::StreamlinePcl),
        ("sl.nis.dll", DllFamily::StreamlineNis),
        ("sl.directsr.dll", DllFamily::StreamlineDirectSr),
        ("sl.reflex.dll", DllFamily::Reflex),
        ("libxess.dll", DllFamily::XessSr),
        ("libxess_dx11.dll", DllFamily::XessSrDx11),
        ("libxess_fg.dll", DllFamily::XessFg),
        ("libxell.dll", DllFamily::Xell),
        ("amd_fidelityfx_dx12.dll", DllFamily::FsrUpscaler),
        ("amd_fidelityfx_vk.dll", DllFamily::FsrUpscalerVk),
        ("amd_fidelityfx_upscaler_dx12.dll", DllFamily::FsrUpscaler),
        ("amd_fidelityfx_framegeneration_dx12.dll", DllFamily::FsrFg),
        ("amd_fidelityfx_loader_dx12.dll", DllFamily::FsrLoader),
        ("ffx_fsr3upscaler_x64.dll", DllFamily::FsrUpscaler),
        ("ffx_frameinterpolation_x64.dll", DllFamily::FsrFg),
        ("amd_fidelityfx_denoiser_dx12.dll", DllFamily::FsrDenoiser),
        ("dstorage.dll", DllFamily::DirectStorage),
        ("dstoragecore.dll", DllFamily::DirectStorageCore),
    ]
});

/// Recognize a catalog-owned DLL name, including files not yet present on disk.
pub fn known_dll_family(filename: &str) -> Option<DllFamily> {
    KNOWN_DLLS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(filename))
        .map(|(_, family)| *family)
}

static SKIP_DIRS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "$Recycle.Bin",
        "System Volume Information",
        ".git",
        "node_modules",
        "__pycache__",
        ".vs",
        ".idea",
        "Logs",
        "Cache",
        "Crashes",
    ]
    .into_iter()
    .collect()
});

pub fn observe_install(root: &Path) -> InstallObservation {
    let mut records = Vec::new();
    let mut errors = Vec::new();
    let walker = jwalk::WalkDir::new(root)
        .skip_hidden(false)
        .process_read_dir(|_, _, _, children| {
            children.retain(|child| {
                if let Ok(c) = child {
                    if c.file_type().is_dir() {
                        let n = c.file_name();
                        return !SKIP_DIRS.contains(n.to_string_lossy().as_ref());
                    }
                }
                true
            });
        });
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(error) => {
                let path = error
                    .path()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| root.to_path_buf());
                let kind = error
                    .io_error()
                    .map(std::io::Error::kind)
                    .unwrap_or(std::io::ErrorKind::Other);
                errors.push(observation_io_error(
                    path,
                    ObservationStage::Walk,
                    kind,
                    error.to_string(),
                ));
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name();
        let lname = name.to_string_lossy().to_lowercase();
        if !lname.ends_with(".dll") {
            continue;
        }
        for (known, family) in KNOWN_DLLS.iter() {
            if lname == *known {
                let path = entry.path();
                let (current_version, file_description, sha256) =
                    observe_known_dll(&path, &mut errors);
                records.push(DllRecord {
                    family: *family,
                    path,
                    current_version,
                    file_description,
                    sha256,
                });
                break;
            }
        }
    }
    InstallObservation {
        complete: errors.is_empty(),
        records,
        errors,
    }
}

pub fn scan_install(root: &Path) -> Result<Vec<DllRecord>, ScanError> {
    Ok(observe_install(root).records)
}

fn observe_known_dll(
    path: &Path,
    errors: &mut Vec<InstallObservationError>,
) -> (Option<String>, Option<String>, Option<String>) {
    let before = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            errors.push(observation_io_error(
                path.to_path_buf(),
                ObservationStage::Hash,
                error.kind(),
                error.to_string(),
            ));
            return (None, None, None);
        }
    };
    if before.len() > SHA_SKIP_THRESHOLD_BYTES {
        errors.push(InstallObservationError {
            path: path.to_path_buf(),
            cause: ObservationErrorCause::Hash,
            stage: ObservationStage::Hash,
            message: format!(
                "file is {} bytes; hashing is capped at {} bytes",
                before.len(),
                SHA_SKIP_THRESHOLD_BYTES
            ),
        });
        return observe_version_without_hash(path, errors);
    }

    let mut bytes = Vec::with_capacity(before.len() as usize);
    let read_result = std::fs::File::open(path).and_then(|mut file| file.read_to_end(&mut bytes));
    if let Err(error) = read_result {
        errors.push(observation_io_error(
            path.to_path_buf(),
            ObservationStage::Hash,
            error.kind(),
            error.to_string(),
        ));
        return (None, None, None);
    }

    let after = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            errors.push(observation_io_error(
                path.to_path_buf(),
                ObservationStage::Hash,
                error.kind(),
                error.to_string(),
            ));
            return (None, None, None);
        }
    };
    if file_changed_during_read(&before, &after, bytes.len()) {
        errors.push(InstallObservationError {
            path: path.to_path_buf(),
            cause: ObservationErrorCause::ChangedDuringRead,
            stage: ObservationStage::Hash,
            message: "file metadata changed while it was being read".into(),
        });
        return (None, None, None);
    }

    let sha256 = Some(hex_sha256(&bytes));
    match pe_version::parse_bytes(&bytes) {
        Ok(version) => (Some(version.file_version), version.file_description, sha256),
        Err(error) => {
            errors.push(InstallObservationError {
                path: path.to_path_buf(),
                cause: ObservationErrorCause::PeParse,
                stage: ObservationStage::PeParse,
                message: error.to_string(),
            });
            (None, None, sha256)
        }
    }
}

fn observe_version_without_hash(
    path: &Path,
    errors: &mut Vec<InstallObservationError>,
) -> (Option<String>, Option<String>, Option<String>) {
    match pe_version::read_dll_version(path) {
        Ok(version) => (Some(version.file_version), version.file_description, None),
        Err(pe_version::VersionError::Io(error)) => {
            errors.push(observation_io_error(
                path.to_path_buf(),
                ObservationStage::PeParse,
                error.kind(),
                error.to_string(),
            ));
            (None, None, None)
        }
        Err(error) => {
            errors.push(InstallObservationError {
                path: path.to_path_buf(),
                cause: ObservationErrorCause::PeParse,
                stage: ObservationStage::PeParse,
                message: error.to_string(),
            });
            (None, None, None)
        }
    }
}

fn file_changed_during_read(
    before: &std::fs::Metadata,
    after: &std::fs::Metadata,
    bytes_read: usize,
) -> bool {
    before.len() != after.len()
        || after.len() != bytes_read as u64
        || before.modified().ok() != after.modified().ok()
}

fn observation_io_error(
    path: PathBuf,
    stage: ObservationStage,
    kind: std::io::ErrorKind,
    message: String,
) -> InstallObservationError {
    InstallObservationError {
        path,
        cause: classify_observation_io(stage, kind),
        stage,
        message,
    }
}

fn classify_observation_io(
    stage: ObservationStage,
    kind: std::io::ErrorKind,
) -> ObservationErrorCause {
    match kind {
        std::io::ErrorKind::PermissionDenied => ObservationErrorCause::AccessDenied,
        std::io::ErrorKind::NotFound => ObservationErrorCause::Missing,
        _ if stage == ObservationStage::Walk => ObservationErrorCause::Walk,
        _ if stage == ObservationStage::PeParse => ObservationErrorCause::PeParse,
        _ => ObservationErrorCause::Hash,
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub fn hash_file_capped(path: &Path) -> std::io::Result<Option<String>> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > SHA_SKIP_THRESHOLD_BYTES {
        return Ok(None);
    }
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest.iter() {
        out.push_str(&format!("{:02x}", b));
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn phase4_partial_observation_keeps_records_and_distinguishes_all_causes() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("nvngx_dlss.dll"), b"not a PE").unwrap();

        let observation = observe_install(root.path());
        assert_eq!(observation.records.len(), 1);
        assert!(!observation.complete);
        assert!(observation
            .errors
            .iter()
            .any(|error| error.cause == ObservationErrorCause::PeParse));

        let causes = [
            classify_observation_io(ObservationStage::Walk, std::io::ErrorKind::Other),
            classify_observation_io(ObservationStage::Hash, std::io::ErrorKind::Other),
            ObservationErrorCause::PeParse,
            classify_observation_io(ObservationStage::Hash, std::io::ErrorKind::PermissionDenied),
            classify_observation_io(ObservationStage::Hash, std::io::ErrorKind::NotFound),
            ObservationErrorCause::ChangedDuringRead,
        ];
        assert_eq!(
            causes,
            [
                ObservationErrorCause::Walk,
                ObservationErrorCause::Hash,
                ObservationErrorCause::PeParse,
                ObservationErrorCause::AccessDenied,
                ObservationErrorCause::Missing,
                ObservationErrorCause::ChangedDuringRead,
            ]
        );

        let metadata = std::fs::metadata(root.path().join("nvngx_dlss.dll")).unwrap();
        assert!(!file_changed_during_read(
            &metadata,
            &metadata,
            metadata.len() as usize
        ));
        assert!(file_changed_during_read(
            &metadata,
            &metadata,
            metadata.len() as usize + 1
        ));
    }

    #[test]
    fn phase4_scan_install_compatibility_matches_observation_records() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("nvngx_dlss.dll"), b"not a PE").unwrap();
        std::fs::write(root.path().join("ordinary.txt"), b"ignored").unwrap();

        let legacy = scan_install(root.path()).unwrap();
        let observation = observe_install(root.path());
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].family, DllFamily::DlssSr);
        assert!(legacy[0].current_version.is_none());
        assert!(legacy[0].file_description.is_none());
        assert!(legacy[0].sha256.is_some());
        assert_eq!(legacy.len(), observation.records.len());
        assert_eq!(legacy[0].family, observation.records[0].family);
        assert_eq!(legacy[0].path, observation.records[0].path);
        assert_eq!(
            legacy[0].current_version,
            observation.records[0].current_version
        );
        assert_eq!(
            legacy[0].file_description,
            observation.records[0].file_description
        );
        assert_eq!(legacy[0].sha256, observation.records[0].sha256);

        let missing = root.path().join("missing-root");
        let legacy_missing = scan_install(&missing).unwrap();
        let observed_missing = observe_install(&missing);
        assert!(legacy_missing.is_empty());
        assert!(observed_missing.records.is_empty());
        assert!(!observed_missing.complete);
        assert!(observed_missing
            .errors
            .iter()
            .any(|error| error.cause == ObservationErrorCause::Missing));
    }

    #[test]
    fn hash_file_capped_returns_hex_for_normal_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("test.dll");
        std::fs::write(&p, b"hello world").unwrap();
        let h = hash_file_capped(&p).unwrap().unwrap();
        assert_eq!(
            h,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn hash_file_capped_returns_none_above_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("huge.dll");
        let mut f = std::fs::File::create(&p).unwrap();
        let chunk = vec![0u8; 1024 * 1024];
        let chunks_needed = (SHA_SKIP_THRESHOLD_BYTES / chunk.len() as u64) + 1;
        for _ in 0..chunks_needed {
            f.write_all(&chunk).unwrap();
        }
        f.sync_all().unwrap();
        drop(f);
        let h = hash_file_capped(&p).unwrap();
        assert!(h.is_none());
    }

    #[test]
    fn hash_file_capped_handles_zero_byte_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("empty.dll");
        std::fs::File::create(&p).unwrap();
        let h = hash_file_capped(&p).unwrap().unwrap();
        assert_eq!(
            h,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hash_file_capped_propagates_missing_file_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ghost.dll");
        let res = hash_file_capped(&p);
        assert!(res.is_err());
    }

    #[test]
    fn streamline_plugins_recognized_case_insensitively() {
        assert!(is_streamline_plugin("sl.dlss.dll"));
        assert!(is_streamline_plugin("sl.dlss_g.dll"));
        assert!(is_streamline_plugin("SL.Reflex.DLL"));
        assert!(is_streamline_plugin("sl.interposer.dll"));
        assert!(!is_streamline_plugin("nvngx_dlss.dll"));
        assert!(!is_streamline_plugin("nvngx_dlssg.dll"));
        assert!(!is_streamline_plugin("libxess.dll"));
        assert!(!is_streamline_plugin("slime.dll"));
    }

    #[test]
    fn dlss_enabler_markers_match_injector_dlls_not_generic_loaders() {
        assert!(is_dlss_enabler_marker("dlss-enabler.dll"));
        assert!(is_dlss_enabler_marker("DLSS-Enabler-Upscaler.dll"));
        assert!(is_dlss_enabler_marker("nvngx.dll"));
        assert!(is_dlss_enabler_marker("OptiScaler.dll"));
        assert!(is_dlss_enabler_marker("nvngx-wrapper.dll"));
        assert!(is_dlss_enabler_marker("NVNGX-WRAPPER.DLL"));
        assert!(is_dlss_enabler_marker("dlss-enabler.log"));
        assert!(is_dlss_enabler_marker("dlss-enabler.ASI"));
        assert!(!is_dlss_enabler_marker("nvngx_dlss.dll"));
        assert!(!is_dlss_enabler_marker("dxgi.dll"));
        assert!(!is_dlss_enabler_marker("sl.dlss.dll"));
    }

    #[test]
    fn detect_dlss_enabler_finds_nvngx_wrapper_alone() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("nvngx-wrapper.dll"), b"x").unwrap();
        assert!(detect_dlss_enabler(root.path()));
    }

    #[test]
    fn dlss_enabler_present_near_walks_up_to_the_game_root() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("dlss-enabler.dll"), b"x").unwrap();
        let nested = root
            .path()
            .join("Engine/Plugins/Runtime/Nvidia/Streamline/Binaries/ThirdParty/Win64");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(dlss_enabler_present_near(&nested, 8));
        assert!(!dlss_enabler_present_near(&nested, 2));
    }

    #[test]
    fn detect_dlss_enabler_finds_marker_in_nested_dir_and_keeps_scan_contract() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("Binaries/Win64");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("dlss-enabler.dll"), b"x").unwrap();
        std::fs::write(root.path().join("sl.dlss_g.dll"), b"x").unwrap();
        assert!(detect_dlss_enabler(root.path()));
        let records = scan_install(root.path()).unwrap();
        assert!(records
            .iter()
            .any(|r| r.path.file_name().unwrap() == "sl.dlss_g.dll"));
    }

    #[test]
    fn detect_dlss_enabler_is_false_without_markers() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("nvngx_dlss.dll"), b"x").unwrap();
        assert!(!detect_dlss_enabler(root.path()));
    }

    #[test]
    fn directstorage_core_and_fsr_denoiser_have_distinct_catalog_keys() {
        assert_eq!(
            DllFamily::DirectStorageCore.catalog_key(),
            "direct_storage_core"
        );
        assert_eq!(DllFamily::DirectStorageCore.vendor(), "microsoft");
        assert_eq!(DllFamily::FsrDenoiser.catalog_key(), "fsr_denoiser");
        assert_eq!(DllFamily::FsrDenoiser.vendor(), "amd");
    }

    #[test]
    fn scan_maps_dstoragecore_and_denoiser_to_their_own_families() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("dstoragecore.dll"), b"x").unwrap();
        std::fs::write(root.path().join("amd_fidelityfx_denoiser_dx12.dll"), b"x").unwrap();
        let recs = scan_install(root.path()).unwrap();
        assert!(recs
            .iter()
            .any(|r| r.family == DllFamily::DirectStorageCore));
        assert!(recs.iter().any(|r| r.family == DllFamily::FsrDenoiser));
    }

    #[test]
    fn sl_dlss_plugins_have_their_own_catalog_keys_distinct_from_nvngx() {
        assert_eq!(DllFamily::SlDlssSr.catalog_key(), "sl_dlss_sr");
        assert_eq!(DllFamily::SlDlssFg.catalog_key(), "sl_dlss_fg");
        assert_eq!(DllFamily::SlDlssRr.catalog_key(), "sl_dlss_rr");
        assert_eq!(DllFamily::SlDlssFg.vendor(), "nvidia");
        assert_ne!(
            DllFamily::SlDlssFg.catalog_key(),
            DllFamily::DlssFg.catalog_key()
        );
        assert_eq!(DllFamily::DlssFg.catalog_key(), "dlss_fg");
    }

    #[test]
    fn scan_splits_sl_dlss_plugins_from_their_nvngx_runtimes() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("sl.dlss_g.dll"), b"x").unwrap();
        std::fs::write(root.path().join("sl.dlss.dll"), b"x").unwrap();
        std::fs::write(root.path().join("sl.dlss_d.dll"), b"x").unwrap();
        std::fs::write(root.path().join("nvngx_dlssg.dll"), b"x").unwrap();
        let recs = scan_install(root.path()).unwrap();
        let fam = |name: &str| {
            recs.iter()
                .find(|r| r.path.file_name().unwrap() == name)
                .map(|r| r.family)
        };
        assert_eq!(fam("sl.dlss_g.dll"), Some(DllFamily::SlDlssFg));
        assert_eq!(fam("sl.dlss.dll"), Some(DllFamily::SlDlssSr));
        assert_eq!(fam("sl.dlss_d.dll"), Some(DllFamily::SlDlssRr));
        assert_eq!(fam("nvngx_dlssg.dll"), Some(DllFamily::DlssFg));
    }
}
