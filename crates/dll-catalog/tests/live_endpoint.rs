use dll_catalog::{Catalog, CANONICAL_MANIFEST_URL};

const EXPECTED_VENDORS: &[&str] = &["nvidia", "amd", "intel", "microsoft"];
const EXPECTED_FAMILIES: &[(&str, &str)] = &[
    ("nvidia", "dlss_sr"),
    ("nvidia", "dlss_fg"),
    ("nvidia", "dlss_rr"),
    ("nvidia", "streamline"),
    ("nvidia", "reflex"),
    ("intel", "xess_sr"),
    ("intel", "xess_fg"),
    ("intel", "xell"),
    ("amd", "fsr_upscaler"),
    ("amd", "fsr_fg"),
    ("microsoft", "direct_storage"),
];

#[tokio::test]
#[ignore = "hits live jsDelivr CDN — run via `cargo test -p dll-catalog --test live_endpoint -- --ignored`"]
async fn live_catalog_endpoint_returns_valid_schema_v2() {
    let url = CANONICAL_MANIFEST_URL.replace("manifest-v3.json", "manifest.json");
    verify_live_catalog(&url, 2).await;
}

#[tokio::test]
#[ignore = "hits live jsDelivr CDN — run via `cargo test -p dll-catalog --test live_endpoint -- --ignored`"]
async fn live_catalog_endpoint_returns_valid_schema_v3() {
    let catalog = verify_live_catalog(CANONICAL_MANIFEST_URL, 3).await;
    assert!(
        !catalog.sources.is_empty(),
        "v3 must expose source freshness"
    );
    assert!(catalog
        .vendors
        .values()
        .flat_map(|families| families.values())
        .flat_map(|family| &family.releases)
        .any(|release| release.artifact.is_some()));
}

async fn verify_live_catalog(url: &str, schema: u32) -> Catalog {
    let client = reqwest::Client::builder()
        .user_agent("dlssync-live-endpoint-test/1.0")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("build reqwest client");

    let catalog = Catalog::fetch_from(&client, url)
        .await
        .unwrap_or_else(|e| panic!("Catalog::fetch_from({url}) failed: {e}"));

    assert_eq!(catalog.schema_version, schema);

    for vendor in EXPECTED_VENDORS {
        assert!(
            catalog.vendors.contains_key(*vendor),
            "live manifest missing vendor `{vendor}`. Found: {:?}",
            catalog.vendors.keys().collect::<Vec<_>>()
        );
    }

    for (vendor, family) in EXPECTED_FAMILIES {
        let releases = catalog.releases(vendor, family);
        assert!(
            !releases.is_empty(),
            "live manifest has no releases for {vendor}/{family}"
        );

        let latest = catalog.latest(vendor, family).unwrap_or_else(|| {
            panic!("{vendor}/{family} has no latest release matching its `latest` pointer")
        });

        assert!(
            !latest.sha256.is_empty(),
            "{vendor}/{family} latest release has empty sha256"
        );
        assert!(
            latest.size_bytes > 0,
            "{vendor}/{family} latest release has zero size_bytes"
        );
        assert!(
            latest.cdn_url.starts_with("https://"),
            "{vendor}/{family} latest cdn_url must be https: {}",
            latest.cdn_url
        );

        let sha_len = latest.sha256.len();
        assert!(
            sha_len == 64 || sha_len == 32,
            "{vendor}/{family} sha256 has invalid length {sha_len} (expected 64 SHA-256 or 32 MD5)"
        );
        assert!(
            latest.sha256.chars().all(|c| c.is_ascii_hexdigit()),
            "{vendor}/{family} sha256 contains non-hex characters"
        );
    }
    catalog
}
