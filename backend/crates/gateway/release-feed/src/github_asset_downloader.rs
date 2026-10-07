use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use reqwest::header::USER_AGENT;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use delta_usecase::{AssetDownloadError, AssetDownloader, DownloadProgress, ReleaseAsset};

use crate::https_client::{https_client_builder, USER_AGENT_VALUE};
use crate::{ClientBuildError, DOWNLOAD_CONNECT_TIMEOUT, DOWNLOAD_STALL_TIMEOUT};

/// What a file is called while it downloads: its final name plus this.
const TEMP_SUFFIX: &str = ".part";

/// The [`AssetDownloader`] for GitHub release assets.
///
/// Follows the redirect GitHub answers a `browser_download_url` with (to its
/// asset host); whether the URL it is handed may be fetched at all is the
/// caller's decision.
#[derive(Debug, Clone)]
pub struct GithubAssetDownloader {
    client: reqwest::Client,
}

impl GithubAssetDownloader {
    /// A downloader giving up when connecting takes longer than
    /// [`DOWNLOAD_CONNECT_TIMEOUT`] or no data arrives for
    /// [`DOWNLOAD_STALL_TIMEOUT`], with no cap on the whole transfer.
    pub fn new() -> Result<Self, ClientBuildError> {
        let client = https_client_builder()?
            .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
            .read_timeout(DOWNLOAD_STALL_TIMEOUT)
            .build()?;
        Ok(Self { client })
    }

    /// Stream `url` into a new file at `temp`, reporting progress, and return
    /// the sha256 of what arrived as lowercase hex.
    async fn fetch(
        &self,
        url: &str,
        temp: &Path,
        progress: &(dyn Fn(DownloadProgress) + Send + Sync),
    ) -> Result<String, AssetDownloadError> {
        let mut response = self
            .client
            .get(url)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .send()
            .await
            .map_err(|err| AssetDownloadError::Request(err.into()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(AssetDownloadError::Status(status.as_u16()));
        }
        let total = response.content_length();
        let io = |source| AssetDownloadError::Io {
            path: temp.to_path_buf(),
            source,
        };
        let mut file = tokio::fs::File::create(temp).await.map_err(io)?;
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        progress(DownloadProgress { received, total });
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|err| AssetDownloadError::Request(err.into()))?
        {
            hasher.update(&chunk);
            file.write_all(&chunk).await.map_err(io)?;
            received += chunk.len() as u64;
            progress(DownloadProgress { received, total });
        }
        if let Some(expected) = total.filter(|&expected| expected != received) {
            return Err(AssetDownloadError::Truncated { received, expected });
        }
        file.sync_all().await.map_err(io)?;
        Ok(to_hex(&hasher.finalize()))
    }
}

#[async_trait]
impl AssetDownloader for GithubAssetDownloader {
    async fn download(
        &self,
        asset: &ReleaseAsset,
        dir: &Path,
        progress: &(dyn Fn(DownloadProgress) + Send + Sync),
    ) -> Result<PathBuf, AssetDownloadError> {
        let expected = asset
            .sha256()
            .ok_or_else(|| AssetDownloadError::NoDigest(asset.name.clone()))?;
        if !is_plain_file_name(&asset.name) {
            return Err(AssetDownloadError::BadName(asset.name.clone()));
        }
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|source| AssetDownloadError::Io {
                path: dir.to_path_buf(),
                source,
            })?;
        let target = dir.join(&asset.name);
        let temp = dir.join(format!("{}{TEMP_SUFFIX}", asset.name));

        let verified =
            match self.fetch(&asset.download_url, &temp, progress).await {
                Ok(actual) if actual == expected => tokio::fs::rename(&temp, &target)
                    .await
                    .map_err(|source| AssetDownloadError::Io {
                        path: target.clone(),
                        source,
                    }),
                Ok(actual) => Err(AssetDownloadError::DigestMismatch { expected, actual }),
                Err(err) => Err(err),
            };
        if let Err(err) = verified {
            remove_leftover(&temp).await;
            return Err(err);
        }
        remove_others(dir, &asset.name).await;
        Ok(target)
    }
}

/// Whether `name` names a file directly in a directory.
fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Remove a partial download. A failure is logged: the file is then left
/// under its temporary name, never under the asset's.
async fn remove_leftover(path: &Path) {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => tracing::warn!(
            path = %path.display(),
            error = %err,
            "could not remove a partial download"
        ),
    }
}

