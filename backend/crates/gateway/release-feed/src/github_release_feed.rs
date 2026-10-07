use std::sync::Arc;

use async_trait::async_trait;
use reqwest::header::{ACCEPT, USER_AGENT};
use rustls_platform_verifier::BuilderVerifierExt;
use serde::Deserialize;

use delta_usecase::{PublishedRelease, ReleaseFeed, ReleaseFeedError};

use crate::{ClientBuildError, REQUEST_TIMEOUT};

/// The `User-Agent` every request sends.
const USER_AGENT_VALUE: &str = concat!("delta/", env!("CARGO_PKG_VERSION"));

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
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .with_platform_verifier()?
            .with_no_client_auth();
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .timeout(REQUEST_TIMEOUT)
            .build()?;
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
}

/// The fields of GitHub's release object the check reads.
#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

/// Parse a release object out of a 2xx body.
fn parse_release(body: &[u8]) -> Result<PublishedRelease, ReleaseFeedError> {
    let release: GithubRelease =
        serde_json::from_slice(body).map_err(|err| ReleaseFeedError::Malformed(err.into()))?;
    Ok(PublishedRelease {
        tag_name: release.tag_name,
        html_url: release.html_url,
    })
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    use super::*;

    /// Serve one plain-HTTP request on a loopback port with `status` and
    /// `body`, handing back the raw request it received.
    async fn serve_once(status: &'static str, body: &'static str) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/repos/x7c1/delta/releases/latest",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0, "the client hung up mid-request");
                request.extend_from_slice(&buf[..n]);
            }
            let response = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
            String::from_utf8(request).unwrap()
        });
        (url, server)
    }

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
