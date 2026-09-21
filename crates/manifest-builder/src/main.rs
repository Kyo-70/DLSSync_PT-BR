mod download_cache;
use download_cache::download_bytes;
// Builds the signed v2 and v3 catalogs from upstream sources.

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use dll_catalog::{normalize_name, AntiCheatEntry, AntiCheatIndex, Catalog, FamilyEntry, Release};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;

const DLSS_SWAPPER_MANIFEST: &str =
    "https://raw.githubusercontent.com/beeradmoore/dlss-swapper/main/docs/manifest.json";

#[derive(Parser, Debug)]
#[command(name = "manifest-builder", version, about)]
struct Cli {
    /// Output path for the generated manifest.json
    #[arg(long, default_value = "manifest/manifest.json")]
    out: PathBuf,
    /// Expanded catalog, published separately from the v2 endpoint.
    #[arg(long, default_value = "manifest/manifest-v3.json")]
    out_v3: PathBuf,
    /// Skip network fetches and only print what would be done.
    #[arg(long)]
    dry_run: bool,
    /// Comma-separated source list (all by default).
    #[arg(
        long,
        value_delimiter = ',',
        default_value = "dlss_swapper,streamline,xess,fsr,reflex,directstorage,anticheat"
    )]
    sources: Vec<String>,
    /// Emit only the distilled anti-cheat snapshot (for the binary's embedded dataset) to this path.
    #[arg(long)]
    emit_anticheat_snapshot: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct SwapperManifest {
    #[serde(default)]
    dlss: Vec<SwapperEntry>,
    #[serde(default)]
    dlss_d: Vec<SwapperEntry>,
    #[serde(default)]
    dlss_g: Vec<SwapperEntry>,
    #[serde(default)]
    fsr_31_dx12: Vec<SwapperEntry>,
    #[serde(default)]
    fsr_31_vk: Vec<SwapperEntry>,
    #[serde(default)]
    xess: Vec<SwapperEntry>,
    #[serde(default)]
    xess_dx11: Vec<SwapperEntry>,
    #[serde(default)]
    xess_fg: Vec<SwapperEntry>,
    #[serde(default)]
    xell: Vec<SwapperEntry>,
}

#[derive(Debug, Deserialize)]
struct SwapperEntry {
    version: String,
    #[serde(default)]
    internal_name: Option<String>,
    md5_hash: String,
    download_url: String,
    #[serde(default)]
    file_description: Option<String>,
    #[serde(default)]
    signed_datetime: Option<String>,
    #[serde(default)]
    is_signature_valid: Option<bool>,
    #[serde(default)]
    file_size: Option<u64>,
    #[serde(default)]
    dll_source: Option<String>,
    #[serde(default)]
    additional_label: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    published_at: Option<DateTime<Utc>>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
    #[serde(default)]
    prerelease: bool,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

struct FilenameRule {
    /// canonical lowercase DLL filename present in the zip
    filename: &'static str,
    /// (vendor, family) destination
    vendor: &'static str,
    family: &'static str,
}

const STREAMLINE_RULES: &[FilenameRule] = &[
    FilenameRule {
        filename: "sl.dlss.dll",
        vendor: "nvidia",
        family: "sl_dlss_sr",
    },
    FilenameRule {
        filename: "sl.dlss_g.dll",
        vendor: "nvidia",
        family: "sl_dlss_fg",
    },
    FilenameRule {
        filename: "sl.dlss_d.dll",
        vendor: "nvidia",
        family: "sl_dlss_rr",
    },
    FilenameRule {
        filename: "sl.interposer.dll",
        vendor: "nvidia",
        family: "streamline",
    },
    FilenameRule {
        filename: "sl.common.dll",
        vendor: "nvidia",
        family: "streamline_common",
    },
    FilenameRule {
        filename: "sl.pcl.dll",
        vendor: "nvidia",
        family: "streamline_pcl",
    },
    FilenameRule {
        filename: "sl.nis.dll",
        vendor: "nvidia",
        family: "streamline_nis",
    },
    FilenameRule {
        filename: "sl.directsr.dll",
        vendor: "nvidia",
        family: "streamline_direct_sr",
    },
    FilenameRule {
        filename: "sl.reflex.dll",
        vendor: "nvidia",
        family: "reflex",
    },
];

const XESS_RULES: &[FilenameRule] = &[
    FilenameRule {
        filename: "libxess.dll",
        vendor: "intel",
        family: "xess_sr",
    },
    FilenameRule {
        filename: "libxess_dx11.dll",
        vendor: "intel",
        family: "xess_sr_dx11",
    },
    FilenameRule {
        filename: "libxess_fg.dll",
        vendor: "intel",
        family: "xess_fg",
    },
    FilenameRule {
        filename: "libxell.dll",
        vendor: "intel",
        family: "xell",
    },
];

const FSR_RULES: &[FilenameRule] = &[
    FilenameRule {
        filename: "amd_fidelityfx_dx12.dll",
        vendor: "amd",
        family: "fsr_upscaler",
    },
    FilenameRule {
        filename: "amd_fidelityfx_vk.dll",
        vendor: "amd",
        family: "fsr_upscaler_vk",
    },
    FilenameRule {
        filename: "amd_fidelityfx_upscaler_dx12.dll",
        vendor: "amd",
        family: "fsr_upscaler",
    },
    FilenameRule {
        filename: "amd_fidelityfx_upscaler_vk.dll",
        vendor: "amd",
        family: "fsr_upscaler_vk",
    },
    FilenameRule {
        filename: "amd_fidelityfx_framegeneration_dx12.dll",
        vendor: "amd",
        family: "fsr_fg",
    },
    FilenameRule {
        filename: "amd_fidelityfx_loader_dx12.dll",
        vendor: "amd",
        family: "fsr_loader",
    },
    FilenameRule {
        filename: "amd_fidelityfx_denoiser_dx12.dll",
        vendor: "amd",
        family: "fsr_denoiser",
    },
];

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?),
        )
        .init();
    let cli = Cli::parse();

    let client = build_client()?;
    if let Some(path) = cli.emit_anticheat_snapshot.as_ref() {
        let index = ingest_anticheat(&client).await?;
        write_atomic(path, &serde_json::to_vec(&index)?)?;
        return Ok(());
    }
    if cli.out == cli.out_v3 {
        return Err(anyhow!("v2 and v3 outputs must be separate files"));
    }
    if cli.dry_run {
        println!(
            "sources: {:?}; v2: {}; v3: {}",
            cli.sources,
            cli.out.display(),
            cli.out_v3.display()
        );
        return Ok(());
    }
    let signing_key = std::env::var(SIGNING_KEY_ENV)
        .context("signing key is required; refusing unsigned output")?;
    // Validate the key before fetching any packages. Never print its value.
    sign_bytes(signing_key.trim(), b"key validation")?;
    let mut catalog = load_previous(&cli.out_v3, &cli.out)?;
    let mut successes = 0;
    let mut requested = std::collections::BTreeSet::new();
    for name in &cli.sources {
        let source = if name == "reflex" {
            "streamline"
        } else {
            name.as_str()
        };
        if !requested.insert(source) {
            continue;
        }
        let mut proposed = catalog.vendors.clone();
        let result = match source {
            "dlss_swapper" => ingest_dlss_swapper(&client, &mut proposed).await,
            "streamline" => {
                ingest_github_zip_releases(
                    &client,
                    &mut proposed,
                    "NVIDIA-RTX/Streamline",
                    STREAMLINE_RULES,
                    |asset| is_streamline_x64_asset(&asset.name),
                )
                .await
            }
            "xess" => {
                ingest_github_zip_releases(
                    &client,
                    &mut proposed,
                    "intel/xess",
                    XESS_RULES,
                    |asset| {
                        asset.name.ends_with(".zip")
                            && asset.name.to_lowercase().contains("xess")
                            && !dll_catalog::v3::has_foreign_architecture(&asset.name)
                    },
                )
                .await
            }
            "fsr" => ingest_fidelityfx(&client, &mut proposed).await,
            "directstorage" => ingest_directstorage_nuget(&client, &mut proposed).await,
            "anticheat" => match ingest_protection_sources(&client, &mut catalog).await {
                Ok(index) if !index.is_empty() => {
                    catalog.anticheat = Some(index);
                    Ok(())
                }
                Ok(_) => Err(anyhow!("anti-cheat source returned an empty index")),
                Err(error) => Err(error),
            },
            _ => return Err(anyhow!("unknown source: {source}")),
        };
        if finish_source(&mut catalog, proposed, source, Utc::now(), result) {
            successes += 1;
        }
    }
    if successes == 0 || catalog.vendors.is_empty() {
        return Err(anyhow!(
            "no source completed; existing catalog files were preserved"
        ));
    }
    catalog.schema_version = 3;
    catalog.generated_at = Utc::now();
    deduplicate_artifacts(&mut catalog);
    link_dependencies(&mut catalog);
    catalog.validate_artifacts()?;
    let v3 = serde_json::to_vec_pretty(&catalog)?;
    let v2 = serde_json::to_vec_pretty(&catalog.legacy_projection())?;
    // Produce both validated documents before replacing either output.
    let sig3 = sign_bytes(signing_key.trim(), &v3)?;
    let sig2 = sign_bytes(signing_key.trim(), &v2)?;
    for (path, bytes, signature) in [(&cli.out, &v2, sig2), (&cli.out_v3, &v3, sig3)] {
        write_atomic(path, bytes)?;
        let mut sig = path.as_os_str().to_owned();
        sig.push(".sig");
        write_atomic(std::path::Path::new(&sig), signature.as_bytes())?;
    }
    tracing::info!(v2 = %cli.out.display(), v3 = %cli.out_v3.display(), sources_ok = successes, "wrote validated signed catalogs");
    Ok(())
}

