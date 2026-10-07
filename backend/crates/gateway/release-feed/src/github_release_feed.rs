use async_trait::async_trait;
use reqwest::header::{ACCEPT, USER_AGENT};
use serde::Deserialize;

use delta_usecase::{PublishedRelease, ReleaseAsset, ReleaseFeed, ReleaseFeedError};

use crate::https_client::{https_client_builder, USER_AGENT_VALUE};
use crate::{ClientBuildError, REQUEST_TIMEOUT};

/// The media type GitHub's REST API documents for its JSON answers.
const GITHUB_JSON: &str = "application/vnd.github+json";

/// The [`ReleaseFeed`] backed by GitHub's releases API.
#[derive(Debug, Clone)]
pub struct GithubReleaseFeed {
    client: reqwest::Client,
    url: String,
}

impl GithubReleaseFeed {
    /// A feed asking `url` (normally [`LATEST_RELEASE_URL`](crate::LATEST_RELEASE_URL)).
    pub fn new(url: impl Into<String>) -> Result<Self, ClientBuildError> {
        let client = https_client_builder()?.timeout(REQUEST_TIMEOUT).build()?;
        Ok(Self {
            client,
            url: url.into(),
        })
    }
}

#[async_trait]
impl ReleaseFeed for GithubReleaseFeed {
    async fn latest_release(&self) -> Result<PublishedRelease, ReleaseFeedError> {
        let response = self
            .client
            .get(&self.url)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .header(ACCEPT, GITHUB_JSON)
            .send()
            .await
            .map_err(|err| ReleaseFeedError::Request(err.into()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(ReleaseFeedError::Status(status.as_u16()));
        }
        let body = response
            .bytes()
            .await
            .map_err(|err| ReleaseFeedError::Request(err.into()))?;
        parse_release(&body)
    }

    fn url(&self) -> &str {
        &self.url
    }
}

/// The fields of GitHub's release object the check reads.
#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    /// Absent reads as a release with no assets.
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

/// The fields of one of a release's assets the update reads.
#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    /// `sha256:<hex>`; absent or `null` on assets GitHub has no digest for.
    #[serde(default)]
    digest: Option<String>,
}

