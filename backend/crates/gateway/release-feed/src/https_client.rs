//! The HTTPS client both the feed and the downloader are built from.

use std::sync::Arc;

use rustls_platform_verifier::BuilderVerifierExt;

use crate::ClientBuildError;

/// The `User-Agent` every request sends (GitHub rejects requests without one).
pub(crate) const USER_AGENT_VALUE: &str = concat!("delta/", env!("CARGO_PKG_VERSION"));

/// A client builder over rustls with the `ring` provider, verifying
/// certificates against the operating system's trust store. Each caller adds
/// its own timeouts.
pub(crate) fn https_client_builder() -> Result<reqwest::ClientBuilder, ClientBuildError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()?
        .with_no_client_auth();
    Ok(reqwest::Client::builder().tls_backend_preconfigured(tls))
}