/// Remove every file in `dir` but `keep`: the files of other versions, and
/// any partial download an earlier run left behind. A failure is logged and
/// the rest are still removed.
async fn remove_others(dir: &Path, keep: &str) {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(err) => {
            tracing::warn!(
                dir = %dir.display(),
                error = %err,
                "could not list the update directory to remove older downloads"
            );
            return;
        }
    };
    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => {
                tracing::warn!(
                    dir = %dir.display(),
                    error = %err,
                    "could not list the update directory to remove older downloads"
                );
                break;
            }
        };
        if entry.file_name() == keep {
            continue;
        }
        let path = entry.path();
        if let Err(err) = tokio::fs::remove_file(&path).await {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "could not remove an older download"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::test_server::serve_raw;

    const NAME: &str = "delta-desktop_0.6.0_amd64.deb";
    const BODY: &[u8] = b"pretend this is a debian package";

    fn sha256_hex(bytes: &[u8]) -> String {
        to_hex(&Sha256::digest(bytes))
    }

    fn ok_response(body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    fn asset(url: String, digest: Option<String>) -> ReleaseAsset {
        ReleaseAsset {
            name: NAME.into(),
            download_url: url,
            digest,
        }
    }

    /// The names of the files in `dir`, sorted; none when it does not exist.
    fn files_in(dir: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    async fn download(
        asset: &ReleaseAsset,
        dir: &Path,
    ) -> (Result<PathBuf, AssetDownloadError>, Vec<DownloadProgress>) {
        let reports = Mutex::new(Vec::new());
        let result = GithubAssetDownloader::new()
            .unwrap()
            .download(asset, dir, &|progress| {
                reports.lock().unwrap().push(progress)
            })
            .await;
        (result, reports.into_inner().unwrap())
    }

    #[tokio::test]
    async fn a_matching_digest_leaves_exactly_the_verified_file() {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("updates");
        let (url, server) = serve_raw(&format!("/download/{NAME}"), ok_response(BODY)).await;
        let digest = format!("sha256:{}", sha256_hex(BODY).to_ascii_uppercase());

        let (result, reports) = download(&asset(url, Some(digest)), &dir).await;

        let path = result.unwrap();
        assert_eq!(path, dir.join(NAME));
        assert_eq!(std::fs::read(&path).unwrap(), BODY);
        assert_eq!(files_in(&dir), [NAME]);
        let total = Some(BODY.len() as u64);
        assert_eq!(
            reports.first(),
            Some(&DownloadProgress { received: 0, total })
        );
        assert_eq!(
            reports.last(),
            Some(&DownloadProgress {
                received: BODY.len() as u64,
                total
            })
        );
        let request = server.await.unwrap().to_ascii_lowercase();
        assert!(request.starts_with(&format!("get /download/{}", NAME.to_ascii_lowercase())));
        assert!(request.contains("user-agent: delta/"), "{request}");
    }

    #[tokio::test]
    async fn a_mismatching_digest_leaves_no_file() {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("updates");
        let (url, server) = serve_raw("/a.deb", ok_response(BODY)).await;
        let expected = sha256_hex(b"another file");

        let (result, _) = download(&asset(url, Some(format!("sha256:{expected}"))), &dir).await;

        let err = result.unwrap_err();
        assert!(
            matches!(
                &err,
                AssetDownloadError::DigestMismatch { expected: e, actual }
                    if *e == expected && *actual == sha256_hex(BODY)
            ),
            "{err:?}"
        );
        assert_eq!(files_in(&dir), Vec::<String>::new());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn an_asset_without_a_sha256_digest_is_never_requested() {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("updates");
        // Nothing listens here: a request would fail as `Request`, not
        // `NoDigest`.
        let port = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let url = format!("http://127.0.0.1:{port}/a.deb");
        for digest in [
            None,
            Some(String::new()),
            Some(sha256_hex(BODY)),
            Some(format!("sha512:{}", sha256_hex(BODY))),
            Some("sha256:abc".to_owned()),
            Some(format!("sha256:{}", "z".repeat(64))),
        ] {
            let (result, reports) = download(&asset(url.clone(), digest.clone()), &dir).await;
            let err = result.unwrap_err();
            assert!(
                matches!(&err, AssetDownloadError::NoDigest(name) if name == NAME),
                "{digest:?}: {err:?}"
            );
            assert!(reports.is_empty());
            assert_eq!(files_in(&dir), Vec::<String>::new());
        }
    }

    #[tokio::test]
    async fn a_non_2xx_answer_is_reported_with_its_status_and_leaves_no_file() {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("updates");
        let body = "Not Found";
        let response = format!(
            "HTTP/1.1 404 Not Found\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let (url, server) = serve_raw("/a.deb", response.into_bytes()).await;

        let (result, _) = download(
            &asset(url, Some(format!("sha256:{}", sha256_hex(BODY)))),
            &dir,
        )
        .await;

        let err = result.unwrap_err();
        assert!(matches!(err, AssetDownloadError::Status(404)), "{err:?}");
        assert_eq!(files_in(&dir), Vec::<String>::new());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_transfer_cut_short_leaves_no_file() {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("updates");
        // Promise more than is sent, then hang up.
        let mut response = format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            BODY.len() * 4
        )
        .into_bytes();
        response.extend_from_slice(BODY);
        let (url, server) = serve_raw("/a.deb", response).await;

        let (result, _) = download(
            &asset(url, Some(format!("sha256:{}", sha256_hex(BODY)))),
            &dir,
        )
        .await;

        let err = result.unwrap_err();
        assert!(
            matches!(
                err,
                AssetDownloadError::Request(_) | AssetDownloadError::Truncated { .. }
            ),
            "{err:?}"
        );
        assert_eq!(files_in(&dir), Vec::<String>::new());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_completed_download_removes_other_versions() {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("updates");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("delta-desktop_0.5.0_amd64.deb"), b"old").unwrap();
        std::fs::write(dir.join("delta-desktop_0.5.1_amd64.deb.part"), b"partial").unwrap();
        let (url, server) = serve_raw("/a.deb", ok_response(BODY)).await;

        let (result, _) = download(
            &asset(url, Some(format!("sha256:{}", sha256_hex(BODY)))),
            &dir,
        )
        .await;

        result.unwrap();
        assert_eq!(files_in(&dir), [NAME]);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_name_that_is_not_a_plain_file_name_is_refused() {
        let data = tempfile::tempdir().unwrap();
        for name in ["../escape.deb", "a/b.deb", "..", ""] {
            let asset = ReleaseAsset {
                name: name.into(),
                download_url: "http://127.0.0.1:9/a.deb".into(),
                digest: Some(format!("sha256:{}", sha256_hex(BODY))),
            };
            let (result, _) = download(&asset, data.path()).await;
            assert!(
                matches!(result, Err(AssetDownloadError::BadName(ref n)) if n == name),
                "{name}: {result:?}"
            );
        }
    }
}
