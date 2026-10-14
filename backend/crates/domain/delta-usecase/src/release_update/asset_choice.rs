//! Which file of a release this platform downloads, and from where it may.

use super::{Platform, UpdateRefusal};
use crate::ports::{ReleaseAsset, SignedAsset};
use crate::NewerRelease;

/// The only place a release asset may be downloaded from: Delta's own GitHub
/// Releases. GitHub redirects these to its asset host; the pin is on the URL
/// the feed gave, not on where it redirects.
pub const RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/x7c1/delta/releases/download/";

/// The asset of `release` that `platform` downloads.
///
/// One asset per platform, by exact name; a platform no release carries, or a
/// release without the platform's asset, is refused rather than guessed.
pub fn choose_asset<'a>(
    platform: &Platform,
    release: &'a NewerRelease,
) -> Result<&'a ReleaseAsset, UpdateRefusal> {
    let name = platform.asset_name(release.version()).ok_or_else(|| {
        UpdateRefusal::UnsupportedPlatform {
            platform: platform.to_string(),
        }
    })?;
    release
        .assets()
        .iter()
        .find(|asset| asset.name == name)
        .ok_or_else(|| UpdateRefusal::MissingAsset {
            version: release.display_version(),
            asset: name,
        })
}

/// The asset of `release` that `platform` may download: the one
/// [`choose_asset`] names, from a URL [`check_download_url`] accepts, stating
/// a sha256 digest to verify it against.
///
/// What the browser is offered Update for, and what a download fetches, so
/// that a release Delta would refuse to download is never offered.
pub fn downloadable_asset<'a>(
    platform: &Platform,
    release: &'a NewerRelease,
) -> Result<&'a ReleaseAsset, UpdateRefusal> {
    let asset = choose_asset(platform, release)?;
    check_download_url(&asset.download_url)?;
    if asset.sha256().is_none() {
        return Err(UpdateRefusal::NoDigest {
            version: release.display_version(),
            asset: asset.name.clone(),
        });
    }
    Ok(asset)
}

/// `asset` of `release` (the one [`downloadable_asset`] picked) with the
/// release's signature of it: the release's asset named
/// `<asset name>.minisig`, from a URL [`check_download_url`] accepts.
///
/// Refused when the release carries no such asset
/// ([`UpdateRefusal::NoSignature`]) or its URL is outside the pinned prefix:
/// the asset cannot be verified, and is never downloaded.
pub fn signed_asset(
    release: &NewerRelease,
    asset: &ReleaseAsset,
) -> Result<SignedAsset, UpdateRefusal> {
    let name = release_signature::signature_name(&asset.name);
    let signature = release
        .assets()
        .iter()
        .find(|candidate| candidate.name == name)
        .ok_or_else(|| UpdateRefusal::NoSignature {
            version: release.display_version(),
            asset: asset.name.clone(),
        })?;
    check_download_url(&signature.download_url)?;
    Ok(SignedAsset {
        asset: asset.clone(),
        signature_url: signature.download_url.clone(),
        version: release.version().clone(),
    })
}

