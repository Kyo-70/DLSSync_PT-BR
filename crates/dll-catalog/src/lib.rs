pub mod download;
pub mod hash;
pub mod v3;
pub mod zip;
pub use v3::SourceHealth;

pub fn ensure_available_space(path: &Path, required: u64) -> Result<(), CatalogError> {
    let directory = if path.is_dir() {
        path
    } else {
        path.parent()
            .ok_or_else(|| CatalogError::Unsafe("destination has no directory".into()))?
    };
    let available = fs2::available_space(directory)?;
    if available < required {
        return Err(CatalogError::Unsafe(format!(
            "insufficient disk space: {required} bytes required, {available} available"
        )));
    }
    Ok(())
}

pub use download::{
    fetch_shared, DownloadCache, DownloadOptions, DownloadProgress, DEFAULT_CACHE_TTL,
    DEFAULT_CHUNK_TIMEOUT, DEFAULT_MAX_RETRIES,
};
pub use hash::{hash_file_with, hex_md5, hex_md5_file, hex_sha256, hex_sha256_file, HashAlgo};
pub use zip::{
    extract_dll_from_bytes, looks_like_zip, MAX_UNCOMPRESSED_ENTRY_BYTES, MAX_ZIP_TOTAL_BYTES,
};

use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("parse: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip: {0}")]
    Zip(#[from] ::zip::result::ZipError),
    #[error("missing: {0}")]
    Missing(String),
    #[error("unsafe archive: {0}")]
    Unsafe(String),
    #[error("integrity: expected sha256 {expected}, got {actual}")]
    Integrity { expected: String, actual: String },
    #[error("truncated: received {got} bytes of {expected}")]
    Truncated { got: u64, expected: u64 },
    #[error("stalled: no bytes for {seconds} s")]
    Stalled { seconds: u64 },
    #[error("cancelled by user")]
    Cancelled,
    #[error(
        "catalog manifest has malformed sha256 ({reason}) for {filename} — refresh the manifest"
    )]
    BadCatalogSha { filename: String, reason: String },
    #[error("after {attempts} retries: {last}")]
    Retries { attempts: u32, last: String },
    #[error("cached error: {0}")]
    Cached(String),
    #[error("manifest signature missing — refusing untrusted manifest from {url}")]
    MissingSignature { url: String },
    #[error("manifest signature verification failed: {0}")]
    Signature(String),
    #[error("catalog is empty — refusing to replace the current catalog")]
    EmptyCatalog,
    #[error("catalog downgrade refused: current {current}, fetched {fetched}")]
    Downgrade { current: String, fetched: String },
    #[error("unsupported catalog schema: expected {expected}, got {actual}")]
    UnsupportedSchema { expected: u32, actual: u32 },
    #[error("catalog is stale: generated {generated}, maximum age is {max_age_days} days")]
    StaleCatalog {
        generated: String,
        max_age_days: i64,
    },
    #[error("catalog timestamp is too far in the future: generated {generated}")]
    FutureCatalog { generated: String },
    #[error("server returned 304 without a verified generation for {url}")]
    NotModifiedWithoutVerifiedGeneration { url: String },
}

pub struct VerifiedFetchRequest<'a> {
    pub client: &'a reqwest::Client,
    pub cache_path: &'a Path,
    pub url: &'a str,
    pub minimum_generated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedGeneration {
    pub catalog: Catalog,
    pub url: String,
    pub etag: Option<String>,
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub verified_at: chrono::DateTime<chrono::Utc>,
    pub content_sha256: String,
}

