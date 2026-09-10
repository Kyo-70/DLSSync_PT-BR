//! Expanded catalog metadata. The v2 projection remains a separate document.
use crate::{Catalog, CatalogError};
use dlssync_contracts::{Architecture, SignatureStatus};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct SourceHealth {
    pub last_attempt: String,
    pub last_success: Option<String>,
    pub error: Option<String>,
    /// Vendor/family keys whose data this source supplied.
    pub families: Vec<String>,
}

impl Catalog {
    /// Strip every extension before writing the endpoint used by 1.6.9.
    pub fn legacy_projection(&self) -> Self {
        let mut legacy = self.clone();
        legacy.schema_version = 2;
        legacy.sources.clear();
        for family in legacy.vendors.values_mut().flat_map(|v| v.values_mut()) {
            let mut releases =
                std::collections::BTreeMap::<(String, String), crate::Release>::new();
            for mut release in std::mem::take(&mut family.releases) {
                let key = (release.filename.clone(), release.version.clone());
                release.artifact = None;
                let rank = |r: &crate::Release| (r.channel == "stable", !r.is_dev, r.released_at);
                if releases
                    .get(&key)
                    .is_none_or(|old| rank(&release) > rank(old))
                {
                    releases.insert(key, release);
                }
            }
            family.releases = releases.into_values().collect();
            family.latest = family
                .releases
                .iter()
                .filter(|r| r.channel == "stable" && !r.is_dev)
                .max_by_key(|r| (r.version_packed, r.released_at))
                .map(|r| r.version.clone())
                .unwrap_or_default();
        }
        legacy
    }

    pub fn validate_artifacts(&self) -> Result<(), CatalogError> {
        let mut ids = BTreeSet::new();
        for (vendor, families) in &self.vendors {
            for (family, entry) in families {
                for release in &entry.releases {
                    let invalid = |reason: &str| {
                        CatalogError::Unsafe(format!(
                            "{vendor}/{family}/{}: {reason}",
                            release.filename
                        ))
                    };
                    if !safe_filename(&release.filename) {
                        return Err(invalid("invalid DLL filename"));
                    }
                    let expected_len = match release.hash_algorithm.as_str() {
                        "sha256" => 64,
                        "md5" => 32,
                        _ => return Err(invalid("unknown hash algorithm")),
                    };
                    if release.sha256.len() != expected_len
                        || !release.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    {
                        return Err(invalid("hash does not match the declared algorithm"));
                    }
                    if let Some(path) = &release.zip_entry {
                        if !safe_archive_entry(path, &release.filename) {
                            return Err(invalid("archive entry does not identify this x64 DLL"));
                        }
                    }
                    if has_foreign_architecture(&release.cdn_url) {
                        return Err(invalid("archive URL identifies another architecture"));
                    }
                    if let Some(artifact) = &release.artifact {
                        if artifact.family != *family
                            || !artifact.filename.eq_ignore_ascii_case(&release.filename)
                            || artifact.architecture != Architecture::X64
                            || artifact.source_url != release.cdn_url
                            || artifact.archive_entry != release.zip_entry
                            || artifact.hash.digest != release.sha256
                            || !artifact.hash.is_valid()
                            || artifact.size_bytes.0 != release.size_bytes.to_string()
                            || artifact.file_version.as_deref() != Some(&release.version)
                        {
                            return Err(invalid("artifact identity disagrees with the release"));
                        }
                        if artifact.signature_status == SignatureStatus::Verified
                            && artifact
                                .observed_publisher
                                .as_deref()
                                .is_none_or(str::is_empty)
                        {
                            return Err(invalid("verified signature has no observed publisher"));
                        }
                        if !ids.insert(artifact.id.clone()) {
                            return Err(invalid("duplicate artifact identity"));
                        }
                    }
                }
            }
        }
        for release in self
            .vendors
            .values()
            .flat_map(|v| v.values())
            .flat_map(|f| &f.releases)
        {
            if let Some(artifact) = &release.artifact {
                if artifact
                    .dependencies
                    .iter()
                    .any(|id| !ids.contains(id) || *id == artifact.id)
                {
                    return Err(CatalogError::Unsafe(format!(
                        "unresolved dependency for {}",
                        artifact.id
                    )));
                }
            }
        }
        Ok(())
    }
}

pub fn safe_filename(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".dll")
        && name.len() > 4
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.+-".contains(&c))
}

pub fn has_foreign_architecture(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    ["aarch64", "arm64", "win32"]
        .iter()
        .any(|s| value.contains(s))
        || value.split(['/', '\\', '-', '_', '.']).any(|s| s == "x86")
}

pub fn safe_archive_entry(path: &str, filename: &str) -> bool {
    let path = path.replace('\\', "/");
    !path.starts_with('/')
        && !has_foreign_architecture(&path)
        && path
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != ".." && !s.contains(':'))
        && path
            .rsplit('/')
            .next()
            .is_some_and(|s| s.eq_ignore_ascii_case(filename))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_reject_traversal_foreign_architecture_and_other_dlls() {
        for name in [
            "../game.dll",
            "C:game.dll",
            "x/y.dll",
            "game.dll:stream",
            "game.exe",
        ] {
            assert!(!safe_filename(name), "{name}");
        }
        for path in [
            "../sl.common.dll",
            "bin/aarch64/sl.common.dll",
            "bin/arm64ec/sl.common.dll",
            "bin/x86/sl.common.dll",
            "bin/x64/sl.reflex.dll",
        ] {
            assert!(!safe_archive_entry(path, "sl.common.dll"), "{path}");
        }
        assert!(safe_archive_entry("bin/x64/sl.common.dll", "sl.common.dll"));
    }
    #[test]
    fn projection_does_not_change_the_legacy_wire_format() {
        let catalog = crate::embedded_fallback_catalog().unwrap();
        let value = serde_json::to_value(catalog.legacy_projection()).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert!(value.get("sources").is_none());
        for family in value["vendors"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|v| v.as_object().unwrap().values())
        {
            for release in family["releases"].as_array().unwrap() {
                assert!(release.get("artifact").is_none());
            }
        }
    }
}
