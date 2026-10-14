//! The HTTPS client both the feed and the downloader are built from.

use std::sync::Arc;

use rustls_platform_verifier::BuilderVerifierExt;

use crate::ClientBuildError;

/// The `User-Agent` every request sends (GitHub rejects requests without one).
pub(crate) const USER_AGENT_VALUE: &str = concat!("delta/", env!("CARGO_PKG_VERSION"));

/// How many redirects a request follows, as many as `reqwest`'s default.
const MAX_REDIRECTS: usize = 10;

/// Which URLs a client may be asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Schemes {
    /// `https://` only, for the URL asked and every redirect: what the app
    /// uses.
    HttpsOnly,
    /// Also `http://` for the URL asked, never for a redirect: for the
    /// tests, whose servers speak plain HTTP on a loopback port.
    #[cfg(test)]
    PlainFirstHop,
}

/// A client builder over rustls with the `ring` provider, verifying
/// certificates against the operating system's trust store, that refuses an
/// `http://` URL and a redirect to one ([`Schemes::HttpsOnly`]): a redirect
/// to plain HTTP would hand the download, or the feed's answer, to anyone on
/// the path. Each caller adds its own timeouts.
pub(crate) fn https_client_builder(
    schemes: Schemes,
) -> Result<reqwest::ClientBuilder, ClientBuildError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()?
        .with_no_client_auth();
    Ok(reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .https_only(schemes == Schemes::HttpsOnly)
        .redirect(https_redirects()))
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
pub(crate) mod tests {
    use crate::test_server::serve_raw;

    use super::*;

    /// A raw response redirecting to `location`.
    pub(crate) fn redirect_to(location: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 302 Found\r\nlocation: {location}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        )
        .into_bytes()
    }

    /// Whether `err`, or one of its sources, is a `reqwest` redirect error.
    pub(crate) fn is_refused_redirect(err: &(dyn std::error::Error + 'static)) -> bool {
        let mut source = Some(err);
        while let Some(err) = source {
            if err
                .downcast_ref::<reqwest::Error>()
                .is_some_and(reqwest::Error::is_redirect)
            {
                return true;
            }
            source = err.source();
        }
        false
    }

    #[tokio::test]
    async fn a_redirect_to_plain_http_is_refused() {
        let (url, server) = serve_raw("/asset", redirect_to("http://127.0.0.1:9/next")).await;
        let client = https_client_builder(Schemes::PlainFirstHop)
            .unwrap()
            .build()
            .unwrap();
        let err = client.get(&url).send().await.unwrap_err();
        assert!(err.is_redirect(), "{err:?}");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_plain_http_url_is_refused_before_any_request() {
        let client = https_client_builder(Schemes::HttpsOnly)
            .unwrap()
            .build()
            .unwrap();
        // Nothing listens on port 9: the refusal comes before connecting.
        let err = client
            .get("http://127.0.0.1:9/latest")
            .send()
            .await
            .unwrap_err();
        // `reqwest` refuses the URL's scheme as a request it cannot build.
        assert!(err.is_builder(), "{err:?}");
    }
}