/// Parse a release object out of a 2xx body.
fn parse_release(body: &[u8]) -> Result<PublishedRelease, ReleaseFeedError> {
    let release: GithubRelease =
        serde_json::from_slice(body).map_err(|err| ReleaseFeedError::Malformed(err.into()))?;
    Ok(PublishedRelease {
        tag_name: release.tag_name,
        html_url: release.html_url,
        assets: release
            .assets
            .into_iter()
            .map(|asset| ReleaseAsset {
                name: asset.name,
                download_url: asset.browser_download_url,
                digest: asset.digest,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;
    use crate::test_server::serve_once;

    #[tokio::test]
    async fn a_release_is_read_with_the_headers_github_requires() {
        let (url, server) = serve_once(
            "200 OK",
            r#"{"tag_name":"v0.6.0","html_url":"https://github.com/x7c1/delta/releases/tag/v0.6.0","draft":false}"#,
        )
        .await;
        let release = GithubReleaseFeed::new(url)
            .unwrap()
            .latest_release()
            .await
            .unwrap();
        assert_eq!(
            release,
            PublishedRelease {
                tag_name: "v0.6.0".into(),
                html_url: "https://github.com/x7c1/delta/releases/tag/v0.6.0".into(),
                assets: Vec::new(),
            }
        );
        let request = server.await.unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /repos/x7c1/delta/releases/latest "));
        assert!(request.contains("user-agent: delta/"), "{request}");
        assert!(
            request.contains("accept: application/vnd.github+json"),
            "{request}"
        );
        assert!(!request.contains("authorization:"), "{request}");
    }

    #[test]
    fn each_assets_name_download_url_and_digest_are_read() {
        // Trimmed from v0.5.0's answer: the fields read, beside some that are not.
        let body = r#"{
            "tag_name": "v0.5.0",
            "html_url": "https://github.com/x7c1/delta/releases/tag/v0.5.0",
            "assets": [
                {
                    "name": "Delta_0.5.0_aarch64.dmg",
                    "browser_download_url": "https://github.com/x7c1/delta/releases/download/v0.5.0/Delta_0.5.0_aarch64.dmg",
                    "digest": "sha256:aaaa",
                    "size": 10
                },
                {
                    "name": "delta-desktop_0.5.0_amd64.deb",
                    "browser_download_url": "https://github.com/x7c1/delta/releases/download/v0.5.0/delta-desktop_0.5.0_amd64.deb",
                    "digest": "sha256:bbbb",
                    "content_type": "application/vnd.debian.binary-package"
                }
            ]
        }"#;
        let release = parse_release(body.as_bytes()).unwrap();
        assert_eq!(
            release.assets,
            vec![
                ReleaseAsset {
                    name: "Delta_0.5.0_aarch64.dmg".into(),
                    download_url: "https://github.com/x7c1/delta/releases/download/v0.5.0/Delta_0.5.0_aarch64.dmg".into(),
                    digest: Some("sha256:aaaa".into()),
                },
                ReleaseAsset {
                    name: "delta-desktop_0.5.0_amd64.deb".into(),
                    download_url: "https://github.com/x7c1/delta/releases/download/v0.5.0/delta-desktop_0.5.0_amd64.deb".into(),
                    digest: Some("sha256:bbbb".into()),
                },
            ]
        );
    }

    #[test]
    fn a_release_without_assets_has_none() {
        for body in [
            r#"{"tag_name":"v0.5.0","html_url":"https://github.com/x7c1/delta/releases/tag/v0.5.0"}"#,
            r#"{"tag_name":"v0.5.0","html_url":"https://github.com/x7c1/delta/releases/tag/v0.5.0","assets":[]}"#,
        ] {
            assert_eq!(parse_release(body.as_bytes()).unwrap().assets, vec![]);
        }
    }

    #[test]
    fn an_asset_without_a_digest_is_read_with_none() {
        for asset in [
            r#"{"name":"a.deb","browser_download_url":"https://github.com/x7c1/delta/releases/download/v0.5.0/a.deb"}"#,
            r#"{"name":"a.deb","browser_download_url":"https://github.com/x7c1/delta/releases/download/v0.5.0/a.deb","digest":null}"#,
        ] {
            let body = format!(
                r#"{{"tag_name":"v0.5.0","html_url":"https://github.com/x7c1/delta/releases/tag/v0.5.0","assets":[{asset}]}}"#
            );
            let release = parse_release(body.as_bytes()).unwrap();
            assert_eq!(release.assets.len(), 1);
            assert_eq!(release.assets[0].name, "a.deb");
            assert_eq!(release.assets[0].digest, None);
        }
    }

    #[test]
    fn an_asset_missing_its_name_or_url_is_malformed() {
        for asset in [
            r#"{"browser_download_url":"https://github.com/x7c1/delta/releases/download/v0.5.0/a.deb"}"#,
            r#"{"name":"a.deb"}"#,
        ] {
            let body = format!(
                r#"{{"tag_name":"v0.5.0","html_url":"https://github.com/x7c1/delta/releases/tag/v0.5.0","assets":[{asset}]}}"#
            );
            let err = parse_release(body.as_bytes()).unwrap_err();
            assert!(matches!(err, ReleaseFeedError::Malformed(_)), "{err:?}");
        }
    }

    #[tokio::test]
    async fn a_non_2xx_status_is_reported_with_its_code() {
        let (url, server) =
            serve_once("403 Forbidden", r#"{"message":"API rate limit exceeded"}"#).await;
        let err = GithubReleaseFeed::new(url)
            .unwrap()
            .latest_release()
            .await
            .unwrap_err();
        assert!(matches!(err, ReleaseFeedError::Status(403)), "{err:?}");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_body_that_is_not_a_release_is_malformed() {
        for body in ["not json", r#"{"tag_name":"v0.6.0"}"#, "[]"] {
            let (url, server) = serve_once("200 OK", body).await;
            let err = GithubReleaseFeed::new(url)
                .unwrap()
                .latest_release()
                .await
                .unwrap_err();
            assert!(
                matches!(&err, ReleaseFeedError::Malformed(cause) if cause.is::<serde_json::Error>()),
                "{body}: {err:?}"
            );
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn an_unreachable_feed_is_a_request_failure() {
        // Bind, read the port back, and close it: nothing listens there now.
        let port = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let err = GithubReleaseFeed::new(format!("http://127.0.0.1:{port}/latest"))
            .unwrap()
            .latest_release()
            .await
            .unwrap_err();
        assert!(
            matches!(&err, ReleaseFeedError::Request(cause) if cause.is::<reqwest::Error>()),
            "{err:?}"
        );
    }
}
