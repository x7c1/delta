//! The sha256 the release itself states for its `.deb`, asked of GitHub.

use std::sync::Arc;
use std::time::Duration;

use rustls_platform_verifier::BuilderVerifierExt;
use semver::Version;
use serde::Deserialize;

use crate::Refusal;

/// Where GitHub answers the release a tag names.
const RELEASE_BY_TAG_PREFIX: &str = "https://api.github.com/repos/x7c1/delta/releases/tags/";

/// How long the request may take, connecting included.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The `User-Agent` the request sends (GitHub rejects requests without one).
const USER_AGENT: &str = concat!("delta-update-helper/", env!("CARGO_PKG_VERSION"));

/// GitHub's API URL for the release `tag` names.
///
/// `tag` comes from [`InstallRequest`](crate::InstallRequest), which accepts
/// only `v<version>` written the way SemVer writes it, so it needs no
/// escaping.
pub fn release_url(tag: &str) -> String {
    format!("{RELEASE_BY_TAG_PREFIX}{tag}")
}

/// The name of the release's `.deb` of `version` for the Debian architecture
/// `deb_arch`, by the release workflow's naming.
pub fn deb_asset_name(version: &Version, deb_arch: &str) -> String {
    format!("delta-desktop_{version}_{deb_arch}.deb")
}

/// The Debian architecture of a Rust target architecture, for the ones
/// Debian names differently.
pub fn deb_arch(arch: &str) -> &str {
    match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

/// The fields of GitHub's release object the helper reads.
#[derive(Deserialize)]
struct Release {
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    /// `sha256:<hex>`; absent or `null` where GitHub has none.
    #[serde(default)]
    digest: Option<String>,
}

/// The sha256 (lowercase hex) release `tag`'s answer `body` states for its
/// asset `asset`.
///
/// Refused when the body is not a release object, when the asset is not in
/// it or states no digest, and when the digest is not `sha256:` followed by
/// 64 hex digits.
pub fn digest_in_release(body: &[u8], tag: &str, asset: &str) -> Result<String, Refusal> {
    let release: Release =
        serde_json::from_slice(body).map_err(|err| Refusal::ReleaseUnavailable {
            tag: tag.to_owned(),
            what: "GitHub's answer is not a release",
            source: Box::new(err),
        })?;
    let missing = || Refusal::DigestMissing {
        tag: tag.to_owned(),
        asset: asset.to_owned(),
    };
    let digest = release
        .assets
        .into_iter()
        .find(|candidate| candidate.name == asset)
        .ok_or_else(missing)?
        .digest
        .ok_or_else(missing)?;
    parse_sha256(&digest).ok_or_else(|| Refusal::DigestMalformed {
        tag: tag.to_owned(),
        asset: asset.to_owned(),
        digest,
    })
}

/// The hex of a `sha256:<64 hex digits>` digest, lowercased, or `None` for
/// anything else.
pub fn parse_sha256(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
}

/// Ask GitHub for release `tag` and return the sha256 it states for `asset`.
pub fn fetch_digest(tag: &str, asset: &str) -> Result<String, Refusal> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| Refusal::ReleaseUnavailable {
            tag: tag.to_owned(),
            what: "could not start the async runtime",
            source: Box::new(err),
        })?;
    let body = runtime.block_on(fetch_release(tag))?;
    digest_in_release(&body, tag, asset)
}

/// Release `tag`'s object from GitHub's API, asked the way the release feed
/// asks: rustls with the `ring` provider, verifying against the operating
/// system's trust store, with no token.
async fn fetch_release(tag: &str) -> Result<Vec<u8>, Refusal> {
    let unavailable = |what: &'static str, source: Box<dyn std::error::Error + Send + Sync>| {
        Refusal::ReleaseUnavailable {
            tag: tag.to_owned(),
            what,
            source,
        }
    };
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
    let response = client
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
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG: &str = "v0.6.0";
    const DEB: &str = "delta-desktop_0.6.0_amd64.deb";

    fn hex_of(c: char) -> String {
        c.to_string().repeat(64)
    }

    #[test]
    fn the_url_asks_for_the_release_by_its_tag() {
        assert_eq!(
            release_url(TAG),
            "https://api.github.com/repos/x7c1/delta/releases/tags/v0.6.0"
        );
    }

    #[test]
    fn the_asset_is_the_release_workflows_deb_for_this_architecture() {
        let version = Version::new(0, 6, 0);
        assert_eq!(deb_asset_name(&version, deb_arch("x86_64")), DEB);
        assert_eq!(
            deb_asset_name(&version, deb_arch("aarch64")),
            "delta-desktop_0.6.0_arm64.deb"
        );
    }

    #[test]
    fn the_digest_is_read_for_the_right_asset() {
        // Trimmed from a release's answer: another asset first, with its own
        // digest, and fields the helper does not read.
        let body = format!(
            r#"{{
                "tag_name": "v0.6.0",
                "assets": [
                    {{"name": "Delta_0.6.0_aarch64.dmg", "digest": "sha256:{a}", "size": 1}},
                    {{"name": "{DEB}", "digest": "sha256:{b}", "size": 2}}
                ]
            }}"#,
            a = hex_of('a'),
            b = hex_of('B'),
        );
        assert_eq!(
            digest_in_release(body.as_bytes(), TAG, DEB).unwrap(),
            hex_of('b')
        );
    }

    #[test]
    fn a_missing_digest_is_refused() {
        for assets in [
            // No asset at all, another asset only, the asset without a digest.
            "[]".to_owned(),
            format!(
                r#"[{{"name": "Delta_0.6.0_aarch64.dmg", "digest": "sha256:{}"}}]"#,
                hex_of('a')
            ),
            format!(r#"[{{"name": "{DEB}"}}]"#),
            format!(r#"[{{"name": "{DEB}", "digest": null}}]"#),
        ] {
            let body = format!(r#"{{"tag_name": "v0.6.0", "assets": {assets}}}"#);
            let err = digest_in_release(body.as_bytes(), TAG, DEB).unwrap_err();
            assert!(
                matches!(&err, Refusal::DigestMissing { asset, .. } if asset == DEB),
                "{assets}: {err:?}"
            );
        }
    }

    #[test]
    fn a_malformed_digest_is_refused() {
        for digest in [
            String::new(),
            hex_of('a'),
            format!("sha512:{}", hex_of('a')),
            format!("sha256:{}", "a".repeat(63)),
            format!("sha256:{}", "a".repeat(65)),
            format!("sha256:{}g", "a".repeat(63)),
            format!("SHA256:{}", hex_of('a')),
        ] {
            let body = format!(r#"{{"assets": [{{"name": "{DEB}", "digest": "{digest}"}}]}}"#);
            let err = digest_in_release(body.as_bytes(), TAG, DEB).unwrap_err();
            assert!(
                matches!(&err, Refusal::DigestMalformed { digest: read, .. } if *read == digest),
                "{digest}: {err:?}"
            );
        }
    }

    #[test]
    fn an_answer_that_is_not_a_release_is_refused() {
        for body in ["", "null", "not json", r#"{"assets": "none"}"#] {
            let err = digest_in_release(body.as_bytes(), TAG, DEB).unwrap_err();
            assert!(
                matches!(err, Refusal::ReleaseUnavailable { .. }),
                "{body}: {err:?}"
            );
        }
    }
}