fn finish_source(
    catalog: &mut Catalog,
    proposed: BTreeMap<String, BTreeMap<String, FamilyEntry>>,
    source: &str,
    now: DateTime<Utc>,
    result: Result<()>,
) -> bool {
    let previous_success = catalog
        .sources
        .get(source)
        .and_then(|s| s.last_success.clone());
    let (success, last_success, error) = match result {
        Ok(()) => {
            catalog.vendors = proposed;
            (true, Some(now.to_rfc3339()), None)
        }
        Err(error) => {
            tracing::error!(source, error = %error, "source failed; retaining previous data and observation date");
            (false, previous_success, Some(format!("{error:#}")))
        }
    };
    catalog.sources.insert(
        source.into(),
        dll_catalog::SourceHealth {
            last_attempt: now.to_rfc3339(),
            last_success,
            error,
            families: source_families(source),
        },
    );
    success
}

fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn load_previous(v3: &std::path::Path, v2: &std::path::Path) -> Result<Catalog> {
    for path in [v3, v2] {
        if !path.exists() {
            continue;
        }
        let mut sig = path.as_os_str().to_owned();
        sig.push(".sig");
        let bytes = std::fs::read(path)?;
        let signature = std::fs::read_to_string(std::path::Path::new(&sig))?;
        return Catalog::from_signed_bytes(&bytes, &signature)
            .context("previous catalog must have a valid signature and identities");
    }
    Ok(Catalog {
        schema_version: 3,
        generated_at: Utc::now(),
        vendors: BTreeMap::new(),
        sources: BTreeMap::new(),
        incompatible_games: vec![],
        anticheat: None,
        anti_cheat_binaries: vec![],
    })
}

