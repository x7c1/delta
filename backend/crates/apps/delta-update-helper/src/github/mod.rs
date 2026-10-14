//! The helper's own requests to GitHub: the release a tag names, and the
//! release's signature of its `.deb`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use rustls::RootCertStore;

use crate::release_digest::release_url;
use crate::Refusal;

/// How long each request may take, connecting included.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How many redirects a request follows, as many as `reqwest`'s default.
const MAX_REDIRECTS: usize = 10;

/// The `User-Agent` the requests send (GitHub rejects requests without one).
const USER_AGENT: &str = concat!("delta-update-helper/", env!("CARGO_PKG_VERSION"));

/// The system's CA certificates, as one PEM bundle where Debian and Ubuntu
/// keep it (`update-ca-certificates` writes it): the only certificates the
/// helper trusts. `SSL_CERT_FILE` and `SSL_CERT_DIR` are not read.
pub const SYSTEM_CA_FILE: &str = "/etc/ssl/certs/ca-certificates.crt";

type Cause = Box<dyn std::error::Error + Send + Sync>;

/// Which URLs the client may be asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schemes {
    /// `https://` only, for the URL asked and every redirect: what the
    /// helper uses.
    HttpsOnly,
    /// Also `http://` for the URL asked, never for a redirect: for the
    /// tests, whose servers speak plain HTTP on a loopback port.
    #[cfg(test)]
    PlainFirstHop,
}

/// An HTTPS client of the helper's own: rustls with the `ring` provider,
/// sending no token. Nothing about it comes from the environment, which is
/// root's but was set up by whoever started the helper:
///
/// - it trusts only the certificates in [`SYSTEM_CA_FILE`], never a file or
///   directory `SSL_CERT_FILE` or `SSL_CERT_DIR` names;
/// - it uses no proxy, whatever `HTTPS_PROXY`, `ALL_PROXY` and the like say;
/// - it asks only for `https://` URLs and follows the redirect GitHub
///   answers a release download with, to its asset host, only to an
///   `https://` URL.
pub struct Github {
    runtime: tokio::runtime::Runtime,
    client: reqwest::Client,
}

impl Github {
    /// Set up the client, for release `tag` (which only names what failed).
    pub fn new(tag: &str) -> Result<Self, Refusal> {
        Self::with(tag, Path::new(SYSTEM_CA_FILE), Schemes::HttpsOnly)
    }

    /// As [`Self::new`], trusting the certificates in `ca_file` and asking
    /// for the URLs `schemes` allows.
    pub fn with(tag: &str, ca_file: &Path, schemes: Schemes) -> Result<Self, Refusal> {
        let unavailable = |what: &'static str, source: Cause| Refusal::ReleaseUnavailable {
            tag: tag.to_owned(),
            what,
            source,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| unavailable("could not start the async runtime", err.into()))?;
        let roots = trust_anchors(ca_file)
            .map_err(|err| unavailable("could not read the system's CA certificates", err))?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|err| unavailable("could not set up TLS", err.into()))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .no_proxy()
            .https_only(schemes == Schemes::HttpsOnly)
            .redirect(https_redirects())
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

/// The certificates in the PEM file `ca_file`, refused when it holds none
/// that parse.
fn trust_anchors(ca_file: &Path) -> Result<RootCertStore, Cause> {
    let read = |err: rustls::pki_types::pem::Error| -> Cause {
        format!("{}: {err}", ca_file.display()).into()
    };
    let certs = CertificateDer::pem_file_iter(ca_file)
        .map_err(read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(read)?;
    let mut roots = RootCertStore::empty();
    roots.add_parsable_certificates(certs);
    if roots.is_empty() {
        return Err(format!("{} holds no CA certificate", ca_file.display()).into());
    }
    Ok(roots)
}

/// Follows up to [`MAX_REDIRECTS`] redirects, each only to an `https://`
/// URL. `https_only` refuses the same redirects; this policy refuses them
/// whatever the first URL was, so the tests can show it over plain HTTP.
fn https_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.url().scheme() != "https" {
            let refused = format!("refused a redirect to {}: not https", attempt.url());
            attempt.error(refused)
        } else if attempt.previous().len() > MAX_REDIRECTS {
            attempt.error(format!("more than {MAX_REDIRECTS} redirects"))
        } else {
            attempt.follow()
        }
    })
}

#[cfg(test)]
mod tests;
