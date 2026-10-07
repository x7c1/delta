use semver::Version;

use super::{NewerRelease, ReleaseCheckError, RELEASE_PAGE_PREFIX};
use crate::ports::PublishedRelease;

/// Judge `release` against `current`: `Some` when it is strictly newer.
///
/// The answer is validated whole before comparing, so a malformed answer is a
/// failure even when its version would not have counted.
pub(super) fn judge_release(
    current: &Version,
    release: PublishedRelease,
) -> Result<Option<NewerRelease>, ReleaseCheckError> {
    if !release.html_url.starts_with(RELEASE_PAGE_PREFIX) {
        return Err(ReleaseCheckError::Page(release.html_url));
    }
    let parsed = match release.tag_name.strip_prefix('v') {
        Some(bare) => Version::parse(bare).map_err(Some),
        None => Err(None),
    };
    let version = match parsed {
        Ok(version) => version,
        Err(cause) => {
            return Err(ReleaseCheckError::Tag {
                tag: release.tag_name,
                cause,
            })
        }
    };
    let newer = version.cmp_precedence(current).is_gt();
    Ok(newer.then_some(NewerRelease {
        version,
        url: release.html_url,
    }))
}

#[cfg(test)]
mod tests {
    use super::super::testing::release;
    use super::*;

    fn judge(
        current: &str,
        release: PublishedRelease,
    ) -> Result<Option<String>, ReleaseCheckError> {
        let current = Version::parse(current).unwrap();
        judge_release(&current, release).map(|newer| newer.map(|r| r.display_version()))
    }

    #[test]
    fn a_greater_release_is_newer() {
        assert_eq!(
            judge("0.5.0", release("v0.6.0")).unwrap(),
            Some("v0.6.0".into())
        );
        assert_eq!(
            judge("0.5.0", release("v0.5.1")).unwrap(),
            Some("v0.5.1".into())
        );
    }

    #[test]
    fn an_equal_release_is_not_newer() {
        assert_eq!(judge("0.5.0", release("v0.5.0")).unwrap(), None);
    }

    #[test]
    fn a_lower_release_is_not_newer() {
        assert_eq!(judge("0.5.0", release("v0.4.9")).unwrap(), None);
    }

    #[test]
    fn a_debug_build_at_the_same_base_version_is_up_to_date() {
        assert_eq!(judge("0.5.0+dev.a1b2c3d", release("v0.5.0")).unwrap(), None);
    }

    #[test]
    fn a_debug_build_behind_a_release_is_told() {
        assert_eq!(
            judge("0.5.0+dev.a1b2c3d", release("v0.5.1")).unwrap(),
            Some("v0.5.1".into())
        );
    }

    #[test]
    fn a_tag_that_is_not_semver_is_a_failed_check() {
        for (tag, has_semver_cause) in [
            ("0.6.0", false),
            ("v0.6", true),
            ("latest", false),
            ("v0.6.0.1", true),
            ("", false),
        ] {
            let err = judge("0.5.0", release(tag)).unwrap_err();
            assert!(
                matches!(
                    &err,
                    ReleaseCheckError::Tag { tag: t, cause } if t == tag && cause.is_some() == has_semver_cause
                ),
                "tag {tag:?}: {err:?}"
            );
        }
    }

    #[test]
    fn a_page_outside_the_pinned_prefix_is_a_failed_check() {
        for url in [
            "https://github.com/someone-else/delta/releases/tag/v0.6.0",
            "http://github.com/x7c1/delta/releases/tag/v0.6.0",
            "https://github.com/x7c1/delta/releases",
            "https://github.com.evil.example/x7c1/delta/releases/tag/v0.6.0",
        ] {
            let answer = PublishedRelease {
                tag_name: "v0.6.0".into(),
                html_url: url.into(),
            };
            let err = judge("0.5.0", answer).unwrap_err();
            assert!(
                matches!(&err, ReleaseCheckError::Page(page) if page == url),
                "url {url:?}: {err:?}"
            );
        }
    }
}