fn source_families(source: &str) -> Vec<String> {
    let rules = match source {
        "streamline" => STREAMLINE_RULES,
        "xess" => XESS_RULES,
        "fsr" => FSR_RULES,
        "directstorage" => DS_RULES,
        "dlss_swapper" => {
            return [
                "nvidia/dlss_sr",
                "nvidia/dlss_rr",
                "nvidia/dlss_fg",
                "amd/fsr_upscaler",
                "amd/fsr_upscaler_vk",
                "intel/xess_sr",
                "intel/xess_sr_dx11",
                "intel/xess_fg",
                "intel/xell",
            ]
            .into_iter()
            .map(str::to_string)
            .collect()
        }
        _ => return vec![],
    };
    rules
        .iter()
        .map(|r| format!("{}/{}", r.vendor, r.family))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn deduplicate_artifacts(catalog: &mut Catalog) {
    for family in catalog.vendors.values_mut().flat_map(|v| v.values_mut()) {
        let mut observed = std::collections::BTreeSet::new();
        family.releases.retain(|release| {
            release
                .artifact
                .as_ref()
                .is_none_or(|artifact| observed.insert(artifact.id.clone()))
        });
    }
}

fn link_dependencies(catalog: &mut Catalog) {
    let artifacts: Vec<_> = catalog
        .vendors
        .values()
        .flat_map(|v| v.values())
        .flat_map(|f| &f.releases)
        .filter_map(|r| r.artifact.clone())
        .collect();
    for release in catalog
        .vendors
        .values_mut()
        .flat_map(|v| v.values_mut())
        .flat_map(|f| &mut f.releases)
    {
        let Some(artifact) = &mut release.artifact else {
            continue;
        };
        // Required runtime dependencies only. Other installed members from the
        // same archive are handled as a coherent update set by the planner.
        let required: &[&str] = if artifact.filename.starts_with("sl.") {
            &["sl.common.dll", "sl.interposer.dll"]
        } else if artifact.filename.starts_with("dstorage") {
            &["dstorage.dll", "dstoragecore.dll"]
        } else if artifact.family == "xess_fg" {
            &["libxell.dll"]
        } else {
            &[]
        };
        artifact.dependencies = artifacts
            .iter()
            .filter(|other| {
                other.id != artifact.id
                    && other.source_url == artifact.source_url
                    && required.contains(&other.filename.as_str())
            })
            .map(|other| other.id.clone())
            .collect();
    }
}

/// Env var holding the 32-byte Ed25519 signing seed (hex) used to sign the
/// generated manifest. Kept out of the repo; provisioned at manifest-build time.
const SIGNING_KEY_ENV: &str = "DLSSYNC_MANIFEST_SIGNING_KEY";

/// Produce the hex-encoded 64-byte detached Ed25519 signature over `body`.
fn sign_bytes(key_hex: &str, body: &[u8]) -> Result<String> {
    use ed25519_dalek::{Signer, SigningKey};
    let seed = hex::decode(key_hex).context("DLSSYNC_MANIFEST_SIGNING_KEY is not valid hex")?;
    let seed: [u8; 32] = seed
        .as_slice()
        .try_into()
        .context("DLSSYNC_MANIFEST_SIGNING_KEY must be a 32-byte (64 hex char) Ed25519 seed")?;
    Ok(hex::encode(
        SigningKey::from_bytes(&seed).sign(body).to_bytes(),
    ))
}

#[cfg(test)]
mod signing_tests {
    use super::sign_bytes;
    use ed25519_dalek::{SigningKey, Verifier};

    #[test]
    fn sign_bytes_roundtrips_against_its_public_key() {
        let seed = [9u8; 32];
        let key_hex = hex::encode(seed);
        let body = br#"{"schema_version":2}"#;
        let sig_hex = sign_bytes(&key_hex, body).unwrap();
        let sig_bytes: [u8; 64] = hex::decode(&sig_hex)
            .unwrap()
            .as_slice()
            .try_into()
            .unwrap();
        let vk = SigningKey::from_bytes(&seed).verifying_key();
        assert!(vk
            .verify(body, &ed25519_dalek::Signature::from_bytes(&sig_bytes))
            .is_ok());
        assert!(vk
            .verify(
                br#"{"schema_version":3}"#,
                &ed25519_dalek::Signature::from_bytes(&sig_bytes)
            )
            .is_err());
    }

    #[test]
    fn sign_bytes_rejects_malformed_seed() {
        assert!(sign_bytes("not-hex", b"x").is_err());
        assert!(sign_bytes(&"aa".repeat(31), b"x").is_err());
    }
}

const ANTICHEAT_DATASET: &str =
    "https://raw.githubusercontent.com/AreWeAntiCheatYet/AreWeAntiCheatYet/master/games.json";
const PCGW_API: &str = "https://www.pcgamingwiki.com/w/api.php";
const CARGO_PAGE: usize = 500;

/// Tokens that are not real protection names — filtered out of PCGamingWiki list
/// fields (Middleware.Anticheat, Availability.Uses_DRM).
const NOISE_TOKENS: &[&str] = &["none", "false", "true", "unknown", "n/a", "yes", "no"];

/// Anti-tamper / heavy-DRM markers worth flagging from the broad Uses_DRM list
/// (which also carries store launchers we ignore here).
const ANTI_TAMPER_MARKERS: &[&str] = &[
    "denuvo",
    "arxan",
    "vmprotect",
    "themida",
    "securom",
    "safedisc",
    "starforce",
];

/// AreWeAntiCheatYet game record — used only for the Linux/Wine `status` overlay
/// (matched by normalized name); PCGamingWiki supplies the protection lists.
#[derive(Debug, Deserialize)]
struct AwacGame {
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    anticheats: Vec<String>,
    #[serde(default, rename = "storeIds")]
    store_ids: serde_json::Value,
}

/// PCGamingWiki Steam_AppID columns hold a comma list (base game + DLC). The
/// base game's id is the first entry.
fn first_appid(raw: &str) -> Option<u32> {
    raw.split(',').next().and_then(|t| t.trim().parse().ok())
}

/// Split a PCGW list field, trim, dedupe (case-insensitive), drop noise tokens.
fn clean_tokens(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tok in raw.split(',') {
        let t = tok.trim();
        if t.is_empty() || NOISE_TOKENS.contains(&t.to_ascii_lowercase().as_str()) {
            continue;
        }
        if !out.iter().any(|x| x.eq_ignore_ascii_case(t)) {
            out.push(t.to_string());
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct CargoResp {
    #[serde(default)]
    cargoquery: Option<Vec<CargoRow>>,
    #[serde(default)]
    error: Option<CargoApiError>,
}

#[derive(Debug, Deserialize)]
struct CargoApiError {
    code: String,
    info: String,
}

impl CargoResp {
    fn into_rows(self) -> Result<Vec<CargoRow>> {
        if let Some(error) = self.error {
            return Err(anyhow!("PCGamingWiki {}: {}", error.code, error.info));
        }
        self.cargoquery
            .context("PCGamingWiki response is missing cargoquery")
    }
}

#[derive(Debug, Deserialize)]
struct CargoRow {
    title: serde_json::Value,
}

/// Run a Cargo query across all result pages, returning the flattened `title`
/// objects. `params` excludes `limit`/`offset` (added per page).
async fn cargo_query_all(
    client: &reqwest::Client,
    params: &[(&str, &str)],
) -> Result<Vec<serde_json::Value>> {
    let mut rows = Vec::new();
    let mut offset = 0usize;
    loop {
        let offset_str = offset.to_string();
        let limit_str = CARGO_PAGE.to_string();
        let mut q: Vec<(&str, &str)> = vec![
            ("action", "cargoquery"),
            ("format", "json"),
            ("limit", &limit_str),
            ("offset", &offset_str),
        ];
        q.extend_from_slice(params);
        let resp: CargoResp = client
            .get(PCGW_API)
            .query(&q)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let page = resp.into_rows()?;
        let n = page.len();
        rows.extend(page.into_iter().map(|r| r.title));
        if n < CARGO_PAGE {
            break;
        }
        offset += CARGO_PAGE;
        if offset >= 500_000 {
            return Err(anyhow!("PCGamingWiki pagination exceeds the row limit"));
        }
    }
    Ok(rows)
}

/// One game's distilled protections, keyed by normalized name during the merge.
#[derive(Default)]
struct GameProtections {
    appid: Option<u32>,
    page: String,
    anticheats: Vec<String>,
    anti_tamper: Vec<String>,
    status: Option<String>,
}

fn merge_list(into: &mut Vec<String>, more: Vec<String>) {
    for m in more {
        if !into.iter().any(|x| x.eq_ignore_ascii_case(&m)) {
            into.push(m);
        }
    }
}

/// Build the anti-cheat / anti-tamper index from PCGamingWiki (broad coverage of
/// all games, keyed by Steam appid + normalized name), with an AreWeAntiCheatYet
/// overlay for Linux/Wine status. PCGW Middleware.Anticheat → `anticheats`;
/// Availability.Uses_DRM Denuvo/Arxan/etc → `anti_tamper`.
async fn ingest_anticheat(client: &reqwest::Client) -> Result<AntiCheatIndex> {
    use std::collections::BTreeMap;
    let mut games: BTreeMap<String, GameProtections> = BTreeMap::new();

    let upsert =
        |games: &mut BTreeMap<String, GameProtections>, page: &str, appid: Option<u32>| -> bool {
            let key = normalize_name(page);
            if key.is_empty() {
                return false;
            }
            let e = games.entry(key).or_default();
            if e.page.is_empty() {
                e.page = page.to_string();
            }
            if e.appid.is_none() {
                e.appid = appid;
            }
            true
        };

    let ac_rows = cargo_query_all(
        client,
        &[
            ("tables", "Middleware,Infobox_game"),
            (
                "fields",
                "Infobox_game._pageName=Page,Middleware.Anticheat=AC,Infobox_game.Steam_AppID=AppID",
            ),
            ("join_on", "Middleware._pageID=Infobox_game._pageID"),
            ("where", "Middleware.Anticheat HOLDS LIKE \"%\""),
        ],
    )
    .await
    .context("PCGW anticheat query")?;
    for t in &ac_rows {
        let page = t.get("Page").and_then(|v| v.as_str()).unwrap_or("");
        let ac = clean_tokens(t.get("AC").and_then(|v| v.as_str()).unwrap_or(""));
        let appid = t
            .get("AppID")
            .and_then(|v| v.as_str())
            .and_then(first_appid);
        if ac.is_empty() || !upsert(&mut games, page, appid) {
            continue;
        }
        let key = normalize_name(page);
        merge_list(&mut games.get_mut(&key).unwrap().anticheats, ac);
    }

    let drm_rows = cargo_query_all(
        client,
        &[
            ("tables", "Availability,Infobox_game"),
            (
                "fields",
                "Infobox_game._pageName=Page,Availability.Uses_DRM=DRM,Infobox_game.Steam_AppID=AppID",
            ),
            ("join_on", "Availability._pageID=Infobox_game._pageID"),
            (
                "where",
                "Availability.Uses_DRM HOLDS LIKE \"%Denuvo%\" OR Availability.Uses_DRM HOLDS LIKE \"%Arxan%\"",
            ),
        ],
    )
    .await
    .context("PCGW anti-tamper query")?;
    for t in &drm_rows {
        let page = t.get("Page").and_then(|v| v.as_str()).unwrap_or("");
        let appid = t
            .get("AppID")
            .and_then(|v| v.as_str())
            .and_then(first_appid);
        let tamper: Vec<String> = clean_tokens(t.get("DRM").and_then(|v| v.as_str()).unwrap_or(""))
            .into_iter()
            .filter(|d| {
                let low = d.to_ascii_lowercase();
                ANTI_TAMPER_MARKERS.iter().any(|m| low.contains(m))
            })
            .collect();
        if tamper.is_empty() || !upsert(&mut games, page, appid) {
            continue;
        }
        let key = normalize_name(page);
        merge_list(&mut games.get_mut(&key).unwrap().anti_tamper, tamper);
    }

    let mut index = AntiCheatIndex::default();
    for (key, g) in games {
        if g.anticheats.is_empty() && g.anti_tamper.is_empty() {
            continue;
        }
        let entry = AntiCheatEntry {
            anticheats: g.anticheats,
            anti_tamper: g.anti_tamper,
            status: g.status,
        };
        index.by_name.insert(key, entry.clone());
        if let Some(appid) = g.appid {
            index.by_appid.insert(appid, entry);
        }
    }
    tracing::info!(
        anticheat_rows = ac_rows.len(),
        tamper_rows = drm_rows.len(),
        by_appid = index.by_appid.len(),
        by_name = index.by_name.len(),
        "distilled PCGamingWiki protection index"
    );
    Ok(index)
}

fn build_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!(
            "dlssync-manifest-builder/",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(120))
        .build()?)
}

async fn ingest_protection_sources(
    client: &reqwest::Client,
    catalog: &mut Catalog,
) -> Result<AntiCheatIndex> {
    let mut index = catalog
        .anticheat
        .clone()
        .unwrap_or_else(AntiCheatIndex::embedded);
    let mut completed = 0;
    let pcgw = ingest_anticheat(client).await;
    let awacy = async {
        let rows: Vec<AwacGame> = client
            .get(ANTICHEAT_DATASET)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok::<_, anyhow::Error>(awacy_index(rows))
    }
    .await;
    for (source, result) in [("pcgamingwiki", pcgw), ("areweanticheatyet", awacy)] {
        let now = Utc::now().to_rfc3339();
        let previous_success = catalog
            .sources
            .get(source)
            .and_then(|s| s.last_success.clone());
        let (last_success, error) = match result {
            Ok(next) if !next.is_empty() => {
                index.merge(&next);
                completed += 1;
                (Some(now.clone()), None)
            }
            Ok(_) => (
                previous_success,
                Some("source returned an empty protection index".into()),
            ),
            Err(error) => {
                tracing::warn!(source, %error, "protection source unavailable");
                (previous_success, Some(format!("{error:#}")))
            }
        };
        catalog.sources.insert(
            source.into(),
            dll_catalog::SourceHealth {
                last_attempt: now,
                last_success,
                error,
                families: vec![],
            },
        );
    }
    if completed == 0 {
        return Err(anyhow!(
            "all protection sources failed; previous index preserved"
        ));
    }
    Ok(index)
}

fn awacy_index(rows: Vec<AwacGame>) -> AntiCheatIndex {
    let mut index = AntiCheatIndex::default();
    for row in rows {
        let anticheats = clean_tokens(&row.anticheats.join(","));
        if anticheats.is_empty() {
            continue;
        }
        let key = normalize_name(&row.name);
        if key.is_empty() {
            continue;
        }
        let entry = AntiCheatEntry {
            anticheats,
            anti_tamper: vec![],
            status: row.status,
        };
        if let Some(appid) = row
            .store_ids
            .get("steam")
            .and_then(|v| v.as_str())
            .and_then(first_appid)
        {
            index.by_appid.insert(appid, entry.clone());
        }
        index.by_name.insert(key, entry);
    }
    index
}

fn github_get(client: &reqwest::Client, url: &str) -> reqwest::RequestBuilder {
    assert!(url.starts_with("https://api.github.com/"));
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
    }
    request
}

/// Recent AMD SDKs distribute signed runtimes in the Git repository. Their
/// release ZIPs are samples, not the SDK archive used by earlier versions.
async fn ingest_fidelityfx(
    client: &reqwest::Client,
    vendors: &mut BTreeMap<String, BTreeMap<String, FamilyEntry>>,
) -> Result<()> {
    const REPO: &str = "GPUOpen-LibrariesAndSDKs/FidelityFX-SDK";
    #[derive(Deserialize)]
    struct Commit {
        sha: String,
    }
    #[derive(Deserialize)]
    struct Tree {
        tree: Vec<TreeEntry>,
        truncated: bool,
    }
    #[derive(Deserialize)]
    struct TreeEntry {
        path: String,
    }
    let releases: Vec<GhRelease> = github_get(
        client,
        &format!("https://api.github.com/repos/{REPO}/releases?per_page=100"),
    )
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
    for package in releases
        .iter()
        .filter(|r| pack_version(r.tag_name.trim_start_matches('v')) >= pack_version("1.1.0"))
    {
        let commit: Commit = github_get(
            client,
            &format!(
                "https://api.github.com/repos/{REPO}/commits/{}",
                package.tag_name
            ),
        )
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
        if commit.sha.len() != 40 || !commit.sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(anyhow!("invalid upstream commit identity"));
        }
        let tree: Tree = github_get(
            client,
            &format!(
                "https://api.github.com/repos/{REPO}/git/trees/{}?recursive=1",
                commit.sha
            ),
        )
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
        if tree.truncated {
            return Err(anyhow!("AMD repository tree was truncated"));
        }
        let mut count = 0;
        for rule in FSR_RULES {
            let paths = [
                format!("Kits/FidelityFX/signedbin/{}", rule.filename),
                format!("PrebuiltSignedDLL/{}", rule.filename),
            ];
            let Some(path) = paths
                .iter()
                .find(|path| tree.tree.iter().any(|entry| entry.path == **path))
            else {
                continue;
            };
            let url = format!(
                "https://raw.githubusercontent.com/{REPO}/{}/{path}",
                commit.sha
            );
            let bytes = download_bytes(client, &url).await?;
            let identity = pe_version::inspect_pe(&bytes)?;
            if identity.architecture != dlssync_contracts::Architecture::X64 || !identity.is_dll {
                return Err(anyhow!("AMD {} is not an x64 DLL", rule.filename));
            }
            let version = pe_version::parse_bytes(&bytes)?.file_version;
            let (signed_hint, signature_subject, signature_status) =
                inspect_signature(&bytes, rule.vendor)?;
            let ext = ExtractedDll {
                vendor: rule.vendor,
                family: rule.family,
                filename: rule.filename.into(),
                zip_entry: String::new(),
                sha256: dll_catalog::hex_sha256(&bytes),
                size: bytes.len() as u64,
                signed_hint,
                signature_subject,
                signature_status,
                file_version: Some(version.clone()),
            };
            let mut artifact =
                artifact_from_extracted(&ext, package.tag_name.trim_start_matches('v'), &url);
            artifact.archive_entry = None;
            artifact.package_id = format!("{REPO}@{}", commit.sha);
            let release = Release {
                artifact: Some(artifact),
                version_packed: pack_version(&version),
                version,
                filename: ext.filename,
                sha256: ext.sha256,
                size_bytes: ext.size,
                signed: ext.signed_hint,
                released_at: package.published_at.unwrap_or_else(Utc::now),
                source: format!("{REPO}@{}", package.tag_name),
                cdn_url: url,
                release_notes: package.name.clone(),
                signature_subject: ext.signature_subject,
                channel: if package.prerelease {
                    "experimental".into()
                } else {
                    "stable".into()
                },
                is_dev: false,
                min_driver: None,
                hash_algorithm: "sha256".into(),
                zip_entry: None,
            };
            upsert_family(vendors, rule.vendor, rule.family, vec![release]);
            count += 1;
        }
        if count == 0 {
            return Err(anyhow!(
                "AMD {} has no recognized signed runtimes",
                package.tag_name
            ));
        }
    }
    Ok(())
}

