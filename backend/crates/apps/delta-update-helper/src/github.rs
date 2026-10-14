//! The helper's own requests to GitHub: the release a tag names, and the
//! release's signature of its `.deb`.

use std::sync::Arc;
use std::time::Duration;

use rustls_platform_verifier::BuilderVerifierExt;

use crate::release_digest::release_url;
use crate::Refusal;

/// How long each request may take, connecting included.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The `User-Agent` the requests send (GitHub rejects requests without one).
const USER_AGENT: &str = concat!("delta-update-helper/", env!("CARGO_PKG_VERSION"));

type Cause = Box<dyn std::error::Error + Send + Sync>;

/// An HTTPS client set up the way the release feed sets up its own: rustls
/// with the `ring` provider, verifying against the operating system's trust
/// store, sending no token. It follows the redirect GitHub answers a
/// release download with, to its asset host.
pub struct Github {
    runtime: tokio::runtime::Runtime,
    client: reqwest::Client,
}

impl Github {
    /// Set up the client, for release `tag` (which only names what failed).
    pub fn new(tag: &str) -> Result<Self, Refusal> {
        let unavailable = |what: &'static str, source: Cause| Refusal::ReleaseUnavailable {
            tag: tag.to_owned(),
            what,
            source,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| unavailable("could not start the async runtime", err.into()))?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .and_then(|builder| builder.with_platform_verifier())
            .map_err(|err| unavailable("could not set up TLS", err.into()))?
            .with_no_client_auth();
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|err| unavailable("could not build the HTTPS client", err.into()))?;
        Ok(Self { runtime, client })
    }

    /// Release `tag`'s object from GitHub's API.
    pub fn release(&self, tag: &str) -> Result<Vec<u8>, Refusal> {
        let unavailable = |what: &'static str, source: Cause| Refusal::ReleaseUnavailable {
            tag: tag.to_owned(),
            what,
            source,
        };
        self.runtime.block_on(async {
            let response = self
                .client
                .get(release_url(tag))
                .header(reqwest::header::USER_AGENT, USER_AGENT)
                .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                .send()
                .await
                .map_err(|err| unavailable("the request failed", err.into()))?;
            let status = response.status();
            if !status.is_success() {
                return Err(Refusal::ReleaseRefused {
                    tag: tag.to_owned(),
                    status,
                });
            }
            let body = response
                .bytes()
                .await
                .map_err(|err| unavailable("the answer could not be read", err.into()))?;
            Ok(body.to_vec())
        })
    }

    /// The signature file at `url`, release `tag`'s own signature of its
    /// `.deb` (`release_digest::signature_url_in_release` checked where it
    /// is), read up to [`release_signature::MAX_SIGNATURE_BYTES`] and one
    /// byte more, so a larger file is refused when it is checked.
    pub fn signature(&self, tag: &str, url: &str) -> Result<Vec<u8>, Refusal> {
        let unavailable = |what: &'static str, source: Cause| Refusal::SignatureUnavailable {
            tag: tag.to_owned(),
            what,
            source,
        };
        self.runtime.block_on(async {
            let mut response = self
                .client
                .get(url)
                .header(reqwest::header::USER_AGENT, USER_AGENT)
                .send()
                .await
                .map_err(|err| unavailable("the request failed", err.into()))?;
            let status = response.status();
            if !status.is_success() {
                return Err(Refusal::SignatureRefused {
                    tag: tag.to_owned(),
                    status,
                });
            }
            let mut text = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|err| unavailable("the answer could not be read", err.into()))?
            {
                text.extend_from_slice(&chunk);
                if text.len() > release_signature::MAX_SIGNATURE_BYTES {
                    break;
                }
            }
            Ok(text)
        })
    }
}
