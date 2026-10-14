//! What the release itself states for its `.deb`, read from GitHub's answer
//! for it: the sha256 digest, and where its signature is.

use semver::Version;
use serde::Deserialize;

use crate::Refusal;

/// Where GitHub answers the release a tag names.
const RELEASE_BY_TAG_PREFIX: &str = "https://api.github.com/repos/x7c1/delta/releases/tags/";

/// The only place a release's files are downloaded from: Delta's own GitHub
/// Releases, as the desktop app pins it (`RELEASE_DOWNLOAD_PREFIX` in
/// `delta-usecase`).
const RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/x7c1/delta/releases/download/";

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
    /// Where GitHub serves the file; absent or `null` is read as nowhere.
    #[serde(default)]
    browser_download_url: Option<String>,
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
    let release = parse_release(body, tag)?;
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

/// Where release `tag`'s answer `body` says the signature of its asset
/// `asset`, `<asset>.minisig`, is downloaded from.
///
/// Refused when the body is not a release object, when the release carries
/// no such signature, and when its URL is not exactly the release's own
/// download URL for it, under [`RELEASE_DOWNLOAD_PREFIX`]: the helper fetches
/// nothing from anywhere else.
pub fn signature_url_in_release(body: &[u8], tag: &str, asset: &str) -> Result<String, Refusal> {
    let release = parse_release(body, tag)?;
    let name = release_signature::signature_name(asset);
    let url = release
        .assets
        .into_iter()
        .find(|candidate| candidate.name == name)
        .ok_or_else(|| Refusal::SignatureMissing {
            tag: tag.to_owned(),
            asset: asset.to_owned(),
        })?
        .browser_download_url
        .unwrap_or_default();
    if url != format!("{RELEASE_DOWNLOAD_PREFIX}{tag}/{name}") {
        return Err(Refusal::SignatureUntrusted {
            tag: tag.to_owned(),
            url,
        });
    }
    Ok(url)
}

fn parse_release(body: &[u8], tag: &str) -> Result<Release, Refusal> {
    serde_json::from_slice(body).map_err(|err| Refusal::ReleaseUnavailable {
        tag: tag.to_owned(),
        what: "GitHub's answer is not a release",
        source: Box::new(err),
    })
}

/// The hex of a `sha256:<64 hex digits>` digest, lowercased, or `None` for
/// anything else.
pub fn parse_sha256(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
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

    #[test]
    fn the_signature_is_the_releases_own_asset_for_the_deb() {
        let body = format!(
            r#"{{"assets": [
                {{"name": "Delta_0.6.0_aarch64.dmg.minisig", "browser_download_url": "{p}v0.6.0/Delta_0.6.0_aarch64.dmg.minisig"}},
                {{"name": "{DEB}.minisig", "browser_download_url": "{p}v0.6.0/{DEB}.minisig"}}
            ]}}"#,
            p = RELEASE_DOWNLOAD_PREFIX,
        );
        assert_eq!(
            signature_url_in_release(body.as_bytes(), TAG, DEB).unwrap(),
            format!("https://github.com/x7c1/delta/releases/download/v0.6.0/{DEB}.minisig")
        );
    }

    #[test]
    fn a_release_without_the_debs_signature_is_refused() {
        for assets in [
            "[]".to_owned(),
            format!(
                r#"[{{"name": "Delta_0.6.0_aarch64.dmg.minisig", "browser_download_url": "{RELEASE_DOWNLOAD_PREFIX}v0.6.0/Delta_0.6.0_aarch64.dmg.minisig"}}]"#
            ),
            format!(
                r#"[{{"name": "{DEB}", "digest": "sha256:{}"}}]"#,
                hex_of('a')
            ),
        ] {
            let body = format!(r#"{{"assets": {assets}}}"#);
            let err = signature_url_in_release(body.as_bytes(), TAG, DEB).unwrap_err();
            assert!(
                matches!(&err, Refusal::SignatureMissing { asset, .. } if asset == DEB),
                "{assets}: {err:?}"
            );
        }
    }

    #[test]
    fn a_signature_anywhere_but_the_releases_own_url_is_refused() {
        for url in [
            format!("https://example.com/{DEB}.minisig"),
            format!("https://objects.githubusercontent.com/x7c1/delta/releases/download/v0.6.0/{DEB}.minisig"),
            format!("http://github.com/x7c1/delta/releases/download/v0.6.0/{DEB}.minisig"),
            // Another release's signature of the same name.
            format!("{RELEASE_DOWNLOAD_PREFIX}v0.5.0/{DEB}.minisig"),
            format!("{RELEASE_DOWNLOAD_PREFIX}v0.6.0/../v0.5.0/{DEB}.minisig"),
            format!("{RELEASE_DOWNLOAD_PREFIX}v0.6.0/{DEB}.minisig?x=1"),
            String::new(),
        ] {
            let body = format!(
                r#"{{"assets": [{{"name": "{DEB}.minisig", "browser_download_url": "{url}"}}]}}"#
            );
            let err = signature_url_in_release(body.as_bytes(), TAG, DEB).unwrap_err();
            assert!(
                matches!(&err, Refusal::SignatureUntrusted { url: refused, .. } if *refused == url),
                "{url}: {err:?}"
            );
        }
        let body = format!(r#"{{"assets": [{{"name": "{DEB}.minisig"}}]}}"#);
        let err = signature_url_in_release(body.as_bytes(), TAG, DEB).unwrap_err();
        assert!(matches!(err, Refusal::SignatureUntrusted { .. }), "{err:?}");
    }
}