async fn ingest_dlss_swapper(
    client: &reqwest::Client,
    vendors: &mut BTreeMap<String, BTreeMap<String, FamilyEntry>>,
) -> Result<()> {
    let previous = vendors.clone();
    tracing::info!("fetching DLSS Swapper manifest");
    let body = client
        .get(DLSS_SWAPPER_MANIFEST)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let manifest: SwapperManifest =
        serde_json::from_str(&body).context("parse dlss-swapper manifest")?;
    merge_swapper(
        vendors,
        "nvidia",
        "dlss_sr",
        "nvngx_dlss.dll",
        &manifest.dlss,
    );
    merge_swapper(
        vendors,
        "nvidia",
        "dlss_rr",
        "nvngx_dlssd.dll",
        &manifest.dlss_d,
    );
    merge_swapper(
        vendors,
        "nvidia",
        "dlss_fg",
        "nvngx_dlssg.dll",
        &manifest.dlss_g,
    );
    merge_swapper(
        vendors,
        "amd",
        "fsr_upscaler",
        "amd_fidelityfx_dx12.dll",
        &manifest.fsr_31_dx12,
    );
    merge_swapper(
        vendors,
        "amd",
        "fsr_upscaler_vk",
        "amd_fidelityfx_vk.dll",
        &manifest.fsr_31_vk,
    );
    merge_swapper(vendors, "intel", "xess_sr", "libxess.dll", &manifest.xess);
    merge_swapper(
        vendors,
        "intel",
        "xess_sr_dx11",
        "libxess_dx11.dll",
        &manifest.xess_dx11,
    );
    merge_swapper(
        vendors,
        "intel",
        "xess_fg",
        "libxess_fg.dll",
        &manifest.xess_fg,
    );
    merge_swapper(vendors, "intel", "xell", "libxell.dll", &manifest.xell);
    inspect_new_swapper_releases(client, vendors, &previous).await?;
    Ok(())
}

fn minimum_driver_for(vendor: &str, family: &str, version: &str) -> Option<String> {
    match (vendor, family) {
        // NVIDIA documents R455+ for DLSS 2; RR and frame generation require newer branches.
        ("nvidia", "dlss_sr" | "sl_dlss_sr") => Some("455.00".into()),
        ("nvidia", "dlss_rr" | "sl_dlss_rr") => Some("535.98".into()),
        ("nvidia", "dlss_fg" | "sl_dlss_fg") => Some("522.25".into()),
        // DLSS 5 / Neural Rendering first appears in NVIDIA's 610.47 driver branch.
        ("nvidia", _) if pack_version(version) >= pack_version("5.0.0") => Some("610.47".into()),
        ("amd", family) if family.starts_with("fsr") => Some("22.7.1".into()),
        ("intel", family) if family.starts_with("xess") || family == "xell" => {
            Some("31.0.101.4255".into())
        }
        _ => None,
    }
}

