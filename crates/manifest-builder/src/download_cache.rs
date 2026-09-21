use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

const MAX_DOWNLOAD: usize = 1024 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct CacheMetadata {
    etag: Option<String>,
    sha256: String,
}

pub async fn download_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let directory = std::env::var_os("DLSSYNC_MANIFEST_CACHE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("dlssync-manifest-cache"));
    download_cached(client, url, &directory).await
}

async fn download_cached(client: &reqwest::Client, url: &str, directory: &Path) -> Result<Vec<u8>> {
    let key = hex::encode(Sha256::digest(url.as_bytes()));
    let data_path = directory.join(format!("{key}.bin"));
    let metadata_path = directory.join(format!("{key}.json"));
    let cached = read_cached(&data_path, &metadata_path);
    let mut request = client.get(url).timeout(std::time::Duration::from_secs(600));
    if let Some((metadata, _)) = &cached {
        if let Some(etag) = &metadata.etag {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
    }
    let mut response = request.send().await?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return cached
            .map(|(_, bytes)| bytes)
            .ok_or_else(|| anyhow!("server returned 304 without a verified cached archive"));
    }
    response = response.error_for_status()?;
    let length = response.content_length();
    if length.is_some_and(|n| n > MAX_DOWNLOAD as u64) {
        return Err(anyhow!("archive exceeds 1 GiB download limit"));
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > MAX_DOWNLOAD {
            return Err(anyhow!("archive exceeds 1 GiB download limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    if length.is_some_and(|n| n != bytes.len() as u64) {
        return Err(anyhow!("archive download was truncated"));
    }
    let metadata = CacheMetadata {
        etag,
        sha256: hex::encode(Sha256::digest(&bytes)),
    };
    if let Err(error) = cache_write(&data_path, &metadata_path, &bytes, &metadata) {
        tracing::warn!(%error, "download succeeded but archive cache could not be written");
    }
    Ok(bytes)
}

fn read_cached(data_path: &Path, metadata_path: &Path) -> Option<(CacheMetadata, Vec<u8>)> {
    if std::fs::metadata(data_path).ok()?.len() > MAX_DOWNLOAD as u64 {
        return None;
    }
    let metadata: CacheMetadata =
        serde_json::from_slice(&std::fs::read(metadata_path).ok()?).ok()?;
    let bytes = std::fs::read(data_path).ok()?;
    if hex::encode(Sha256::digest(&bytes)) != metadata.sha256 {
        return None;
    }
    Some((metadata, bytes))
}

fn cache_write(data: &Path, meta: &Path, bytes: &[u8], metadata: &CacheMetadata) -> Result<()> {
    super::write_atomic(data, bytes)?;
    super::write_atomic(meta, &serde_json::to_vec(metadata)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn conditional_request_reuses_only_verified_bytes() {
        let server = MockServer::start().await;
        let directory = tempfile::tempdir().unwrap();
        let client = reqwest::Client::new();
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(b"archive")
                    .insert_header("ETag", "\"v1\""),
            )
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(
            download_cached(&client, &server.uri(), directory.path())
                .await
                .unwrap(),
            b"archive"
        );
        Mock::given(method("GET"))
            .and(header("If-None-Match", "\"v1\""))
            .respond_with(ResponseTemplate::new(304))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(
            download_cached(&client, &server.uri(), directory.path())
                .await
                .unwrap(),
            b"archive"
        );
    }

    #[test]
    fn altered_cache_is_not_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("file.bin");
        let meta = dir.path().join("file.json");
        cache_write(
            &data,
            &meta,
            b"original",
            &CacheMetadata {
                etag: Some("v1".into()),
                sha256: hex::encode(Sha256::digest(b"original")),
            },
        )
        .unwrap();
        std::fs::write(&data, b"changed").unwrap();
        assert!(read_cached(&data, &meta).is_none());
    }
}
