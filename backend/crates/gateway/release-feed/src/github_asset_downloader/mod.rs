use std::path::{Path, PathBuf};

use async_trait::async_trait;
use reqwest::header::USER_AGENT;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use delta_usecase::{AssetDownloadError, AssetDownloader, DownloadProgress, SignedAsset};
use release_signature::{
    signature_name, BundleCheck, BundleSignature, ReleaseKey, SignatureError, MAX_SIGNATURE_BYTES,
};

use crate::https_client::{https_client_builder, Schemes, USER_AGENT_VALUE};
use crate::{ClientBuildError, DOWNLOAD_CONNECT_TIMEOUT, DOWNLOAD_STALL_TIMEOUT};

mod local_file;
use local_file::{exists, read_regular_file, remove_logged, remove_others, sha256_of_regular_file};

/// The mode the download directory is created with when it is missing.
const UPDATES_DIR_MODE: u32 = 0o700;

#[cfg(test)]
mod tests;

/// What a file is called while it downloads: its final name plus this.
const TEMP_SUFFIX: &str = ".part";

/// The [`AssetDownloader`] for GitHub release assets.
///
/// Follows the redirect GitHub answers a `browser_download_url` with (to its
/// asset host), only to an `https://` URL, and refuses an `http://` URL
/// outright; whether the URL it is handed may be fetched at all is the
/// caller's decision. Checks every asset against its signature with the
/// release key ([`ReleaseKey::production`]) besides its digest.
#[derive(Debug, Clone)]
pub struct GithubAssetDownloader {
    client: reqwest::Client,
    key: ReleaseKey,
}

impl GithubAssetDownloader {
    /// A downloader giving up when connecting takes longer than
    /// [`DOWNLOAD_CONNECT_TIMEOUT`] or no data arrives for
    /// [`DOWNLOAD_STALL_TIMEOUT`], with no cap on the whole transfer.
    pub fn new() -> Result<Self, ClientBuildError> {
        Self::with(ReleaseKey::production(), Schemes::HttpsOnly)
    }

    /// As [`Self::new`], checking signatures against `key` and fetching the
    /// URLs `schemes` allows: the tests sign with a key of their own and
    /// serve plain HTTP.
    fn with(key: ReleaseKey, schemes: Schemes) -> Result<Self, ClientBuildError> {
        let client = https_client_builder(schemes)?
            .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
            .read_timeout(DOWNLOAD_STALL_TIMEOUT)
            .build()?;
        Ok(Self { client, key })
    }

    /// Fetch the signature file at `url`, refusing one larger than
    /// [`MAX_SIGNATURE_BYTES`] without reading the rest.
    async fn fetch_signature(&self, url: &str) -> Result<Vec<u8>, AssetDownloadError> {
        let request = |err: reqwest::Error| AssetDownloadError::SignatureRequest(err.into());
        let mut response = self
            .client
            .get(url)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .send()
            .await
            .map_err(request)?;
        let status = response.status();
        if !status.is_success() {
            return Err(AssetDownloadError::SignatureStatus(status.as_u16()));
        }
        let mut text = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(request)? {
            text.extend_from_slice(&chunk);
            if text.len() > MAX_SIGNATURE_BYTES {
                return Err(AssetDownloadError::SignatureRejected(
                    SignatureError::TooLarge {
                        limit: MAX_SIGNATURE_BYTES,
                    },
                ));
            }
        }
        Ok(text)
    }

    /// Stream `url` into a new file at `temp`, feeding it to `check` and
    /// reporting progress, and return the sha256 of what arrived as
    /// lowercase hex.
    async fn fetch(
        &self,
        url: &str,
        temp: &Path,
        check: &mut BundleCheck<'_>,
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
            check.update(&chunk);
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

    /// Fetch `signed`'s signature and asset, the asset into `temp`, and
    /// verify the asset against its digest `expected` and the signature;
    /// then write the signature to `signature` and move the asset to
    /// `target`. Leaves whatever it wrote for the caller to remove on
    /// failure.
    async fn fetch_verified(
        &self,
        signed: &SignedAsset,
        expected: &str,
        [temp, signature, target]: [&Path; 3],
        progress: &(dyn Fn(DownloadProgress) + Send + Sync),
    ) -> Result<(), AssetDownloadError> {
        let asset = &signed.asset;
        // The signature comes first: one made with another key, or naming
        // another file or version, is refused before the asset is fetched.
        let text = self.fetch_signature(&signed.signature_url).await?;
        let parsed = BundleSignature::parse(&self.key, &text, &asset.name, &signed.version)
            .map_err(AssetDownloadError::SignatureRejected)?;
        let mut check = parsed
            .check()
            .map_err(AssetDownloadError::SignatureRejected)?;
        let actual = self
            .fetch(&asset.download_url, temp, &mut check, progress)
            .await?;
        if actual != expected {
            return Err(AssetDownloadError::DigestMismatch {
                expected: expected.to_owned(),
                actual,
            });
        }
        check
            .finish()
            .map_err(AssetDownloadError::SignatureRejected)?;
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| AssetDownloadError::Io { path, source }
        };
        tokio::fs::write(signature, &text)
            .await
            .map_err(io(signature))?;
        tokio::fs::rename(temp, target).await.map_err(io(target))
    }
}