async fn inspect_new_swapper_releases(
    client: &reqwest::Client,
    vendors: &mut BTreeMap<String, BTreeMap<String, FamilyEntry>>,
    previous: &BTreeMap<String, BTreeMap<String, FamilyEntry>>,
) -> Result<()> {
    for (vendor, family) in [
        ("nvidia", "dlss_sr"),
        ("nvidia", "dlss_rr"),
        ("nvidia", "dlss_fg"),
        ("amd", "fsr_upscaler"),
        ("amd", "fsr_upscaler_vk"),
        ("intel", "xess_sr"),
        ("intel", "xess_sr_dx11"),
        ("intel", "xess_fg"),
        ("intel", "xell"),
    ] {
        let Some(entry) = vendors.get_mut(vendor).and_then(|v| v.get_mut(family)) else {
            continue;
        };
        let mut newest = BTreeMap::<String, u64>::new();
        for release in entry
            .releases
            .iter()
            .filter(|r| r.channel == "stable" && !r.is_dev)
        {
            newest
                .entry(release.filename.clone())
                .and_modify(|version| *version = (*version).max(release.version_packed))
                .or_insert(release.version_packed);
        }
        for release in &mut entry.releases {
            let known = previous
                .get(vendor)
                .and_then(|v| v.get(family))
                .is_some_and(|f| {
                    f.releases.iter().any(|old| {
                        old.version == release.version && old.filename == release.filename
                    })
                });
            // Historical MD5 records retain their explicit algorithm. New files
            // and current candidates must be inspected and receive SHA-256.
            if release
                .artifact
                .as_ref()
                .is_some_and(|a| a.signature_status == dlssync_contracts::SignatureStatus::Verified)
                || (known && Some(release.version_packed) != newest.get(&release.filename).copied())
            {
                continue;
            }
            tracing::info!(family, version = %release.version, "inspecting community artifact");
            let bytes = download_bytes(client, &release.cdn_url).await?;
            if dll_catalog::looks_like_zip(&bytes) && release.zip_entry.is_none() {
                let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes))?;
                let mut matches = Vec::new();
                for index in 0..archive.len() {
                    let file = archive.by_index(index)?;
                    let name = file.name().replace('\\', "/");
                    if name
                        .rsplit('/')
                        .next()
                        .is_some_and(|n| n.eq_ignore_ascii_case(&release.filename))
                        && production_rank(&name.to_lowercase()) > 0
                        && dll_catalog::v3::safe_archive_entry(&name, &release.filename)
                    {
                        matches.push(name);
                    }
                }
                if matches.len() != 1 {
                    return Err(anyhow!(
                        "{} has no unique x64 production entry",
                        release.filename
                    ));
                }
                release.zip_entry = matches.pop();
            }
            let directory = tempfile::tempdir()?;
            let path = dll_catalog::extract_dll_from_bytes(&bytes, release, directory.path())?;
            let identity = pe_version::read_pe_identity(&path)?;
            if identity.architecture != dlssync_contracts::Architecture::X64 || !identity.is_dll {
                return Err(anyhow!("{} is not an x64 DLL", release.filename));
            }
            let file_bytes = std::fs::read(&path)?;
            let file_version = pe_version::parse_bytes(&file_bytes)?.file_version;
            let (signed_hint, signature_subject, signature_status) =
                inspect_signature(&file_bytes, vendor)?;
            let extracted = ExtractedDll {
                vendor,
                family,
                filename: release.filename.clone(),
                zip_entry: release.zip_entry.clone().unwrap_or_default(),
                sha256: dll_catalog::hex_sha256(&file_bytes),
                size: file_bytes.len() as u64,
                signed_hint,
                signature_subject,
                signature_status,
                file_version: Some(file_version.clone()),
            };
            let mut artifact =
                artifact_from_extracted(&extracted, &release.version, &release.cdn_url);
            artifact.archive_entry = release.zip_entry.clone();
            release.version = file_version;
            release.version_packed = pack_version(&release.version);
            release.hash_algorithm = "sha256".into();
            release.sha256 = extracted.sha256;
            release.size_bytes = extracted.size;
            release.signed = extracted.signed_hint;
            release.signature_subject = extracted.signature_subject;
            release.artifact = Some(artifact);
        }
        entry.latest = entry
            .releases
            .iter()
            .filter(|r| r.channel == "stable" && !r.is_dev)
            .max_by_key(|r| r.version_packed)
            .map(|r| r.version.clone())
            .unwrap_or_default();
    }
    Ok(())
}

fn vendor_subject(vendor: &str) -> &'static str {
    match vendor {
        "amd" => "Advanced Micro Devices, Inc.",
        "intel" => "Intel Corporation",
        "microsoft" => "Microsoft Corporation",
        _ => "NVIDIA Corporation",
    }
}

fn merge_swapper(
    vendors: &mut BTreeMap<String, BTreeMap<String, FamilyEntry>>,
    vendor: &str,
    family: &str,
    filename: &str,
    entries: &[SwapperEntry],
) {
    let releases: Vec<Release> = entries
        .iter()
        .map(|e| {
            let released_at = e
                .signed_datetime
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(Utc::now);
            let is_experimental = e
                .additional_label
                .as_deref()
                .map(|s| {
                    s.to_lowercase().contains("beta") || s.to_lowercase().contains("experimental")
                })
                .unwrap_or(false);
            Release {
                artifact: None,
                version: e.version.clone(),
                version_packed: pack_version(&e.version),
                filename: filename.to_string(),
                sha256: e.md5_hash.clone().to_lowercase(),
                hash_algorithm: "md5".to_string(),
                size_bytes: e.file_size.unwrap_or(0),
                signed: e.is_signature_valid.unwrap_or(false),
                released_at,
                source: e
                    .dll_source
                    .clone()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "beeradmoore/dlss-swapper".to_string()),
                cdn_url: e.download_url.clone(),
                release_notes: e
                    .internal_name
                    .clone()
                    .or_else(|| e.file_description.clone()),
                // The source reports signature validity, but does not identify
                // the observed signer. Do not fill this from the vendor name.
                signature_subject: None,
                channel: if is_experimental {
                    "experimental".into()
                } else {
                    "stable".into()
                },
                is_dev: false,
                min_driver: minimum_driver_for(vendor, family, &e.version),
                zip_entry: None,
            }
        })
        .collect();
    upsert_family(vendors, vendor, family, releases);
}

/// Union new releases into a (vendor, family) entry instead of replacing it, so a
/// second upstream source for the same family (e.g. FidelityFX-SDK on top of
/// dlss-swapper FSR) extends the history rather than clobbering it. Releases are
/// deduped by (version, filename) and `latest` is recomputed from the uniformly
/// packed version across the merged set.
fn upsert_family(
    vendors: &mut BTreeMap<String, BTreeMap<String, FamilyEntry>>,
    vendor: &str,
    family: &str,
    mut new_releases: Vec<Release>,
) {
    let fam = vendors
        .entry(vendor.to_string())
        .or_default()
        .entry(family.to_string())
        .or_insert_with(|| FamilyEntry {
            latest: String::new(),
            releases: Vec::new(),
        });
    for mut release in new_releases.drain(..) {
        if let Some(index) = fam.releases.iter().position(|old| {
            old.filename == release.filename
                && old.version == release.version
                && (old.cdn_url == release.cdn_url || old.artifact.is_none())
        }) {
            let previous = &fam.releases[index];
            // Metadata cannot lower an observation of these exact bytes.
            if previous.sha256 == release.sha256 {
                if let Some(observed) = &previous.artifact {
                    if release.artifact.is_none()
                        || (observed.signature_status
                            == dlssync_contracts::SignatureStatus::Verified
                            && release.artifact.as_ref().is_some_and(|a| {
                                a.signature_status == dlssync_contracts::SignatureStatus::NotChecked
                            }))
                    {
                        release.artifact = previous.artifact.clone();
                        release.signed = previous.signed;
                        release.signature_subject = previous.signature_subject.clone();
                    }
                }
            }
            // A community listing does not replace an inspected SHA-256 artifact.
            if previous.artifact.is_some() && release.artifact.is_none() {
                continue;
            }
            fam.releases[index] = release;
        } else {
            fam.releases.push(release);
        }
    }
    fam.releases.sort_by(|a, b| {
        a.version_packed
            .cmp(&b.version_packed)
            .then_with(|| a.released_at.cmp(&b.released_at))
            .then_with(|| a.filename.cmp(&b.filename))
    });

    fam.latest = fam
        .releases
        .iter()
        .rev()
        .find(|r| r.channel == "stable" && !r.is_dev)
        .map(|r| r.version.clone())
        .unwrap_or_default();
}