/// Accept `url` only under [`RELEASE_DOWNLOAD_PREFIX`].
///
/// Each path segment after the prefix must be a plain name (letters, digits,
/// `.`, `_`, `-`, `+`, `~`, and not `.` or `..`): a URL parser would otherwise
/// resolve `..`, its percent-encoded form or a backslash into a path outside
/// the prefix the string seems to be under.
pub fn check_download_url(url: &str) -> Result<(), UpdateRefusal> {
    let untrusted = || UpdateRefusal::UntrustedUrl(url.to_owned());
    let rest = url
        .strip_prefix(RELEASE_DOWNLOAD_PREFIX)
        .ok_or_else(untrusted)?;
    let plain = |segment: &str| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | '~'))
    };
    if rest.split('/').all(plain) {
        Ok(())
    } else {
        Err(untrusted())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release_update::testing::{asset, newer_release, signature_of, LINUX_DEB};

    #[test]
    fn linux_x86_64_downloads_the_deb() {
        let release = newer_release(
            "0.6.0",
            vec![
                asset("Delta_0.6.0_aarch64.dmg"),
                asset("delta-desktop_0.6.0_amd64.deb"),
            ],
        );
        let chosen = choose_asset(&Platform::new("linux", "x86_64"), &release).unwrap();
        assert_eq!(chosen.name, "delta-desktop_0.6.0_amd64.deb");
    }

    #[test]
    fn macos_aarch64_downloads_the_dmg() {
        let release = newer_release(
            "0.6.0",
            vec![
                asset("delta-desktop_0.6.0_amd64.deb"),
                asset("Delta_0.6.0_aarch64.dmg"),
            ],
        );
        let chosen = choose_asset(&Platform::new("macos", "aarch64"), &release).unwrap();
        assert_eq!(chosen.name, "Delta_0.6.0_aarch64.dmg");
    }

    #[test]
    fn another_platform_is_unsupported() {
        let release = newer_release("0.6.0", vec![asset("delta-desktop_0.6.0_amd64.deb")]);
        for (os, arch) in [
            ("windows", "x86_64"),
            ("macos", "x86_64"),
            ("linux", "aarch64"),
        ] {
            let err = choose_asset(&Platform::new(os, arch), &release).unwrap_err();
            assert!(
                matches!(&err, UpdateRefusal::UnsupportedPlatform { platform } if *platform == format!("{os} {arch}")),
                "{err:?}"
            );
        }
    }

    #[test]
    fn a_release_without_the_platforms_asset_is_refused() {
        // Another version's file does not stand in for this one's.
        let release = newer_release(
            "0.6.0",
            vec![
                asset("delta-desktop_0.5.0_amd64.deb"),
                asset("Delta_0.6.0_aarch64.dmg"),
            ],
        );
        let err = choose_asset(&Platform::new("linux", "x86_64"), &release).unwrap_err();
        assert!(
            matches!(
                &err,
                UpdateRefusal::MissingAsset { version, asset }
                    if version == "v0.6.0" && asset == "delta-desktop_0.6.0_amd64.deb"
            ),
            "{err:?}"
        );
    }

    #[test]
    fn a_downloadable_asset_is_named_for_the_platform_pinned_and_digested() {
        let linux = Platform::new("linux", "x86_64");
        let release = newer_release("0.6.0", vec![asset("delta-desktop_0.6.0_amd64.deb")]);
        let chosen = downloadable_asset(&linux, &release).unwrap();
        assert_eq!(chosen.name, "delta-desktop_0.6.0_amd64.deb");

        let err = downloadable_asset(&Platform::new("windows", "x86_64"), &release).unwrap_err();
        assert!(
            matches!(err, UpdateRefusal::UnsupportedPlatform { .. }),
            "{err:?}"
        );

        let mut untrusted = release.clone();
        untrusted.assets[0].download_url = "https://example.com/a.deb".into();
        let err = downloadable_asset(&linux, &untrusted).unwrap_err();
        assert!(matches!(err, UpdateRefusal::UntrustedUrl(_)), "{err:?}");

        for digest in [
            None,
            Some("sha512:00".to_owned()),
            Some("sha256:abc".to_owned()),
        ] {
            let mut undigested = release.clone();
            undigested.assets[0].digest = digest.clone();
            let err = downloadable_asset(&linux, &undigested).unwrap_err();
            assert!(
                matches!(
                    &err,
                    UpdateRefusal::NoDigest { version, asset }
                        if version == "v0.6.0" && asset == "delta-desktop_0.6.0_amd64.deb"
                ),
                "{digest:?}: {err:?}"
            );
        }
    }

    #[test]
    fn the_signed_asset_carries_the_releases_signature_of_it_and_its_version() {
        let release = newer_release(
            "0.6.0",
            vec![
                asset(LINUX_DEB),
                signature_of("Delta_0.6.0_aarch64.dmg"),
                signature_of(LINUX_DEB),
            ],
        );
        let chosen = downloadable_asset(&Platform::new("linux", "x86_64"), &release).unwrap();
        let signed = signed_asset(&release, chosen).unwrap();
        assert_eq!(signed.asset, asset(LINUX_DEB));
        assert_eq!(
            signed.signature_url,
            format!("{RELEASE_DOWNLOAD_PREFIX}v0.6.0/{LINUX_DEB}.minisig")
        );
        assert_eq!(signed.version, semver::Version::new(0, 6, 0));
    }

    #[test]
    fn an_asset_without_a_signature_is_refused() {
        // Another asset's signature does not stand in for this one's.
        let release = newer_release(
            "0.6.0",
            vec![asset(LINUX_DEB), signature_of("Delta_0.6.0_aarch64.dmg")],
        );
        let err = signed_asset(&release, &asset(LINUX_DEB)).unwrap_err();
        assert!(
            matches!(
                &err,
                UpdateRefusal::NoSignature { version, asset }
                    if version == "v0.6.0" && asset == LINUX_DEB
            ),
            "{err:?}"
        );
    }

    #[test]
    fn a_signature_outside_the_pinned_prefix_is_refused() {
        let mut signature = signature_of(LINUX_DEB);
        signature.download_url = format!("https://example.com/{LINUX_DEB}.minisig");
        let release = newer_release("0.6.0", vec![asset(LINUX_DEB), signature]);
        let err = signed_asset(&release, &asset(LINUX_DEB)).unwrap_err();
        assert!(matches!(err, UpdateRefusal::UntrustedUrl(_)), "{err:?}");
    }

    #[test]
    fn a_url_under_the_prefix_is_accepted() {
        check_download_url(
            "https://github.com/x7c1/delta/releases/download/v0.6.0/delta-desktop_0.6.0_amd64.deb",
        )
        .unwrap();
    }

    #[test]
    fn a_url_outside_the_prefix_is_refused() {
        for url in [
            // Another host.
            "https://example.com/x7c1/delta/releases/download/v0.6.0/a.deb",
            "https://github.com.evil.example/x7c1/delta/releases/download/v0.6.0/a.deb",
            "https://objects.githubusercontent.com/x7c1/delta/releases/download/v0.6.0/a.deb",
            // Another repository.
            "https://github.com/someone-else/delta/releases/download/v0.6.0/a.deb",
            "https://github.com/x7c1/delta-fork/releases/download/v0.6.0/a.deb",
            // Not HTTPS.
            "http://github.com/x7c1/delta/releases/download/v0.6.0/a.deb",
            // Outside `releases/download/`.
            "https://github.com/x7c1/delta/releases/tag/v0.6.0",
            "https://github.com/x7c1/delta/archive/refs/tags/v0.6.0.tar.gz",
            "https://github.com/x7c1/delta/releases/download/",
            // Inside it only as a string: a URL parser resolves these elsewhere.
            "https://github.com/x7c1/delta/releases/download/../../../evil/a.deb",
            "https://github.com/x7c1/delta/releases/download/%2e%2e/%2e%2e/evil/a.deb",
            "https://github.com/x7c1/delta/releases/download/v0.6.0\\..\\..\\evil",
            "https://github.com/x7c1/delta/releases/download/v0.6.0/a.deb?x=1",
            "https://github.com/x7c1/delta/releases/download/v0.6.0//a.deb",
        ] {
            let err = check_download_url(url).unwrap_err();
            assert!(
                matches!(&err, UpdateRefusal::UntrustedUrl(refused) if refused == url),
                "{url}: {err:?}"
            );
        }
    }
}