/// The asset's sha256 as lowercase hex, refusing an asset without one or
/// whose name has no place in a directory.
fn checked_asset(signed: &SignedAsset) -> Result<String, AssetDownloadError> {
    let asset = &signed.asset;
    let expected = asset
        .sha256()
        .ok_or_else(|| AssetDownloadError::NoDigest(asset.name.clone()))?;
    if !is_plain_file_name(&asset.name) {
        return Err(AssetDownloadError::BadName(asset.name.clone()));
    }
    Ok(expected)
}

#[async_trait]
impl AssetDownloader for GithubAssetDownloader {
    async fn download(
        &self,
        signed: &SignedAsset,
        dir: &Path,
        progress: &(dyn Fn(DownloadProgress) + Send + Sync),
    ) -> Result<PathBuf, AssetDownloadError> {
        let expected = checked_asset(signed)?;
        let name = &signed.asset.name;
        // Owner-only, like the data directory it sits in: a mode left to the
        // umask would let another local user list what was downloaded.
        tokio::fs::DirBuilder::new()
            .recursive(true)
            .mode(UPDATES_DIR_MODE)
            .create(dir)
            .await
            .map_err(|source| AssetDownloadError::Io {
                path: dir.to_path_buf(),
                source,
            })?;
        let signature_file = signature_name(name);
        let target = dir.join(name);
        let signature = dir.join(&signature_file);
        let temp = dir.join(format!("{name}{TEMP_SUFFIX}"));

        let verified = self
            .fetch_verified(signed, &expected, [&temp, &signature, &target], progress)
            .await;
        if let Err(err) = verified {
            remove_logged(&temp, "could not remove a partial download").await;
            remove_logged(
                &signature,
                "could not remove a refused download's signature",
            )
            .await;
            return Err(err);
        }
        remove_others(dir, &[name, &signature_file]).await;
        Ok(target)
    }

    async fn reverify(
        &self,
        signed: &SignedAsset,
        dir: &Path,
    ) -> Result<Option<PathBuf>, AssetDownloadError> {
        let expected = checked_asset(signed)?;
        let name = &signed.asset.name;
        let signature_file = signature_name(name);
        let target = dir.join(name);
        let signature = dir.join(&signature_file);
        let verified = self
            .reverify_files(signed, &expected, &target, &signature)
            .await;
        if !matches!(verified, Ok(Some(_))) {
            for path in [&target, &signature] {
                remove_logged(
                    path,
                    "could not remove a download that failed its check again",
                )
                .await;
            }
        }
        remove_others(dir, &[name, &signature_file]).await;
        verified
    }
}

impl GithubAssetDownloader {
    /// Check the asset left at `target` against its digest `expected` and
    /// the signature left at `signature`, or `None` when no asset is there.
    async fn reverify_files(
        &self,
        signed: &SignedAsset,
        expected: &str,
        target: &Path,
        signature: &Path,
    ) -> Result<Option<PathBuf>, AssetDownloadError> {
        let rejected = AssetDownloadError::SignatureRejected;
        let Some(text) = read_regular_file(signature, MAX_SIGNATURE_BYTES).await? else {
            return if exists(target).await? {
                Err(rejected(SignatureError::Missing {
                    bundle: signed.asset.name.clone(),
                }))
            } else {
                Ok(None)
            };
        };
        let parsed = BundleSignature::parse(&self.key, &text, &signed.asset.name, &signed.version)
            .map_err(rejected)?;
        let mut check = parsed.check().map_err(rejected)?;
        let Some(actual) = sha256_of_regular_file(target, &mut |chunk| check.update(chunk)).await?
        else {
            return Ok(None);
        };
        if actual != expected {
            return Err(AssetDownloadError::DigestMismatch {
                expected: expected.to_owned(),
                actual,
            });
        }
        check.finish().map_err(rejected)?;
        Ok(Some(target.to_path_buf()))
    }
}

/// Whether `name` names a file directly in a directory.
fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