async fn ingest_github_zip_releases<F>(
    client: &reqwest::Client,
    vendors: &mut BTreeMap<String, BTreeMap<String, FamilyEntry>>,
    repo: &str,
    rules: &[FilenameRule],
    asset_filter: F,
) -> Result<()>
where
    F: Fn(&GhAsset) -> bool,
{
    tracing::info!(repo, "fetching GitHub releases");
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=100");
    // Authentication belongs only to the GitHub API request, never to downloads
    // or third-party sources sharing this HTTP client.
    let releases: Vec<GhRelease> = github_get(client, &url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    tracing::info!(repo, count = releases.len(), "releases listed");

    let mut by_target: BTreeMap<(String, String), Vec<Release>> = BTreeMap::new();
    for rel in &releases {
        let Some(asset) = rel.assets.iter().find(|a| asset_filter(a)) else {
            tracing::debug!(tag = %rel.tag_name, "no matching asset, skipping");
            continue;
        };
        tracing::info!(repo, tag = %rel.tag_name, asset = %asset.name, size = asset.size, "downloading asset");
        let bytes = match download_bytes(client, &asset.browser_download_url).await {
            Ok(b) => b,
            Err(e) => {
                return Err(e.context(format!("{}: asset download failed", rel.tag_name)));
            }
        };
        let extracted = match extract_dlls_from_zip(&bytes, rules) {
            Ok(v) => v,
            Err(e) => {
                return Err(e.context(format!("{}: archive inspection failed", rel.tag_name)));
            }
        };
        let tag = rel.tag_name.trim_start_matches('v').to_string();
        let released_at = rel.published_at.unwrap_or_else(Utc::now);
        let channel = if rel.prerelease {
            "experimental"
        } else {
            "stable"
        };
        for ext in extracted {
            let Some(file_version) = ext.file_version.clone() else {
                return Err(anyhow!("{}: missing PE file version", ext.filename));
            };
            let release = Release {
                artifact: Some(artifact_from_extracted(
                    &ext,
                    &tag,
                    &asset.browser_download_url,
                )),
                version_packed: pack_version(&file_version),
                version: file_version,
                filename: ext.filename.clone(),
                sha256: ext.sha256,
                hash_algorithm: "sha256".to_string(),
                size_bytes: ext.size,
                signed: ext.signed_hint,
                released_at,
                source: format!("{repo}@{}", rel.tag_name),
                cdn_url: asset.browser_download_url.clone(),
                release_notes: rel.name.clone().or_else(|| rel.body.clone()),
                signature_subject: ext.signature_subject,
                channel: channel.into(),
                is_dev: false,
                min_driver: minimum_driver_for(ext.vendor, ext.family, &tag),
                zip_entry: Some(ext.zip_entry.clone()),
            };
            by_target
                .entry((ext.vendor.into(), ext.family.into()))
                .or_default()
                .push(release);
        }
    }
    if by_target.is_empty() {
        return Err(anyhow!("{repo}: no compatible production artifacts found"));
    }
    for ((vendor, family), list) in by_target {
        upsert_family(vendors, &vendor, &family, list);
    }
    Ok(())
}

fn is_streamline_x64_asset(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("streamline-sdk-v")
        && name.ends_with(".zip")
        && !["aarch64", "arm64", "x86", "win32", "linux"]
            .iter()
            .any(|part| name.contains(part))
}

struct ExtractedDll {
    vendor: &'static str,
    family: &'static str,
    filename: String,
    zip_entry: String,
    sha256: String,
    size: u64,
    signed_hint: bool,
    signature_status: dlssync_contracts::SignatureStatus,
    signature_subject: Option<String>,
    file_version: Option<String>,
}

/// Rank a zip entry path for a basename match: the canonical production runtime
/// (`bin/x64/<name>`) outranks any `/development/` or build-artifact copy, so a
/// multi-copy SDK zip (the Streamline feature plugins ship 4 copies) always
/// records and extracts the signed production binary regardless of entry order.
fn production_rank(path_lower: &str) -> u8 {
    if path_lower.contains("/development/") || path_lower.contains("_artifacts/") {
        0
    } else if path_lower.starts_with("bin/x64/") {
        2
    } else {
        1
    }
}

fn extract_dlls_from_zip(bytes: &[u8], rules: &[FilenameRule]) -> Result<Vec<ExtractedDll>> {
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor)?;
    let mut best: std::collections::HashMap<
        (&'static str, &'static str, &'static str),
        (usize, String, u8),
    > = Default::default();
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        if !entry.is_file() {
            continue;
        }
        let entry_name = entry.name().replace('\\', "/");
        if entry.enclosed_name().is_none() {
            return Err(anyhow!("unsafe archive path: {entry_name}"));
        }
        if entry_name
            .to_ascii_lowercase()
            .split('/')
            .any(|part| matches!(part, "arm64" | "aarch64" | "arm64ec" | "x86" | "win32"))
        {
            continue;
        }
        let base = entry_name.rsplit('/').next().unwrap_or("").to_lowercase();
        let Some(rule) = rules.iter().find(|r| r.filename == base) else {
            continue;
        };
        let rank = production_rank(&entry_name.to_lowercase());
        if rank == 0 {
            continue;
        }
        let key = (rule.vendor, rule.family, rule.filename);
        if best
            .get(&key)
            .is_none_or(|(_, _, best_rank)| rank > *best_rank)
        {
            best.insert(key, (i, entry_name, rank));
        }
    }
    let mut out = Vec::new();
    let mut total_bytes = 0u64;
    for ((vendor, family, filename), (idx, zip_entry, _)) in best {
        let mut entry = zip.by_index(idx)?;
        if entry.size() > 200 * 1024 * 1024 {
            return Err(anyhow!("DLL exceeds 200 MiB limit"));
        }
        total_bytes = total_bytes.saturating_add(entry.size());
        if total_bytes > 1024 * 1024 * 1024 {
            return Err(anyhow!("DLL set exceeds 1 GiB limit"));
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        (&mut entry)
            .take(200 * 1024 * 1024 + 1)
            .read_to_end(&mut buf)?;
        if buf.len() as u64 != entry.size() {
            return Err(anyhow!("DLL size mismatch"));
        }
        let identity = pe_version::inspect_pe(&buf).context("invalid candidate PE header")?;
        if identity.architecture != dlssync_contracts::Architecture::X64 || !identity.is_dll {
            return Err(anyhow!(
                "incompatible archive candidate {zip_entry}: {identity:?}; x64 DLL required"
            ));
        }
        let file_version = pe_version::parse_bytes(&buf).ok().map(|v| v.file_version);
        let mut hasher = Sha256::new();
        hasher.update(&buf);
        let sha = hex::encode(hasher.finalize());
        let (signed_hint, signature_subject, signature_status) = inspect_signature(&buf, vendor)?;
        out.push(ExtractedDll {
            vendor,
            family,
            filename: filename.into(),
            zip_entry,
            sha256: sha,
            size: buf.len() as u64,
            signed_hint,
            signature_subject,
            signature_status,
            file_version,
        });
    }
    if out.is_empty() {
        return Err(anyhow!("no matching DLLs in zip"));
    }
    Ok(out)
}

fn inspect_signature(
    bytes: &[u8],
    vendor: &str,
) -> Result<(bool, Option<String>, dlssync_contracts::SignatureStatus)> {
    use dlssync_contracts::SignatureStatus;
    #[cfg(windows)]
    {
        let info = inspect_closed_dll(bytes, |path| {
            pe_version::read_authenticode(path).context("signature inspection unavailable")
        })?;
        if info.trusted {
            pe_version::enforce_subject(&info, vendor).map_err(|e| anyhow!(e))?;
        }
        let verified = info.trusted && !info.revocation_bypassed;
        let status = if verified {
            SignatureStatus::Verified
        } else if info.subject_cn.is_some() {
            SignatureStatus::Untrusted
        } else {
            SignatureStatus::Missing
        };
        Ok((verified, info.subject_cn, status))
    }
    #[cfg(not(windows))]
    {
        let _ = (bytes, vendor);
        Ok((false, None, SignatureStatus::NotChecked))
    }
}

#[cfg(windows)]
fn inspect_closed_dll<T>(
    bytes: &[u8],
    inspect: impl FnOnce(&std::path::Path) -> Result<T>,
) -> Result<T> {
    use std::io::Write;
    let mut file = tempfile::Builder::new().suffix(".dll").tempfile()?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    // CryptQueryObject rejects an embedded signature while a writer is open.
    // into_temp_path closes the handle while retaining automatic file cleanup.
    let path = file.into_temp_path();
    inspect(&path)
}

fn artifact_from_extracted(
    ext: &ExtractedDll,
    package_version: &str,
    source_url: &str,
) -> dlssync_contracts::ArtifactDescriptor {
    use dlssync_contracts::*;
    let identity = format!(
        "{}/{}/{}/{}/{}/{}",
        ext.vendor, ext.family, ext.filename, source_url, ext.zip_entry, ext.sha256
    );
    ArtifactDescriptor {
        id: hex::encode(Sha256::digest(identity.as_bytes())),
        family: ext.family.into(),
        filename: ext.filename.clone(),
        file_version: ext.file_version.clone(),
        package_version: package_version.into(),
        package_id: format!(
            "archive:{}",
            hex::encode(Sha256::digest(source_url.as_bytes()))
        ),
        compatibility_line: format!(
            "{}:{}",
            ext.family,
            ext.file_version
                .as_deref()
                .unwrap_or("unknown")
                .split('.')
                .next()
                .unwrap_or("unknown")
        ),
        architecture: Architecture::X64,
        hash: ContentHash {
            algorithm: HashAlgorithm::Sha256,
            digest: ext.sha256.clone(),
        },
        size_bytes: ext.size.into(),
        source_url: source_url.into(),
        archive_entry: Some(ext.zip_entry.clone()),
        expected_publisher: Some(vendor_subject(ext.vendor).into()),
        observed_publisher: ext.signature_subject.clone(),
        signature_status: ext.signature_status,
        dependencies: vec![],
        checked_at: Utc::now().to_rfc3339(),
    }
}

fn pack_version(s: &str) -> u64 {
    let parts: Vec<u16> = s
        .split(['.', '-', '+'])
        .take(4)
        .map(|p| {
            p.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
        })
        .map(|p| {
            p.parse::<u64>()
                .map(|n| n.min(u16::MAX as u64) as u16)
                .unwrap_or(0)
        })
        .collect();
    let major = parts.first().copied().unwrap_or(0) as u64;
    let minor = parts.get(1).copied().unwrap_or(0) as u64;
    let build = parts.get(2).copied().unwrap_or(0) as u64;
    let patch = parts.get(3).copied().unwrap_or(0) as u64;
    (major << 48) | (minor << 32) | (build << 16) | patch
}

#[derive(Debug, Deserialize)]
struct NugetIndex {
    versions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct NugetRegistration {
    items: Vec<NugetRegistrationPage>,
}

#[derive(Debug, Deserialize)]
struct NugetRegistrationPage {
    items: Vec<NugetRegistrationLeaf>,
}

#[derive(Debug, Deserialize)]
struct NugetRegistrationLeaf {
    #[serde(rename = "catalogEntry")]
    catalog_entry: NugetCatalogEntry,
}

#[derive(Debug, Deserialize)]
struct NugetCatalogEntry {
    version: String,
    #[serde(default)]
    published: Option<DateTime<Utc>>,
}

const DS_PKG: &str = "microsoft.direct3d.directstorage";
const DS_RULES: &[FilenameRule] = &[
    FilenameRule {
        filename: "dstorage.dll",
        vendor: "microsoft",
        family: "direct_storage",
    },
    FilenameRule {
        filename: "dstoragecore.dll",
        vendor: "microsoft",
        family: "direct_storage_core",
    },
];

/// DirectStorage versions from the NuGet flat-container API. Uses its OWN
/// unauthenticated client because the shared client carries a GitHub bearer for
/// the release sources, and NuGet's Azure backend rejects any request that
/// presents one with HTTP 403.
async fn ingest_directstorage_nuget(
    _shared: &reqwest::Client,
    vendors: &mut BTreeMap<String, BTreeMap<String, FamilyEntry>>,
) -> Result<()> {
    tracing::info!("fetching DirectStorage NuGet index");
    let client = reqwest::Client::builder()
        .user_agent("dlssync-manifest-builder/0.1 (+https://github.com/xt0n1-t3ch/DLSSync)")
        .build()?;
    let idx_url = format!("https://api.nuget.org/v3-flatcontainer/{DS_PKG}/index.json");
    let index: NugetIndex = client
        .get(&idx_url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let reg_url = format!("https://api.nuget.org/v3/registration5-semver1/{DS_PKG}/index.json");
    let mut date_by_ver: BTreeMap<String, DateTime<Utc>> = BTreeMap::new();
    if let Ok(resp) = client.get(&reg_url).send().await {
        if let Ok(reg) = resp.error_for_status()?.json::<NugetRegistration>().await {
            for page in reg.items {
                for leaf in page.items {
                    if let Some(p) = leaf.catalog_entry.published {
                        date_by_ver.insert(leaf.catalog_entry.version.to_lowercase(), p);
                    }
                }
            }
        }
    }

    let mut by_target: BTreeMap<(String, String), Vec<Release>> = BTreeMap::new();
    for ver in index.versions {
        let is_prerelease = ver.contains('-');
        let pkg_url =
            format!("https://api.nuget.org/v3-flatcontainer/{DS_PKG}/{ver}/{DS_PKG}.{ver}.nupkg");
        tracing::info!(version = %ver, "downloading DirectStorage nupkg");
        let bytes = match download_bytes(&client, &pkg_url).await {
            Ok(b) => b,
            Err(e) => {
                return Err(e.context(format!("DirectStorage {ver}: package download failed")));
            }
        };
        let extracted = match extract_dlls_from_zip(&bytes, DS_RULES) {
            Ok(v) => v,
            Err(e) => {
                return Err(e.context(format!("DirectStorage {ver}: archive inspection failed")));
            }
        };
        let released_at = date_by_ver
            .get(&ver.to_lowercase())
            .copied()
            .unwrap_or_else(Utc::now);
        let channel = if is_prerelease {
            "experimental"
        } else {
            "stable"
        };
        for ext in extracted {
            let file_version = ext
                .file_version
                .clone()
                .context("DirectStorage DLL has no file version")?;
            let release = Release {
                artifact: Some(artifact_from_extracted(&ext, &ver, &pkg_url)),
                version: file_version.clone(),
                version_packed: pack_version(&file_version),
                filename: ext.filename.clone(),
                sha256: ext.sha256,
                hash_algorithm: "sha256".to_string(),
                size_bytes: ext.size,
                signed: ext.signed_hint,
                released_at,
                source: format!("nuget:{DS_PKG}@{ver}"),
                cdn_url: pkg_url.clone(),
                release_notes: None,
                signature_subject: ext.signature_subject,
                channel: channel.into(),
                is_dev: false,
                min_driver: minimum_driver_for(ext.vendor, ext.family, &ver),
                zip_entry: Some(ext.zip_entry.clone()),
            };
            by_target
                .entry((ext.vendor.into(), ext.family.into()))
                .or_default()
                .push(release);
        }
    }
    for ((vendor, family), list) in by_target {
        upsert_family(vendors, &vendor, &family, list);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn signature_reader_receives_a_closed_complete_file() {
        use std::os::windows::fs::OpenOptionsExt;
        inspect_closed_dll(b"complete bytes", |path| {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(path)?;
            assert_eq!(file.metadata()?.len(), 14);
            Ok(())
        })
        .unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "uses a supplied signed vendor DLL; set DLSSYNC_SIGNATURE_FIXTURE"]
    fn signed_vendor_fixture_remains_signed_after_staging() {
        let path = std::env::var_os("DLSSYNC_SIGNATURE_FIXTURE").expect("signed vendor DLL path");
        let vendor = std::env::var("DLSSYNC_SIGNATURE_VENDOR").unwrap_or_else(|_| "nvidia".into());
        let bytes = std::fs::read(path).unwrap();
        let (valid, subject, status) = inspect_signature(&bytes, &vendor).unwrap();
        assert!(valid, "{status:?}: {subject:?}");
        assert_eq!(status, dlssync_contracts::SignatureStatus::Verified);
    }

    #[test]
    fn cargo_permission_error_is_not_an_empty_success() {
        let response: CargoResp = serde_json::from_str(
            r#"{"error":{"code":"permissiondenied","info":"Cargo queries require permission"}}"#,
        )
        .unwrap();
        assert!(response
            .into_rows()
            .unwrap_err()
            .to_string()
            .contains("permissiondenied"));
    }

    #[test]
    fn public_protection_index_preserves_steam_identity_and_engine() {
        let rows: Vec<AwacGame> = serde_json::from_str(r#"[{"name":"Game","anticheats":["Easy Anti-Cheat"],"status":"Supported","storeIds":{"steam":"12345"}}]"#).unwrap();
        let index = awacy_index(rows);
        assert_eq!(index.by_appid[&12345].anticheats, ["Easy Anti-Cheat"]);
        assert_eq!(index.by_name["game"].status.as_deref(), Some("Supported"));
    }

    #[test]
    fn failed_source_keeps_previous_data_and_last_success() {
        let mut catalog = dll_catalog::embedded_fallback_catalog().unwrap();
        let original = serde_json::to_value(&catalog.vendors).unwrap();
        let success_time = "2026-09-01T00:00:00+00:00";
        catalog.sources.insert(
            "streamline".into(),
            dll_catalog::SourceHealth {
                last_attempt: success_time.into(),
                last_success: Some(success_time.into()),
                error: None,
                families: vec![],
            },
        );
        let attempt = Utc::now();
        assert!(!finish_source(
            &mut catalog,
            BTreeMap::new(),
            "streamline",
            attempt,
            Err(anyhow!("archive removed"))
        ));
        assert_eq!(serde_json::to_value(&catalog.vendors).unwrap(), original);
        let status = &catalog.sources["streamline"];
        assert_eq!(status.last_success.as_deref(), Some(success_time));
        assert_eq!(status.last_attempt, attempt.to_rfc3339());
        assert_eq!(status.error.as_deref(), Some("archive removed"));
    }

    #[test]
    fn unknown_source_success_time_is_not_invented_after_failure() {
        let mut catalog = dll_catalog::embedded_fallback_catalog().unwrap();
        let proposed = catalog.vendors.clone();
        finish_source(
            &mut catalog,
            proposed,
            "xess",
            Utc::now(),
            Err(anyhow!("offline")),
        );
        assert!(catalog.sources["xess"].last_success.is_none());
    }

    #[test]
    fn pack_handles_simple_tags() {
        assert_eq!(pack_version("310.6.0"), (310u64 << 48) | (6u64 << 32));
        assert_eq!(
            pack_version("2.10.3"),
            (2u64 << 48) | (10u64 << 32) | (3u64 << 16)
        );
        assert_eq!(pack_version("1.4.0-preview1"), (1u64 << 48) | (4u64 << 32));
    }

    #[test]
    fn streamline_rules_source_sl_dlss_plugins_into_their_own_families() {
        for (file, family) in [
            ("sl.dlss.dll", "sl_dlss_sr"),
            ("sl.dlss_g.dll", "sl_dlss_fg"),
            ("sl.dlss_d.dll", "sl_dlss_rr"),
        ] {
            assert!(
                STREAMLINE_RULES
                    .iter()
                    .any(|r| r.filename == file && r.family == family && r.vendor == "nvidia"),
                "{file} must source family {family} (v1.6 Streamline Set Updater) — never the \
                 nvngx 310.x families (that was the v1.5.2 cross-scheme bug)"
            );
        }
    }

    #[test]
    fn production_rank_prefers_bin_x64_over_development_and_artifacts() {
        assert!(
            production_rank("bin/x64/sl.dlss_g.dll")
                > production_rank("bin/x64/development/sl.dlss_g.dll")
        );
        assert!(
            production_rank("bin/x64/sl.dlss_g.dll")
                > production_rank("_artifacts/sl.dlss_g/production_x64/sl.dlss_g.dll")
        );
        assert_eq!(production_rank("bin/x64/development/sl.dlss_g.dll"), 0);
    }

    #[test]
    fn first_appid_takes_base_game_from_comma_list() {
        assert_eq!(first_appid("1245620"), Some(1245620));
        assert_eq!(first_appid("3768760,4707780, 4601250"), Some(3768760));
        assert_eq!(first_appid(" 990080 "), Some(990080));
        assert_eq!(first_appid(""), None);
        assert_eq!(first_appid("not-a-number"), None);
    }

    #[test]
    fn streamline_asset_selection_is_independent_of_upstream_order() {
        let names = [
            "streamline-sdk-v2.14.1-aarch64.zip",
            "streamline-sdk-v2.14.1-arm64ec.zip",
            "streamline-sdk-v2.14.1.zip",
        ];
        assert_eq!(
            names.into_iter().find(|name| is_streamline_x64_asset(name)),
            Some("streamline-sdk-v2.14.1.zip")
        );
        assert!(!is_streamline_x64_asset("streamline-sdk-v2.14.1-x86.zip"));
        assert!(is_streamline_x64_asset("streamline-sdk-v2.14.1-x64.zip"));
    }

    fn test_pe(machine: u16) -> Vec<u8> {
        let mut bytes = vec![0; 128];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&machine.to_le_bytes());
        bytes[84..86].copy_from_slice(&240u16.to_le_bytes());
        bytes[86..88].copy_from_slice(&0x2000u16.to_le_bytes());
        bytes[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        bytes
    }

    fn test_zip(entries: &[(&str, u16)]) -> Vec<u8> {
        use std::io::{Cursor, Write};
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, machine) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&test_pe(*machine)).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn zip_prefers_production_x64_and_does_not_infer_signature() {
        let bytes = test_zip(&[
            ("bin/arm64/sl.common.dll", 0xaa64),
            ("bin/x64/development/sl.common.dll", 0x8664),
            ("bin/x64/sl.common.dll", 0x8664),
        ]);
        let dlls = extract_dlls_from_zip(&bytes, STREAMLINE_RULES).unwrap();
        assert_eq!(dlls.len(), 1);
        assert_eq!(dlls[0].zip_entry, "bin/x64/sl.common.dll");
        assert!(!dlls[0].signed_hint);
        assert!(dlls[0].signature_subject.is_none());
        assert!(dlls[0].file_version.is_none());
    }

    #[test]
    fn x64_path_cannot_disguise_an_arm_binary() {
        let bytes = test_zip(&[("bin/x64/sl.common.dll", 0xaa64)]);
        assert!(extract_dlls_from_zip(&bytes, STREAMLINE_RULES).is_err());
    }

    #[test]
    fn clean_tokens_splits_trims_dedupes_and_drops_noise() {
        assert_eq!(
            clean_tokens("Easy Anti-Cheat, BattlEye , Easy Anti-Cheat"),
            vec!["Easy Anti-Cheat".to_string(), "BattlEye".to_string()]
        );
        assert!(clean_tokens("None, none, false, , Unknown").is_empty());
        assert_eq!(
            clean_tokens("Steam,Ubisoft Connect,Denuvo Anti-Tamper")
                .into_iter()
                .filter(|d| ANTI_TAMPER_MARKERS
                    .iter()
                    .any(|m| d.to_ascii_lowercase().contains(m)))
                .collect::<Vec<_>>(),
            vec!["Denuvo Anti-Tamper".to_string()]
        );
    }

    #[test]
    fn merge_list_unions_case_insensitively() {
        let mut a = vec!["Denuvo Anti-Tamper".to_string()];
        merge_list(&mut a, vec!["denuvo anti-tamper".into(), "Arxan".into()]);
        assert_eq!(
            a,
            vec!["Denuvo Anti-Tamper".to_string(), "Arxan".to_string()]
        );
    }

    #[test]
    fn merge_swapper_maps_fsr_to_amd_family_with_vendor_subject() {
        let entries: Vec<SwapperEntry> = serde_json::from_str(
            r#"[
            {"version":"1.0.0.36208","version_number":1000036208,"md5_hash":"ABC","download_url":"https://x/fsr_a.zip","file_size":100,"is_signature_valid":true},
            {"version":"1.0.1.41314","version_number":1000141314,"md5_hash":"DEF","download_url":"https://x/fsr_b.zip","file_size":200,"is_signature_valid":true}
        ]"#,
        )
        .unwrap();
        let mut vendors: BTreeMap<String, BTreeMap<String, FamilyEntry>> = BTreeMap::new();
        merge_swapper(
            &mut vendors,
            "amd",
            "fsr_upscaler",
            "amd_fidelityfx_dx12.dll",
            &entries,
        );
        let fam = &vendors["amd"]["fsr_upscaler"];
        assert_eq!(fam.releases.len(), 2);
        assert!(fam
            .releases
            .iter()
            .all(|r| r.filename == "amd_fidelityfx_dx12.dll"));
        assert!(fam.releases[0].signature_subject.is_none());
        assert_eq!(fam.latest, "1.0.1.41314");
    }

    #[test]
    fn vendor_subject_maps_each_vendor() {
        assert_eq!(vendor_subject("amd"), "Advanced Micro Devices, Inc.");
        assert_eq!(vendor_subject("intel"), "Intel Corporation");
        assert_eq!(vendor_subject("nvidia"), "NVIDIA Corporation");
    }

    #[test]
    fn capability_minimum_drivers_are_populated_by_family() {
        assert_eq!(
            minimum_driver_for("nvidia", "dlss_rr", "310.6.0").as_deref(),
            Some("535.98")
        );
        assert_eq!(
            minimum_driver_for("nvidia", "dlss_fg", "310.6.0").as_deref(),
            Some("522.25")
        );
        assert_eq!(
            minimum_driver_for("intel", "xess_sr", "2.1.0").as_deref(),
            Some("31.0.101.4255")
        );
    }

    fn rel(ver: &str, file: &str) -> Release {
        Release {
            artifact: None,
            version: ver.to_string(),
            version_packed: pack_version(ver),
            filename: file.to_string(),
            sha256: "x".into(),
            hash_algorithm: "sha256".into(),
            size_bytes: 0,
            signed: false,
            released_at: Utc::now(),
            source: "t".into(),
            cdn_url: "https://x/y".into(),
            release_notes: None,
            signature_subject: None,
            channel: "stable".into(),
            is_dev: false,
            min_driver: None,
            zip_entry: None,
        }
    }

    #[test]
    fn upsert_family_unions_sources_without_clobbering_history() {
        let mut vendors: BTreeMap<String, BTreeMap<String, FamilyEntry>> = BTreeMap::new();
        upsert_family(
            &mut vendors,
            "amd",
            "fsr_upscaler",
            vec![
                rel("3.1.0", "amd_fidelityfx_dx12.dll"),
                rel("3.1.1", "amd_fidelityfx_dx12.dll"),
                rel("3.1.2", "amd_fidelityfx_dx12.dll"),
            ],
        );
        upsert_family(
            &mut vendors,
            "amd",
            "fsr_upscaler",
            vec![rel("2.0.0", "amd_fidelityfx_upscaler_dx12.dll")],
        );
        let fam = &vendors["amd"]["fsr_upscaler"];
        assert_eq!(
            fam.releases.len(),
            4,
            "second source must extend, not clobber, the first"
        );
        assert_eq!(
            fam.latest, "3.1.2",
            "uniform packing keeps the genuinely newest version as latest across sources"
        );
    }

    #[test]
    fn upsert_family_dedupes_same_version_and_filename() {
        let mut vendors: BTreeMap<String, BTreeMap<String, FamilyEntry>> = BTreeMap::new();
        upsert_family(&mut vendors, "amd", "fsr_fg", vec![rel("1.1.2", "a.dll")]);
        upsert_family(&mut vendors, "amd", "fsr_fg", vec![rel("1.1.2", "a.dll")]);
        assert_eq!(vendors["amd"]["fsr_fg"].releases.len(), 1);
    }
}