#[derive(Debug)]
pub enum VerifiedFetchOutcome {
    Modified(VerifiedGeneration),
    NotModified(VerifiedGeneration),
    Failed {
        error: CatalogError,
        retained: Option<VerifiedGeneration>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct VerifiedGenerationFile {
    schema_version: u32,
    url: String,
    etag: Option<String>,
    generated_at: chrono::DateTime<chrono::Utc>,
    verified_at: chrono::DateTime<chrono::Utc>,
    content_sha256: String,
    manifest: String,
    signature: String,
}

const VERIFIED_GENERATION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Catalog {
    pub schema_version: u32,
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub vendors: BTreeMap<String, BTreeMap<String, FamilyEntry>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, SourceHealth>,
    #[serde(default)]
    pub incompatible_games: Vec<String>,
    #[serde(default)]
    pub anticheat: Option<AntiCheatIndex>,
    /// Manifest-driven anti-cheat binary signatures, merged on top of the
    /// compile-time `ANTI_CHEAT_BINARIES` baseline at scan time so a newly named
    /// or renamed engine is detected from a manifest refresh without shipping an
    /// app release. Absent in older manifests (`#[serde(default)]` → empty) →
    /// the static baseline alone applies, no regression.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anti_cheat_binaries: Vec<AntiCheatBinary>,
}

/// One manifest-supplied anti-cheat binary signature: a lowercase filename
/// `needle` matched as a substring against files on disk, and the canonical
/// engine `name` reported on a hit. Mirrors a `(needle, name)` row of the
/// compile-time `ANTI_CHEAT_BINARIES` table so the two layers merge cleanly.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AntiCheatBinary {
    pub needle: String,
    pub name: String,
}

/// Slim per-game anti-cheat index distilled from PCGamingWiki at manifest-build
/// time (with an AreWeAntiCheatYet Linux/Wine status overlay applied
/// server-side), bundled into the manifest with zero new runtime outbound. Keys
/// in `by_name` are lowercased for case-insensitive matching.
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct AntiCheatIndex {
    #[serde(default)]
    pub by_appid: BTreeMap<u32, AntiCheatEntry>,
    #[serde(default)]
    pub by_name: BTreeMap<String, AntiCheatEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AntiCheatEntry {
    /// Kernel/usermode anti-cheat engines (Easy Anti-Cheat, BattlEye, Vanguard …)
    /// — account-ban risk on a DLL swap.
    pub anticheats: Vec<String>,
    /// Anti-tamper / heavy DRM (Denuvo Anti-Tamper, Arxan, VMProtect …) — these
    /// can reject a swapped DLL on signature mismatch and block launch.
    #[serde(default)]
    pub anti_tamper: Vec<String>,
    /// AreWeAntiCheatYet Linux/Wine compatibility status when known.
    #[serde(default)]
    pub status: Option<String>,
}

impl AntiCheatEntry {
    /// Union another entry's protection lists into this one (case-insensitive
    /// dedupe) and adopt its status when present. Used by the layered merge so
    /// no source erases another's findings.
    fn absorb(&mut self, other: &AntiCheatEntry) {
        push_unique(&mut self.anticheats, &other.anticheats);
        push_unique(&mut self.anti_tamper, &other.anti_tamper);
        if other.status.is_some() {
            self.status = other.status.clone();
        }
    }
}

fn push_unique(into: &mut Vec<String>, more: &[String]) {
    for m in more {
        if !into.iter().any(|x| x.eq_ignore_ascii_case(m)) {
            into.push(m.clone());
        }
    }
}

/// Canonical game-name key: lowercase, keep only ASCII alphanumerics. Used both
/// when building the index and when looking up, so "Assassin's Creed: Shadows"
/// and "assassins creed shadows" resolve to the same entry.
pub fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

impl AntiCheatIndex {
    /// The dataset distilled from PCGamingWiki (with an AreWeAntiCheatYet status
    /// overlay), embedded so the warning works offline and before the CDN
    /// manifest carries the index.
    pub fn embedded() -> Self {
        serde_json::from_str(include_str!("../anticheat-snapshot.json")).unwrap_or_default()
    }

    /// Fold `other` into `self`, unioning the protection lists per game (so a
    /// layer that only knows the anti-cheat does not erase another layer's
    /// anti-tamper finding) and taking `other`'s status when it has one. Layer
    /// order: embedded (base) → manifest → live-fetch (freshest last). Union,
    /// not replace, keeps detection maximal — a game flagged by any layer stays
    /// flagged.
    pub fn merge(&mut self, other: &AntiCheatIndex) {
        for (id, entry) in &other.by_appid {
            self.by_appid.entry(*id).or_default().absorb(entry);
        }
        for (name, entry) in &other.by_name {
            self.by_name.entry(name.clone()).or_default().absorb(entry);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.by_appid.is_empty() && self.by_name.is_empty()
    }

    pub fn lookup(&self, app_id: Option<&str>, name: &str) -> Option<&AntiCheatEntry> {
        if let Some(id) = app_id.and_then(|s| s.parse::<u32>().ok()) {
            if let Some(entry) = self.by_appid.get(&id) {
                return Some(entry);
            }
        }
        self.by_name.get(&normalize_name(name))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FamilyEntry {
    pub latest: String,
    pub releases: Vec<Release>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Release {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<dlssync_contracts::ArtifactDescriptor>,
    pub version: String,
    pub version_packed: u64,
    pub filename: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub signed: bool,
    pub released_at: chrono::DateTime<chrono::Utc>,
    pub source: String,
    pub cdn_url: String,
    #[serde(default)]
    pub release_notes: Option<String>,
    #[serde(default)]
    pub signature_subject: Option<String>,
    #[serde(default = "default_channel")]
    pub channel: String,
    #[serde(default)]
    pub is_dev: bool,
    #[serde(default)]
    pub min_driver: Option<String>,
    #[serde(default = "default_hash_algorithm")]
    pub hash_algorithm: String,
    /// Exact path of the wanted file inside a multi-copy SDK zip, e.g.
    /// `bin/x64/sl.dlss_g.dll`. Disambiguates the signed production binary from
    /// the unsigned `bin/x64/development/` copy that shares the same basename;
    /// `None` keeps the basename-match behaviour for single-copy archives.
    #[serde(default)]
    pub zip_entry: Option<String>,
}

fn default_channel() -> String {
    "stable".to_string()
}

fn default_hash_algorithm() -> String {
    "sha256".to_string()
}

/// Commit the embedded `fallback/manifest.json` (+ `.sig`) was snapshotted from.
/// Refreshed alongside the bundled fallback when a release re-bundles it.
pub const FALLBACK_MANIFEST_COMMIT_SHA: &str = "9c5b4a3b717e3637c0b8e708f93c66c38dd6d912";

/// Manifest URL used by the standard build. It tracks `@main` so the catalog can
/// refresh with new upstream DLL versions between application releases.
#[cfg(not(feature = "nexus"))]
pub const DEFAULT_MANIFEST_URL: &str =
    "https://cdn.jsdelivr.net/gh/xt0n1-t3ch/dlssync-manifest@main/manifest-v3.json";

/// Manifest URL used by the Nexus Mods build. It is pinned to the same immutable
/// commit as the bundled fallback manifest so outbound DLL download destinations
/// cannot change without a manual source change and a new application release.
#[cfg(feature = "nexus")]
pub const DEFAULT_MANIFEST_URL: &str =
    "https://cdn.jsdelivr.net/gh/xt0n1-t3ch/dlssync-manifest@9c5b4a3b717e3637c0b8e708f93c66c38dd6d912/manifest.json";

/// Canonical moving upstream used only for a user-requested catalog refresh.
/// The Nexus build never calls this automatically; its policy is enforced in
/// `dlssync-application` before any request is created.
pub const CANONICAL_MANIFEST_URL: &str =
    "https://cdn.jsdelivr.net/gh/xt0n1-t3ch/dlssync-manifest@main/manifest-v3.json";

pub const MANIFEST_ENV_VAR: &str = "DLSSYNC_MANIFEST_URL";
pub const SUPPORTED_SCHEMA_VERSION: u32 = 3;
pub const MAX_CATALOG_AGE_DAYS: i64 = 180;
pub const MAX_FUTURE_CLOCK_SKEW_MINUTES: i64 = 10;

const MANIFEST_RETRY_BACKOFF_MS: &[u64] = &[200, 800, 2000];

/// Suffix appended to the manifest URL to locate its detached Ed25519
/// signature. `manifest.json` → `manifest.json.sig`. The signature file holds
/// the 64-byte raw Ed25519 signature, hex-encoded (with optional surrounding
/// whitespace), computed over the exact bytes of `manifest.json`.
const MANIFEST_SIGNATURE_SUFFIX: &str = ".sig";

/// Production Ed25519 public verification key (32 bytes, hex) for the DLSSync
/// manifest. The matching private key is provisioned out-of-band and never lives
/// in the repo; the manifest pipeline signs `manifest.json` into
/// `manifest.json.sig` with it. Release builds fail closed on a bad/missing
/// signature (see `ENFORCE_MANIFEST_SIGNATURE`); debug builds verify-and-log only.
pub const MANIFEST_PUBKEY_HEX: &str =
    "e9dd0828f9ee5ecb72e0a811723a79c6e5373ca1c20bd5b255d68a2b3928fcd3";

/// Master enforcement flag for manifest signature verification. ARMED: release
/// builds fail closed on a missing or invalid `manifest.json.sig`. This is safe
/// because the manifest URL is pinned to an immutable commit SHA (jsdelivr serves
/// a byte-identical manifest + signature with no propagation skew) and a signed
/// fallback manifest is embedded for the offline / CDN-outage first run. Debug
/// builds still skip enforcement (see `signature_enforced`) so local dev against
/// an unsigned manifest is unaffected.
pub const ENFORCE_MANIFEST_SIGNATURE: bool = true;

/// Whether signature enforcement is active for this build. Enforcement is on in
/// release builds when `ENFORCE_MANIFEST_SIGNATURE` is set; debug builds skip it
/// so local dev / tests against an unsigned manifest still work. The
/// verification path itself always runs — only the fail-closed reaction is
/// gated here.
pub fn signature_enforced() -> bool {
    ENFORCE_MANIFEST_SIGNATURE && !cfg!(debug_assertions)
}

/// The configured manifest URL.
///
/// In release builds this is always the hardcoded HTTPS CDN — the
/// `DLSSYNC_MANIFEST_URL` env var is ignored so a pre-launch environment
/// injection cannot redirect every catalog/integrity fetch to an attacker.
/// In debug builds the env var is honored to support local manifest testing.
pub fn manifest_url() -> String {
    #[cfg(debug_assertions)]
    {
        if let Ok(v) = std::env::var(MANIFEST_ENV_VAR) {
            return v;
        }
    }
    DEFAULT_MANIFEST_URL.to_string()
}

/// Derive the detached-signature URL for a given manifest URL.
fn signature_url(manifest_url: &str) -> String {
    format!("{manifest_url}{MANIFEST_SIGNATURE_SUFFIX}")
}

/// Verify a detached Ed25519 signature (hex-encoded, 64 raw bytes) over the
/// exact `manifest_bytes`, against the baked-in public key. Returns the parsed
/// reason string on any failure so the caller can surface it.
fn verify_manifest_signature(manifest_bytes: &[u8], signature_hex: &str) -> Result<(), String> {
    verify_with_pubkey(MANIFEST_PUBKEY_HEX, manifest_bytes, signature_hex)
}

/// Core Ed25519 detached-signature check, parameterized on the hex public key so
/// the crypto path is testable against a known keypair without touching the
/// baked-in placeholder key.
fn verify_with_pubkey(
    pubkey_hex: &str,
    manifest_bytes: &[u8],
    signature_hex: &str,
) -> Result<(), String> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let key_bytes: [u8; 32] = hex::decode(pubkey_hex)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| "public key is not 32 hex-encoded bytes".to_string())?;
    let verifying_key =
        VerifyingKey::from_bytes(&key_bytes).map_err(|e| format!("public key is invalid: {e}"))?;

    let sig_bytes: [u8; 64] = hex::decode(signature_hex.trim())
        .map_err(|e| format!("signature is not valid hex: {e}"))?
        .try_into()
        .map_err(|_| "signature is not 64 bytes".to_string())?;
    let signature = Signature::from_bytes(&sig_bytes);

    verifying_key
        .verify(manifest_bytes, &signature)
        .map_err(|e| format!("signature does not match manifest: {e}"))
}

impl Catalog {
    /// Read an existing signed document without claiming it is current.
    /// The generator uses this to retain last-good historical data on failure.
    pub fn from_signed_bytes(bytes: &[u8], signature: &str) -> Result<Self, CatalogError> {
        verify_manifest_signature(bytes, signature).map_err(CatalogError::Signature)?;
        let catalog: Catalog = serde_json::from_slice(bytes)?;
        if !matches!(catalog.schema_version, 2 | 3) {
            return Err(CatalogError::UnsupportedSchema {
                expected: SUPPORTED_SCHEMA_VERSION,
                actual: catalog.schema_version,
            });
        }
        catalog.validate_artifacts()?;
        Ok(catalog)
    }

    pub async fn fetch(client: &reqwest::Client) -> Result<Self, CatalogError> {
        let url = manifest_url();
        Self::fetch_from(client, &url).await
    }

    pub async fn fetch_from(client: &reqwest::Client, url: &str) -> Result<Self, CatalogError> {
        let mut last_err = String::new();
        for (idx, backoff) in MANIFEST_RETRY_BACKOFF_MS.iter().enumerate() {
            match try_fetch(client, url).await {
                Ok(c) => return Ok(c),
                Err(e) => {
                    last_err = e.to_string();
                    tracing::warn!(attempt = idx + 1, error = %last_err, "catalog fetch attempt failed");
                    if idx + 1 < MANIFEST_RETRY_BACKOFF_MS.len() {
                        tokio::time::sleep(Duration::from_millis(*backoff)).await;
                    }
                }
            }
        }
        Err(CatalogError::Retries {
            attempts: MANIFEST_RETRY_BACKOFF_MS.len() as u32,
            last: last_err,
        })
    }

    pub async fn fetch_with_cache(
        client: &reqwest::Client,
        cache_path: &Path,
    ) -> Result<Self, CatalogError> {
        let url = manifest_url();
        match fetch_raw_verified(client, &url).await {
            Ok((catalog, raw, sig)) => {
                write_catalog_cache(cache_path, &raw, sig.as_deref());
                Ok(catalog)
            }
            Err(e) => match load_cached_catalog(cache_path) {
                Some(c) => {
                    tracing::warn!(error = %e, "manifest fetch failed; using re-verified on-disk cache");
                    Ok(c)
                }
                None => {
                    tracing::warn!(error = %e, "manifest fetch failed with no usable cache; using embedded fallback manifest");
                    embedded_fallback_catalog()
                }
            },
        }
    }

    /// Fetch an explicitly selected upstream, require its detached signature in
    /// every build profile, reject empty/older data, then atomically stage the
    /// exact signed bytes and signature. This is the manual Nexus refresh path.
    pub async fn fetch_verified_with_cache_from(
        client: &reqwest::Client,
        cache_path: &Path,
        url: &str,
        minimum_generated_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Self, CatalogError> {
        let mut last_error = String::new();
        for (index, backoff) in MANIFEST_RETRY_BACKOFF_MS.iter().enumerate() {
            match fetch_verified_generation(VerifiedFetchRequest {
                client,
                cache_path,
                url,
                minimum_generated_at,
            })
            .await
            {
                VerifiedFetchOutcome::Modified(generation)
                | VerifiedFetchOutcome::NotModified(generation) => return Ok(generation.catalog),
                VerifiedFetchOutcome::Failed { error, .. }
                    if matches!(
                        error,
                        CatalogError::Downgrade { .. }
                            | CatalogError::Io(_)
                            | CatalogError::NotModifiedWithoutVerifiedGeneration { .. }
                    ) =>
                {
                    return Err(error);
                }
                VerifiedFetchOutcome::Failed { error, .. } => {
                    last_error = error.to_string();
                    if index + 1 < MANIFEST_RETRY_BACKOFF_MS.len() {
                        tokio::time::sleep(Duration::from_millis(*backoff)).await;
                    }
                }
            }
        }
        Err(CatalogError::Retries {
            attempts: MANIFEST_RETRY_BACKOFF_MS.len() as u32,
            last: last_error,
        })
    }

    pub fn releases(&self, vendor: &str, family: &str) -> Vec<Release> {
        self.vendors
            .get(vendor)
            .and_then(|v| v.get(family))
            .map(|f| f.releases.clone())
            .unwrap_or_default()
    }

    pub fn latest(&self, vendor: &str, family: &str) -> Option<Release> {
        let v = self.vendors.get(vendor)?;
        let f = v.get(family)?;
        f.releases.iter().find(|r| r.version == f.latest).cloned()
    }

    pub fn find(&self, vendor: &str, family: &str, version: &str) -> Option<Release> {
        let v = self.vendors.get(vendor)?;
        let f = v.get(family)?;
        f.releases.iter().find(|r| r.version == version).cloned()
    }

    pub fn find_file(
        &self,
        vendor: &str,
        family: &str,
        version: &str,
        filename: &str,
    ) -> Option<Release> {
        let v = self.vendors.get(vendor)?;
        let f = v.get(family)?;
        f.releases
            .iter()
            .filter(|r| r.version == version && r.filename.eq_ignore_ascii_case(filename))
            .max_by_key(|r| (r.channel == "stable", !r.is_dev, r.released_at))
            .cloned()
    }

    pub fn find_latest_for_file(
        &self,
        vendor: &str,
        family: &str,
        filename: &str,
    ) -> Option<Release> {
        let v = self.vendors.get(vendor)?;
        let f = v.get(family)?;
        f.releases
            .iter()
            .filter(|r| r.filename.eq_ignore_ascii_case(filename))
            .filter(|r| r.channel == "stable" && !r.is_dev)
            .max_by_key(|r| (r.version_packed, r.released_at))
            .cloned()
    }
}

fn validate_catalog_candidate(
    catalog: &Catalog,
    minimum_generated_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<(), CatalogError> {
    validate_catalog_at(catalog, minimum_generated_at, chrono::Utc::now())
}

fn validate_catalog_at(
    catalog: &Catalog,
    minimum_generated_at: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), CatalogError> {
    if !matches!(catalog.schema_version, 2 | 3) {
        return Err(CatalogError::UnsupportedSchema {
            expected: SUPPORTED_SCHEMA_VERSION,
            actual: catalog.schema_version,
        });
    }
    if catalog.generated_at > now + chrono::Duration::minutes(MAX_FUTURE_CLOCK_SKEW_MINUTES) {
        return Err(CatalogError::FutureCatalog {
            generated: catalog.generated_at.to_rfc3339(),
        });
    }
    if catalog.generated_at < now - chrono::Duration::days(MAX_CATALOG_AGE_DAYS) {
        return Err(CatalogError::StaleCatalog {
            generated: catalog.generated_at.to_rfc3339(),
            max_age_days: MAX_CATALOG_AGE_DAYS,
        });
    }
    if catalog.vendors.is_empty() {
        return Err(CatalogError::EmptyCatalog);
    }
    if let Some(current) = minimum_generated_at {
        if catalog.generated_at < current {
            return Err(CatalogError::Downgrade {
                current: current.to_rfc3339(),
                fetched: catalog.generated_at.to_rfc3339(),
            });
        }
    }
    catalog.validate_artifacts()?;
    Ok(())
}

pub async fn fetch_verified_generation(request: VerifiedFetchRequest<'_>) -> VerifiedFetchOutcome {
    let retained = load_verified_generation(request.cache_path);
    let matching_validator = retained
        .as_ref()
        .filter(|generation| generation.url == request.url)
        .and_then(|generation| generation.etag.clone());

    let mut http_request = request.client.get(request.url);
    if let Some(etag) = matching_validator.as_deref() {
        http_request = http_request.header(reqwest::header::IF_NONE_MATCH, etag);
    }

    let response = match http_request.send().await {
        Ok(response) => response,
        Err(error) => {
            return VerifiedFetchOutcome::Failed {
                error: CatalogError::Http(error),
                retained,
            };
        }
    };
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return match retained.filter(|generation| {
            generation.url == request.url
                && generation.etag.as_deref() == matching_validator.as_deref()
        }) {
            Some(generation) if matching_validator.is_some() => {
                VerifiedFetchOutcome::NotModified(generation)
            }
            _ => VerifiedFetchOutcome::Failed {
                error: CatalogError::NotModifiedWithoutVerifiedGeneration {
                    url: request.url.to_string(),
                },
                retained: None,
            },
        };
    }

    let response = match response.error_for_status() {
        Ok(response) => response,
        Err(error) => {
            return VerifiedFetchOutcome::Failed {
                error: CatalogError::Http(error),
                retained,
            };
        }
    };
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let raw = match response.bytes().await {
        Ok(bytes) => bytes.to_vec(),
        Err(error) => {
            return VerifiedFetchOutcome::Failed {
                error: CatalogError::Http(error),
                retained,
            };
        }
    };
    let signature = match fetch_signature(request.client, request.url).await {
        Ok(Some(signature)) => signature,
        Ok(None) => {
            return VerifiedFetchOutcome::Failed {
                error: CatalogError::MissingSignature {
                    url: request.url.to_string(),
                },
                retained,
            };
        }
        Err(error) => {
            return VerifiedFetchOutcome::Failed {
                error: CatalogError::Http(error),
                retained,
            };
        }
    };
    if let Err(reason) = verify_manifest_signature(&raw, &signature) {
        return VerifiedFetchOutcome::Failed {
            error: CatalogError::Signature(reason),
            retained,
        };
    }
    let catalog = match serde_json::from_slice::<Catalog>(&raw) {
        Ok(catalog) => catalog,
        Err(error) => {
            return VerifiedFetchOutcome::Failed {
                error: CatalogError::Parse(error),
                retained,
            };
        }
    };
    if let Err(error) = validate_catalog_candidate(&catalog, request.minimum_generated_at) {
        return VerifiedFetchOutcome::Failed { error, retained };
    }
    let manifest = match String::from_utf8(raw.clone()) {
        Ok(manifest) => manifest,
        Err(error) => {
            return VerifiedFetchOutcome::Failed {
                error: CatalogError::Unsafe(format!("catalog manifest is not UTF-8: {error}")),
                retained,
            };
        }
    };
    let content_sha256 = hex::encode(sha2::Sha256::digest(&raw));
    let verified_at = chrono::Utc::now();
    let file = VerifiedGenerationFile {
        schema_version: VERIFIED_GENERATION_SCHEMA_VERSION,
        url: request.url.to_string(),
        etag: etag.clone(),
        generated_at: catalog.generated_at,
        verified_at,
        content_sha256: content_sha256.clone(),
        manifest,
        signature,
    };
    if let Err(error) = persist_verified_generation(request.cache_path, &file) {
        return VerifiedFetchOutcome::Failed { error, retained };
    }

    VerifiedFetchOutcome::Modified(VerifiedGeneration {
        generated_at: catalog.generated_at,
        catalog,
        url: request.url.to_string(),
        etag,
        verified_at,
        content_sha256,
    })
}

pub fn load_verified_generation(cache_path: &Path) -> Option<VerifiedGeneration> {
    let raw_file = std::fs::read(cache_path).ok()?;
    let file = serde_json::from_slice::<VerifiedGenerationFile>(&raw_file).ok()?;
    if file.schema_version != VERIFIED_GENERATION_SCHEMA_VERSION || file.url.is_empty() {
        return None;
    }
    let manifest = file.manifest.as_bytes();
    let actual_sha256 = hex::encode(sha2::Sha256::digest(manifest));
    if actual_sha256 != file.content_sha256 {
        return None;
    }
    verify_manifest_signature(manifest, &file.signature).ok()?;
    let catalog = serde_json::from_slice::<Catalog>(manifest).ok()?;
    validate_catalog_candidate(&catalog, None).ok()?;
    if catalog.generated_at != file.generated_at {
        return None;
    }
    Some(VerifiedGeneration {
        generated_at: catalog.generated_at,
        catalog,
        url: file.url,
        etag: file.etag,
        verified_at: file.verified_at,
        content_sha256: file.content_sha256,
    })
}

fn persist_verified_generation(
    cache_path: &Path,
    generation: &VerifiedGenerationFile,
) -> Result<(), CatalogError> {
    persist_verified_generation_with(cache_path, generation, || Ok(()))
}

fn persist_verified_generation_with<F>(
    cache_path: &Path,
    generation: &VerifiedGenerationFile,
    before_commit: F,
) -> Result<(), CatalogError>
where
    F: FnOnce() -> std::io::Result<()>,
{
    let parent = cache_path
        .parent()
        .ok_or_else(|| CatalogError::Missing("catalog cache has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let mut stage = tempfile::NamedTempFile::new_in(parent)?;
    {
        use std::io::Write as _;
        serde_json::to_writer(stage.as_file_mut(), generation)?;
        stage.as_file_mut().flush()?;
        stage.as_file().sync_all()?;
    }
    before_commit()?;
    stage
        .persist(cache_path)
        .map_err(|error| CatalogError::Io(error.error))?;
    Ok(())
}

async fn try_fetch(client: &reqwest::Client, url: &str) -> Result<Catalog, CatalogError> {
    let bytes = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    enforce_manifest_signature(client, url, &bytes).await?;
    let c: Catalog = serde_json::from_slice(&bytes)?;
    validate_catalog_candidate(&c, None)?;
    Ok(c)
}

/// Fetch the detached signature for `url` and verify it over `manifest_bytes`.
///
/// Fail-closed when `signature_enforced()`: a missing signature yields
/// `MissingSignature`, a present-but-invalid one yields `Signature`. When
/// enforcement is off (debug builds) the verification still runs if a signature
/// is reachable — a present-but-invalid signature is logged but tolerated, and
/// a missing signature is ignored — so local/dev work against the not-yet-signed
/// manifest is unaffected.
async fn enforce_manifest_signature(
    client: &reqwest::Client,
    url: &str,
    manifest_bytes: &[u8],
) -> Result<(), CatalogError> {
    let fetched = fetch_signature(client, url).await;
    enforce_signature_outcome(url, manifest_bytes, fetched)
}

/// Decide a manifest fetch given an already-fetched signature result. Split out
/// of `enforce_manifest_signature` so the raw-caching path can keep the signature
/// text (for the sidecar cache) without fetching it twice.
fn enforce_signature_outcome(
    url: &str,
    manifest_bytes: &[u8],
    fetched: Result<Option<String>, reqwest::Error>,
) -> Result<(), CatalogError> {
    let enforced = signature_enforced();
    let sig_text = match fetched {
        Ok(Some(text)) => text,
        Ok(None) => {
            if enforced {
                return Err(CatalogError::MissingSignature {
                    url: url.to_string(),
                });
            }
            tracing::warn!(
                %url,
                "manifest has no detached signature; verification skipped (enforcement off in debug)"
            );
            return Ok(());
        }
        Err(e) => {
            if enforced {
                return Err(CatalogError::Signature(format!(
                    "could not fetch detached signature: {e}"
                )));
            }
            tracing::warn!(%url, error = %e, "could not fetch manifest signature; verification skipped (enforcement off in debug)");
            return Ok(());
        }
    };

    match verify_manifest_signature(manifest_bytes, &sig_text) {
        Ok(()) => Ok(()),
        Err(reason) => {
            if enforced {
                Err(CatalogError::Signature(reason))
            } else {
                tracing::warn!(%url, %reason, "manifest signature did not verify; tolerated (enforcement off in debug)");
                Ok(())
            }
        }
    }
}

/// Fetch the manifest + detached signature, enforce the signature, parse, and
/// return the parsed catalog alongside the EXACT bytes the signature covers and
/// the signature text — so the caller can persist a re-verifiable cache instead
/// of a re-serialized copy (which the signature would no longer match). Retries
/// with backoff like `fetch_from`.
async fn fetch_raw_verified(
    client: &reqwest::Client,
    url: &str,
) -> Result<(Catalog, Vec<u8>, Option<String>), CatalogError> {
    let mut last_err = String::new();
    for (idx, backoff) in MANIFEST_RETRY_BACKOFF_MS.iter().enumerate() {
        match fetch_raw_once(client, url).await {
            Ok(triple) => return Ok(triple),
            Err(e) => {
                last_err = e.to_string();
                tracing::warn!(attempt = idx + 1, error = %last_err, "catalog raw fetch attempt failed");
                if idx + 1 < MANIFEST_RETRY_BACKOFF_MS.len() {
                    tokio::time::sleep(Duration::from_millis(*backoff)).await;
                }
            }
        }
    }
    Err(CatalogError::Retries {
        attempts: MANIFEST_RETRY_BACKOFF_MS.len() as u32,
        last: last_err,
    })
}

async fn fetch_raw_once(
    client: &reqwest::Client,
    url: &str,
) -> Result<(Catalog, Vec<u8>, Option<String>), CatalogError> {
    let bytes = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let fetched = fetch_signature(client, url).await;
    let sig_opt = match &fetched {
        Ok(Some(s)) => Some(s.clone()),
        _ => None,
    };
    enforce_signature_outcome(url, &bytes, fetched)?;
    let c: Catalog = serde_json::from_slice(&bytes)?;
    validate_catalog_candidate(&c, None)?;
    Ok((c, bytes.to_vec(), sig_opt))
}

/// The bundled, signed manifest used only when there is neither network NOR a
/// usable on-disk cache (first run offline / CDN outage). Always verified against
/// the baked-in pubkey, so a tampered bundled file can never poison the catalog.
const FALLBACK_MANIFEST: &[u8] = include_bytes!("../fallback/manifest.json");
const FALLBACK_SIGNATURE: &str = include_str!("../fallback/manifest.json.sig");

pub fn embedded_fallback_catalog() -> Result<Catalog, CatalogError> {
    verify_manifest_signature(FALLBACK_MANIFEST, FALLBACK_SIGNATURE)
        .map_err(CatalogError::Signature)?;
    let catalog: Catalog = serde_json::from_slice(FALLBACK_MANIFEST)?;
    // The immutable fallback may outlive the network freshness window; require schema,
    // signature, and non-empty data but do not brick offline startup over its age.
    if !matches!(catalog.schema_version, 2 | 3) {
        return Err(CatalogError::UnsupportedSchema {
            expected: SUPPORTED_SCHEMA_VERSION,
            actual: catalog.schema_version,
        });
    }
    if catalog.vendors.is_empty() {
        return Err(CatalogError::EmptyCatalog);
    }
    catalog.validate_artifacts()?;
    Ok(catalog)
}

/// Sidecar path holding the detached signature next to the cached manifest, so
/// the cache can be re-verified on load.
fn cache_sig_path(cache_path: &Path) -> PathBuf {
    let mut p = cache_path.as_os_str().to_owned();
    p.push(".sig");
    PathBuf::from(p)
}

/// Persist the RAW manifest bytes (atomic) plus the detached signature sidecar.
/// Best-effort: cache write failures are logged, never fatal to a live fetch.
fn write_catalog_cache(cache_path: &Path, raw: &[u8], sig: Option<&str>) {
    match sig {
        Some(signature) => {
            if let Err(error) = write_verified_cache_atomic(cache_path, raw, signature) {
                tracing::warn!(%error, "failed to persist verified catalog cache");
            }
        }
        None => {
            tracing::warn!("unsigned catalog cache is not persisted");
            let _ = std::fs::remove_file(cache_sig_path(cache_path));
        }
    }
}

fn write_verified_cache_atomic(
    cache_path: &Path,
    raw: &[u8],
    signature: &str,
) -> Result<(), CatalogError> {
    let parent = cache_path
        .parent()
        .ok_or_else(|| CatalogError::Missing("catalog cache has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let mut manifest_stage = tempfile::NamedTempFile::new_in(parent)?;
    let mut signature_stage = tempfile::NamedTempFile::new_in(parent)?;
    {
        use std::io::Write as _;
        manifest_stage.write_all(raw)?;
        manifest_stage.as_file().sync_all()?;
        signature_stage.write_all(signature.as_bytes())?;
        signature_stage.as_file().sync_all()?;
    }
    signature_stage
        .persist(cache_sig_path(cache_path))
        .map_err(|error| error.error)?;
    manifest_stage
        .persist(cache_path)
        .map_err(|error| error.error)?;
    Ok(())
}

pub fn manifest_public_key_fingerprint() -> String {
    let digest = sha2::Sha256::digest(
        hex::decode(MANIFEST_PUBKEY_HEX).expect("embedded manifest key is valid hex"),
    );
    hex::encode(digest)
}

/// Load + parse an on-disk cache after requiring its detached signature to verify.
pub fn load_verified_cache(cache_path: &Path) -> Option<Catalog> {
    load_verified_generation(cache_path)
        .map(|generation| generation.catalog)
        .or_else(|| load_cached_catalog_with(cache_path, true))
}

/// Load + parse the on-disk cache. When enforcement is on, the cached raw bytes
/// are re-verified against the sidecar `.sig`; a missing or invalid signature
/// rejects the cache (the caller then falls back to the embedded manifest). A
/// legacy re-serialized cache from before sidecar caching has no `.sig`, so it is
/// correctly rejected under enforcement and accepted only when enforcement is off.
fn load_cached_catalog(cache_path: &Path) -> Option<Catalog> {
    load_verified_generation(cache_path)
        .map(|generation| generation.catalog)
        .or_else(|| load_cached_catalog_with(cache_path, signature_enforced()))
}

fn load_cached_catalog_with(cache_path: &Path, require_signature: bool) -> Option<Catalog> {
    let raw = std::fs::read(cache_path).ok()?;
    if require_signature {
        let sig = std::fs::read_to_string(cache_sig_path(cache_path)).ok()?;
        verify_manifest_signature(&raw, &sig).ok()?;
    }
    let catalog: Catalog = serde_json::from_slice(&raw).ok()?;
    validate_catalog_candidate(&catalog, None).ok()?;
    Some(catalog)
}

/// A raw Ed25519 signature is 64 bytes = 128 hex chars.
const MANIFEST_SIGNATURE_HEX_LEN: usize = 128;

/// True when `text` (trimmed) is exactly a 128-char lowercase/uppercase hex
/// string — the shape of a real detached Ed25519 signature.
fn is_signature_hex(text: &str) -> bool {
    let t = text.trim();
    t.len() == MANIFEST_SIGNATURE_HEX_LEN && t.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Fetch the detached signature text. `Ok(None)` means the signature is ABSENT —
/// either HTTP 404/410, or a 200 whose body is not a 128-char hex string (a CDN
/// error page, redirect-to-login HTML, or JSON error). Treating a non-hex 200 as
/// absent (rather than as a corrupt signature) lets the caller fall through to
/// `MissingSignature` and the fallback path instead of erroring on CDN noise. Any
/// other transport/HTTP failure is a real error.
async fn fetch_signature(
    client: &reqwest::Client,
    manifest_url: &str,
) -> Result<Option<String>, reqwest::Error> {
    let resp = client.get(signature_url(manifest_url)).send().await?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
        return Ok(None);
    }
    let text = resp.error_for_status()?.text().await?;
    if is_signature_hex(&text) {
        Ok(Some(text))
    } else {
        Ok(None)
    }
}

pub async fn download_and_extract_dll(
    client: &reqwest::Client,
    release: &Release,
    dest_dir: &Path,
) -> Result<PathBuf, CatalogError> {
    let cache = DownloadCache::new();
    download_and_extract_dll_cached(
        &cache,
        client,
        release,
        dest_dir,
        DownloadOptions::default(),
    )
    .await
}

pub async fn download_and_extract_dll_cached(
    cache: &DownloadCache,
    client: &reqwest::Client,
    release: &Release,
    dest_dir: &Path,
    opts: DownloadOptions,
) -> Result<PathBuf, CatalogError> {
    let bytes = fetch_shared(cache, client, &release.cdn_url, opts).await?;
    extract_dll_from_bytes(&bytes, release, dest_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified_generation_fixture(url: &str, etag: Option<&str>) -> VerifiedGenerationFile {
        let catalog = Catalog::from_signed_bytes(FALLBACK_MANIFEST, FALLBACK_SIGNATURE).unwrap();
        VerifiedGenerationFile {
            schema_version: VERIFIED_GENERATION_SCHEMA_VERSION,
            url: url.to_string(),
            etag: etag.map(str::to_owned),
            generated_at: catalog.generated_at,
            verified_at: chrono::Utc::now(),
            content_sha256: hex::encode(sha2::Sha256::digest(FALLBACK_MANIFEST)),
            manifest: String::from_utf8(FALLBACK_MANIFEST.to_vec()).unwrap(),
            signature: FALLBACK_SIGNATURE.to_string(),
        }
    }

    #[test]
    fn space_guard_rejects_impossible_allocation_without_creating_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("candidate.dll");
        let error = ensure_available_space(&target, u64::MAX).unwrap_err();
        assert!(error.to_string().contains("insufficient disk space"));
        assert!(!target.exists());
        ensure_available_space(dir.path(), 0).unwrap();
    }

    #[test]
    fn embedded_fallback_verifies_against_production_pubkey_and_parses() {
        // This is the brick-guard: it runs the real Ed25519 verification of the
        // bundled manifest.json.sig against MANIFEST_PUBKEY_HEX over the bundled
        // manifest.json bytes. If the embedded pair is ever stale/corrupt, this
        // fails — so enforcement can never ship with an unverifiable fallback.
        let catalog = embedded_fallback_catalog().expect("embedded fallback must verify + parse");
        assert!(
            !catalog.vendors.is_empty(),
            "embedded fallback catalog has no vendors"
        );
    }

    #[test]
    fn embedded_signature_is_128_hex() {
        assert!(is_signature_hex(FALLBACK_SIGNATURE));
    }

    #[test]
    fn signature_hex_shape_guard() {
        assert!(is_signature_hex(&"a".repeat(128)));
        assert!(is_signature_hex(&format!("  {}\n", "0".repeat(128))));
        assert!(!is_signature_hex(&"a".repeat(127)));
        assert!(!is_signature_hex(&"g".repeat(128))); // non-hex
        assert!(!is_signature_hex("<html>error</html>"));
        assert!(!is_signature_hex(""));
    }

    #[test]
    fn manifest_url_targets_the_signed_manifest_repo() {
        assert!(DEFAULT_MANIFEST_URL.contains("dlssync-manifest"));
        assert!(DEFAULT_MANIFEST_URL.starts_with("https://"));
        assert_eq!(FALLBACK_MANIFEST_COMMIT_SHA.len(), 40);
        if cfg!(feature = "nexus") {
            assert!(DEFAULT_MANIFEST_URL.contains(FALLBACK_MANIFEST_COMMIT_SHA));
            assert!(!DEFAULT_MANIFEST_URL.contains("@main"));
        } else {
            assert!(DEFAULT_MANIFEST_URL.contains("@main"));
        }
    }

    fn release_with(filename: &str, version: &str, packed: u64, sha: &str) -> Release {
        Release {
            artifact: None,
            version: version.into(),
            version_packed: packed,
            filename: filename.into(),
            sha256: sha.into(),
            size_bytes: 100,
            signed: false,
            released_at: chrono::Utc::now(),
            source: "test".into(),
            cdn_url: "https://example.test/x.dll".into(),
            release_notes: None,
            signature_subject: None,
            channel: "stable".into(),
            is_dev: false,
            min_driver: None,
            hash_algorithm: "sha256".into(),
            zip_entry: None,
        }
    }

    fn catalog_with(family: &str, releases: Vec<Release>) -> Catalog {
        let mut families = BTreeMap::new();
        let latest = releases
            .iter()
            .max_by_key(|r| r.version_packed)
            .map(|r| r.version.clone())
            .unwrap_or_default();
        families.insert(family.to_string(), FamilyEntry { latest, releases });
        let mut vendors = BTreeMap::new();
        vendors.insert("intel".to_string(), families);
        Catalog {
            sources: Default::default(),
            schema_version: 2,
            generated_at: chrono::Utc::now(),
            vendors,
            incompatible_games: vec![],
            anticheat: None,
            anti_cheat_binaries: vec![],
        }
    }

    #[test]
    fn manifest_url_falls_back_to_default_cdn() {
        let url = manifest_url();
        assert!(url.starts_with("https://"), "manifest URL must be https");
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn manifest_url_ignores_env_override_in_release() {
        std::env::set_var(MANIFEST_ENV_VAR, "http://attacker.example/evil.json");
        let url = manifest_url();
        std::env::remove_var(MANIFEST_ENV_VAR);
        assert_eq!(url, DEFAULT_MANIFEST_URL);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn manifest_url_honors_env_override_in_debug() {
        std::env::set_var(MANIFEST_ENV_VAR, "https://local.test/manifest.json");
        let url = manifest_url();
        std::env::remove_var(MANIFEST_ENV_VAR);
        assert_eq!(url, "https://local.test/manifest.json");
    }

    #[test]
    fn signature_url_appends_sig_suffix() {
        assert_eq!(
            signature_url("https://cdn.test/manifest.json"),
            "https://cdn.test/manifest.json.sig"
        );
    }

    #[test]
    fn baked_in_pubkey_rejects_bogus_signature() {
        let result = verify_manifest_signature(b"any-manifest-bytes", &"00".repeat(64));
        assert!(
            result.is_err(),
            "a bogus all-zero signature must be rejected by the baked-in production key"
        );
    }

    fn test_signing_key() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&[7u8; 32])
    }

    #[test]
    fn verified_cache_rejects_sidecar_from_wrong_key() {
        use ed25519_dalek::Signer;
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("manifest.json");
        let catalog = catalog_with(
            "dlss",
            vec![release_with("nvngx_dlss.dll", "1.0.0", 1, "a")],
        );
        let raw = serde_json::to_vec(&catalog).unwrap();
        let sig_hex = hex::encode(test_signing_key().sign(&raw).to_bytes());
        std::fs::write(&cache_path, raw).unwrap();
        std::fs::write(cache_sig_path(&cache_path), sig_hex).unwrap();
        assert!(load_verified_cache(&cache_path).is_none());
    }

    #[test]
    fn valid_signature_over_exact_bytes_verifies() {
        use ed25519_dalek::Signer;
        let sk = test_signing_key();
        let pubkey_hex = hex::encode(sk.verifying_key().to_bytes());
        let manifest = br#"{"schema_version":2}"#;
        let sig_hex = hex::encode(sk.sign(manifest).to_bytes());
        assert!(verify_with_pubkey(&pubkey_hex, manifest, &sig_hex).is_ok());
        assert!(verify_with_pubkey(&pubkey_hex, manifest, &format!("  {sig_hex}\n")).is_ok());
    }

    #[test]
    fn signature_over_tampered_bytes_is_rejected() {
        use ed25519_dalek::Signer;
        let sk = test_signing_key();
        let pubkey_hex = hex::encode(sk.verifying_key().to_bytes());
        let sig_hex = hex::encode(sk.sign(br#"{"schema_version":2}"#).to_bytes());
        assert!(verify_with_pubkey(&pubkey_hex, br#"{"schema_version":3}"#, &sig_hex).is_err());
    }

    #[test]
    fn malformed_signature_inputs_are_rejected() {
        let sk = test_signing_key();
        let pubkey_hex = hex::encode(sk.verifying_key().to_bytes());
        let manifest = b"data";
        assert!(verify_with_pubkey(&pubkey_hex, manifest, "not-hex!!").is_err());
        assert!(verify_with_pubkey(&pubkey_hex, manifest, "abcd").is_err());
        assert!(verify_with_pubkey("zz", manifest, &"00".repeat(64)).is_err());
        assert!(verify_with_pubkey(&"aa".repeat(31), manifest, &"00".repeat(64)).is_err());
    }

    #[test]
    fn signature_enforcement_tracks_flag_and_build() {
        assert_eq!(
            signature_enforced(),
            ENFORCE_MANIFEST_SIGNATURE && !cfg!(debug_assertions)
        );
    }

    #[test]
    fn anticheat_index_prefers_appid_then_falls_back_to_name() {
        let mut index = AntiCheatIndex::default();
        index.by_appid.insert(
            440,
            AntiCheatEntry {
                anticheats: vec!["VAC".into()],
                status: Some("Supported".into()),
                ..Default::default()
            },
        );
        index.by_name.insert(
            normalize_name("Team Fortress 2"),
            AntiCheatEntry {
                anticheats: vec!["VAC".into()],
                ..Default::default()
            },
        );
        assert_eq!(
            index
                .lookup(Some("440"), "anything")
                .unwrap()
                .status
                .as_deref(),
            Some("Supported")
        );
        assert_eq!(
            index.lookup(None, "Team Fortress 2!").unwrap().anticheats,
            vec!["VAC".to_string()]
        );
        assert!(index.lookup(Some("999999"), "unknown game").is_none());
    }

    #[test]
    fn normalize_name_strips_punctuation_and_case() {
        assert_eq!(
            normalize_name("Assassin's Creed: Shadows"),
            "assassinscreedshadows"
        );
        assert_eq!(normalize_name("Team Fortress 2"), "teamfortress2");
        assert_eq!(normalize_name("  ELDEN RING  "), "eldenring");
    }

    #[test]
    fn catalog_without_anti_cheat_binaries_defaults_to_empty() {
        let json = r#"{
            "schema_version": 2,
            "generated_at": "2026-01-01T00:00:00Z",
            "vendors": {}
        }"#;
        let catalog: Catalog = serde_json::from_str(json).unwrap();
        assert!(
            catalog.anti_cheat_binaries.is_empty(),
            "absent field must default to empty so the static baseline alone applies"
        );
    }

    #[test]
    fn catalog_parses_manifest_anti_cheat_binaries() {
        let json = r#"{
            "schema_version": 2,
            "generated_at": "2026-01-01T00:00:00Z",
            "vendors": {},
            "anti_cheat_binaries": [
                { "needle": "newguard", "name": "New Guard AC" }
            ]
        }"#;
        let catalog: Catalog = serde_json::from_str(json).unwrap();
        assert_eq!(
            catalog.anti_cheat_binaries,
            vec![AntiCheatBinary {
                needle: "newguard".into(),
                name: "New Guard AC".into(),
            }]
        );
    }

    #[test]
    fn catalog_omits_empty_anti_cheat_binaries_when_serialized() {
        let catalog = catalog_with(
            "xess_sr",
            vec![release_with(
                "libxess.dll",
                "1.0.0",
                1,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )],
        );
        let value = serde_json::to_value(catalog).unwrap();
        assert!(
            value.get("anti_cheat_binaries").is_none(),
            "empty anti_cheat_binaries must not be emitted into the public manifest"
        );
    }

    #[test]
    fn embedded_snapshot_loads_and_resolves_known_titles() {
        let index = AntiCheatIndex::embedded();
        assert!(!index.is_empty());
        assert!(index
            .lookup(Some("1245620"), "whatever")
            .is_some_and(|e| e.anticheats.iter().any(|a| a.contains("Easy Anti-Cheat"))));
        assert!(index.lookup(None, "Elden Ring").is_some());
    }

    #[test]
    fn embedded_snapshot_carries_anti_tamper_for_denuvo_titles() {
        let index = AntiCheatIndex::embedded();
        let ac_shadows = index.lookup(None, "Assassin's Creed Shadows");
        assert!(
            ac_shadows.is_some_and(|e| e.anti_tamper.iter().any(|a| a.contains("Denuvo"))),
            "AC Shadows should resolve with Denuvo anti-tamper from the embedded snapshot"
        );
    }

    #[test]
    fn merge_unions_lists_and_adopts_status() {
        let mut base = AntiCheatIndex::default();
        base.by_appid.insert(
            1,
            AntiCheatEntry {
                anticheats: vec!["Easy Anti-Cheat".into()],
                anti_tamper: vec!["Arxan Anti-Tamper".into()],
                status: None,
            },
        );
        let mut top = AntiCheatIndex::default();
        top.by_appid.insert(
            1,
            AntiCheatEntry {
                anticheats: vec!["easy anti-cheat".into()],
                status: Some("Supported".into()),
                ..Default::default()
            },
        );
        top.by_appid.insert(
            2,
            AntiCheatEntry {
                anticheats: vec!["Added".into()],
                ..Default::default()
            },
        );
        base.merge(&top);
        let one = base.by_appid.get(&1).unwrap();
        assert_eq!(one.anticheats, vec!["Easy Anti-Cheat".to_string()]);
        assert_eq!(one.anti_tamper, vec!["Arxan Anti-Tamper".to_string()]);
        assert_eq!(one.status.as_deref(), Some("Supported"));
        assert_eq!(
            base.by_appid.get(&2).unwrap().anticheats,
            vec!["Added".to_string()]
        );
    }

    #[test]
    fn find_latest_for_file_picks_highest_packed_version() {
        let c = catalog_with(
            "xess_sr",
            vec![
                release_with("libxess.dll", "2.0.0", 200, "shaA"),
                release_with("libxess.dll", "3.0.1", 301, "shaC"),
                release_with("libxess.dll", "2.5.0", 250, "shaB"),
            ],
        );
        let r = c
            .find_latest_for_file("intel", "xess_sr", "libxess.dll")
            .unwrap();
        assert_eq!(r.version, "3.0.1");
        assert_eq!(r.sha256, "shaC");
    }

    #[test]
    fn find_latest_for_file_is_case_insensitive() {
        let c = catalog_with(
            "xess_sr",
            vec![release_with("libxess.dll", "3.0.1", 301, "shaX")],
        );
        let r = c
            .find_latest_for_file("intel", "xess_sr", "LIBXESS.DLL")
            .unwrap();
        assert_eq!(r.sha256, "shaX");
    }

    #[test]
    fn automatic_candidate_excludes_preview_and_development() {
        let stable = release_with("libxess.dll", "2.0.0", 200, "stable");
        let mut preview = release_with("libxess.dll", "3.0.0", 300, "preview");
        preview.channel = "experimental".into();
        let mut developer = release_with("libxess.dll", "4.0.0", 400, "development");
        developer.is_dev = true;
        let catalog = catalog_with("xess_sr", vec![stable, preview, developer]);
        assert_eq!(
            catalog
                .find_latest_for_file("intel", "xess_sr", "libxess.dll")
                .unwrap()
                .version,
            "2.0.0"
        );
    }

    #[test]
    fn valid_signature_does_not_approve_an_arm_catalog_candidate() {
        use ed25519_dalek::{Signer, SigningKey};
        let mut candidate = release_with("libxess.dll", "2.0.0", 200, &"aa".repeat(32));
        candidate.cdn_url = "https://example.com/sdk-aarch64.zip".into();
        candidate.zip_entry = Some("bin/aarch64/libxess.dll".into());
        let catalog = catalog_with("xess_sr", vec![candidate]);
        let bytes = serde_json::to_vec(&catalog).unwrap();
        let key = SigningKey::from_bytes(&[9; 32]);
        let signature = hex::encode(key.sign(&bytes).to_bytes());
        assert!(verify_with_pubkey(
            &hex::encode(key.verifying_key().to_bytes()),
            &bytes,
            &signature
        )
        .is_ok());
        assert!(catalog.validate_artifacts().is_err());
    }

    #[test]
    fn find_latest_for_file_filters_by_filename() {
        let c = catalog_with(
            "xess_sr",
            vec![
                release_with("libxess.dll", "3.0.1", 301, "main"),
                release_with("libxess_dx11.dll", "3.0.1", 301, "dx11"),
            ],
        );
        let r = c
            .find_latest_for_file("intel", "xess_sr", "libxess_dx11.dll")
            .unwrap();
        assert_eq!(r.sha256, "dx11");
    }

    #[test]
    fn find_file_disambiguates_same_version_by_filename() {
        let c = catalog_with(
            "xess_sr",
            vec![
                release_with("libxess.dll", "3.0.1", 301, "main"),
                release_with("libxess_dx11.dll", "3.0.1", 301, "dx11"),
            ],
        );
        assert_eq!(
            c.find_file("intel", "xess_sr", "3.0.1", "libxess_dx11.dll")
                .unwrap()
                .sha256,
            "dx11"
        );
        assert_eq!(
            c.find_file("intel", "xess_sr", "3.0.1", "LIBXESS.DLL")
                .unwrap()
                .sha256,
            "main"
        );
        assert_eq!(c.find("intel", "xess_sr", "3.0.1").unwrap().sha256, "main");
    }

    #[test]
    fn find_file_returns_none_for_unknown_filename() {
        let c = catalog_with(
            "xess_sr",
            vec![release_with("libxess.dll", "3.0.1", 301, "main")],
        );
        assert!(c
            .find_file("intel", "xess_sr", "3.0.1", "nvngx_dlss.dll")
            .is_none());
    }

    #[test]
    fn find_latest_for_file_returns_none_on_unknown_vendor() {
        let c = catalog_with(
            "xess_sr",
            vec![release_with("libxess.dll", "3.0.1", 301, "x")],
        );
        assert!(c
            .find_latest_for_file("nvidia", "xess_sr", "libxess.dll")
            .is_none());
    }

    #[test]
    fn find_latest_for_file_returns_none_on_missing_filename() {
        let c = catalog_with(
            "xess_sr",
            vec![release_with("libxess.dll", "3.0.1", 301, "x")],
        );
        assert!(c
            .find_latest_for_file("intel", "xess_sr", "nvngx_dlss.dll")
            .is_none());
    }

    #[tokio::test]
    async fn strict_manual_fetch_requires_signature_and_persists_a_verified_pair() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(FALLBACK_MANIFEST))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/manifest.json.sig"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FALLBACK_SIGNATURE))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("catalog.json");
        let catalog = Catalog::fetch_verified_with_cache_from(
            &reqwest::Client::new(),
            &cache,
            &format!("{}/manifest.json", server.uri()),
            None,
        )
        .await
        .unwrap();
        assert!(!catalog.vendors.is_empty());
        assert!(load_verified_cache(&cache).is_some());
    }

    #[tokio::test]
    async fn phase4_etag_is_sent_only_for_the_exact_verified_source_url() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        for manifest_path in ["/first.json", "/second.json"] {
            Mock::given(method("GET"))
                .and(path(manifest_path))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("ETag", "\"generation-1\"")
                        .set_body_bytes(FALLBACK_MANIFEST),
                )
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("{manifest_path}.sig")))
                .respond_with(ResponseTemplate::new(200).set_body_string(FALLBACK_SIGNATURE))
                .mount(&server)
                .await;
        }

        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("catalog.json");
        let client = reqwest::Client::new();
        let first_url = format!("{}/first.json", server.uri());
        assert!(matches!(
            fetch_verified_generation(VerifiedFetchRequest {
                client: &client,
                cache_path: &cache,
                url: &first_url,
                minimum_generated_at: None,
            })
            .await,
            VerifiedFetchOutcome::Modified(_)
        ));

        let second_url = format!("{}/second.json", server.uri());
        assert!(matches!(
            fetch_verified_generation(VerifiedFetchRequest {
                client: &client,
                cache_path: &cache,
                url: &second_url,
                minimum_generated_at: None,
            })
            .await,
            VerifiedFetchOutcome::Modified(_)
        ));

        let requests = server.received_requests().await.unwrap();
        let first = requests
            .iter()
            .find(|request| request.url.path() == "/first.json")
            .unwrap();
        let second = requests
            .iter()
            .find(|request| request.url.path() == "/second.json")
            .unwrap();
        assert!(first.headers.get("if-none-match").is_none());
        assert!(second.headers.get("if-none-match").is_none());
    }

    #[tokio::test]
    async fn phase4_unverified_generation_never_sends_etag() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(FALLBACK_MANIFEST))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/manifest.json.sig"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FALLBACK_SIGNATURE))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("catalog.json");
        let url = format!("{}/manifest.json", server.uri());
        let mut generation = verified_generation_fixture(&url, Some("untrusted-etag"));
        generation.content_sha256 = "00".repeat(32);
        persist_verified_generation(&cache, &generation).unwrap();

        assert!(matches!(
            fetch_verified_generation(VerifiedFetchRequest {
                client: &reqwest::Client::new(),
                cache_path: &cache,
                url: &url,
                minimum_generated_at: None,
            })
            .await,
            VerifiedFetchOutcome::Modified(_)
        ));
        let requests = server.received_requests().await.unwrap();
        let request = requests
            .iter()
            .find(|request| request.url.path() == "/manifest.json")
            .unwrap();
        assert!(request.headers.get("if-none-match").is_none());
    }

    #[tokio::test]
    async fn phase4_not_modified_and_failed_preserve_verified_generation() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .and(header("if-none-match", "\"generation-1\""))
            .respond_with(ResponseTemplate::new(304))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("ETag", "\"generation-1\"")
                    .set_body_bytes(FALLBACK_MANIFEST),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/manifest.json.sig"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FALLBACK_SIGNATURE))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("catalog.json");
        let client = reqwest::Client::new();
        let url = format!("{}/manifest.json", server.uri());
        let first = fetch_verified_generation(VerifiedFetchRequest {
            client: &client,
            cache_path: &cache,
            url: &url,
            minimum_generated_at: None,
        })
        .await;
        let modified = match first {
            VerifiedFetchOutcome::Modified(generation) => generation,
            other => panic!("expected modified, got {other:?}"),
        };
        let persisted = std::fs::read(&cache).unwrap();

        let second = fetch_verified_generation(VerifiedFetchRequest {
            client: &client,
            cache_path: &cache,
            url: &url,
            minimum_generated_at: None,
        })
        .await;
        let retained = match second {
            VerifiedFetchOutcome::NotModified(generation) => generation,
            other => panic!("expected not modified, got {other:?}"),
        };
        assert_eq!(retained.content_sha256, modified.content_sha256);
        assert_eq!(std::fs::read(&cache).unwrap(), persisted);

        let failed_url = format!("{}/missing.json", server.uri());
        assert!(matches!(
            fetch_verified_generation(VerifiedFetchRequest {
                client: &client,
                cache_path: &cache,
                url: &failed_url,
                minimum_generated_at: None,
            })
            .await,
            VerifiedFetchOutcome::Failed {
                retained: Some(_),
                ..
            }
        ));
        assert_eq!(std::fs::read(&cache).unwrap(), persisted);
    }

    #[test]
    fn phase4_interrupted_generation_write_leaves_no_mixed_state() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("catalog.json");
        let old = verified_generation_fixture("https://example.test/old.json", Some("old"));
        persist_verified_generation(&cache, &old).unwrap();
        let old_bytes = std::fs::read(&cache).unwrap();

        let new = verified_generation_fixture("https://example.test/new.json", Some("new"));
        let error = persist_verified_generation_with(&cache, &new, || {
            Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "simulated interruption before commit",
            ))
        })
        .unwrap_err();
        assert!(matches!(error, CatalogError::Io(_)));
        assert_eq!(std::fs::read(&cache).unwrap(), old_bytes);
        let loaded = load_verified_generation(&cache).unwrap();
        assert_eq!(loaded.url, "https://example.test/old.json");
        assert_eq!(loaded.etag.as_deref(), Some("old"));
    }

    #[test]
    fn candidate_validation_rejects_empty_and_older_catalogs() {
        let empty = Catalog {
            sources: Default::default(),
            schema_version: 2,
            generated_at: chrono::Utc::now(),
            vendors: BTreeMap::new(),
            incompatible_games: Vec::new(),
            anticheat: None,
            anti_cheat_binaries: Vec::new(),
        };
        assert!(matches!(
            validate_catalog_candidate(&empty, None),
            Err(CatalogError::EmptyCatalog)
        ));

        let now = chrono::Utc::now();
        let mut invalid =
            catalog_with("dlss_sr", vec![release_with("nvngx_dlss.dll", "1", 1, "a")]);
        invalid.schema_version = 4;
        assert!(matches!(
            validate_catalog_at(&invalid, None, now),
            Err(CatalogError::UnsupportedSchema { actual: 4, .. })
        ));
        invalid.schema_version = SUPPORTED_SCHEMA_VERSION;
        invalid.generated_at = now - chrono::Duration::days(MAX_CATALOG_AGE_DAYS + 1);
        assert!(matches!(
            validate_catalog_at(&invalid, None, now),
            Err(CatalogError::StaleCatalog { .. })
        ));
        invalid.generated_at = now + chrono::Duration::minutes(MAX_FUTURE_CLOCK_SKEW_MINUTES + 1);
        assert!(matches!(
            validate_catalog_at(&invalid, None, now),
            Err(CatalogError::FutureCatalog { .. })
        ));

        let catalog = embedded_fallback_catalog().unwrap();
        assert!(matches!(
            validate_catalog_candidate(
                &catalog,
                Some(catalog.generated_at + chrono::Duration::seconds(1))
            ),
            Err(CatalogError::Downgrade { .. })
        ));
    }
}
