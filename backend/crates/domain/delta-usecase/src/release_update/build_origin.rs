//! Where this build was made.

/// Where this build was made.
///
/// A desktop app built locally from a newer tree carries the same version and
/// identifier as the release bundle, so replacing it with the release would
/// roll the user's tree back. Only a build made by the release workflow may
/// therefore be replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildOrigin {
    /// Built by the release workflow, which asks for it explicitly.
    Release,
    /// Built anywhere else (`make desktop`, `make desktop-dev`, `cargo build`).
    Local,
}

impl BuildOrigin {
    /// The origin a build-time `DELTA_BUILD_ORIGIN` value names: `release`
    /// only for exactly `release`; unset, empty or anything else is `local`,
    /// so a build can only claim `release` by asking for it.
    pub fn from_build_env(value: Option<&str>) -> Self {
        match value {
            Some("release") => Self::Release,
            _ => Self::Local,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_explicit_release_value_is_a_release_build() {
        assert_eq!(
            BuildOrigin::from_build_env(Some("release")),
            BuildOrigin::Release
        );
        for value in [None, Some(""), Some("Release"), Some("local"), Some("ci")] {
            assert_eq!(
                BuildOrigin::from_build_env(value),
                BuildOrigin::Local,
                "{value:?}"
            );
        }
    }
}
