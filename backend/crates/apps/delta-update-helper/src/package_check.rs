//! Whether a package is the update asked for: Delta's, the requested
//! version, and newer than the installed one.

use semver::Version;

use crate::Refusal;

/// The Debian package Delta's desktop app ships as.
pub const PACKAGE_NAME: &str = "delta-desktop";

/// Read `Package` and `Version` out of `dpkg-deb --field <file> Package
/// Version`'s output (`Key: value` lines).
pub fn package_fields(output: &str) -> Result<(String, String), Refusal> {
    let field = |key: &str| {
        output.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name == key).then(|| value.trim().to_owned())
        })
    };
    match (field("Package"), field("Version")) {
        (Some(name), Some(version)) => Ok((name, version)),
        _ => Err(Refusal::PackageUnreadable {
            what: "the update file's package name and version",
            cause: format!("dpkg-deb answered {:?}", output.trim()),
        }),
    }
}

/// Check that the package `name` of `version` is [`PACKAGE_NAME`] at exactly
/// `requested`, and newer than the `installed` version.
pub fn check_package(
    name: &str,
    version: &str,
    requested: &Version,
    installed: &str,
) -> Result<(), Refusal> {
    if name != PACKAGE_NAME {
        return Err(Refusal::NotDelta(name.to_owned()));
    }
    let mismatch = || Refusal::VersionMismatch {
        requested: requested.to_string(),
        actual: version.to_owned(),
    };
    let package = Version::parse(version).map_err(|_| mismatch())?;
    if package != *requested {
        return Err(mismatch());
    }
    let installed_version =
        Version::parse(installed.trim()).map_err(|source| Refusal::InstalledVersionMalformed {
            installed: installed.to_owned(),
            source,
        })?;
    if package <= installed_version {
        return Err(Refusal::NotNewer {
            package: package.to_string(),
            installed: installed_version.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v060() -> Version {
        Version::new(0, 6, 0)
    }

    #[test]
    fn dpkg_debs_fields_are_read() {
        assert_eq!(
            package_fields("Package: delta-desktop\nVersion: 0.6.0\n").unwrap(),
            ("delta-desktop".to_owned(), "0.6.0".to_owned())
        );
        for output in ["", "Package: delta-desktop\n", "Version: 0.6.0\n"] {
            assert!(matches!(
                package_fields(output),
                Err(Refusal::PackageUnreadable { .. })
            ));
        }
    }

    #[test]
    fn delta_at_the_requested_newer_version_is_accepted() {
        check_package("delta-desktop", "0.6.0", &v060(), "0.5.0\n").unwrap();
    }

    #[test]
    fn another_package_is_refused() {
        for name in ["delta", "delta-desktop-dev", "apt", ""] {
            let err = check_package(name, "0.6.0", &v060(), "0.5.0").unwrap_err();
            assert!(
                matches!(&err, Refusal::NotDelta(read) if read == name),
                "{err:?}"
            );
        }
    }

    #[test]
    fn another_version_than_the_requested_one_is_refused() {
        for version in ["0.6.1", "0.7.0", "0.5.0", "0.6.0-1", "1:0.6.0", ""] {
            let err = check_package("delta-desktop", version, &v060(), "0.5.0").unwrap_err();
            assert!(
                matches!(&err, Refusal::VersionMismatch { actual, .. } if actual == version),
                "{version}: {err:?}"
            );
        }
    }

    #[test]
    fn a_version_not_newer_than_the_installed_one_is_refused() {
        for installed in ["0.6.0", "0.7.0", "1.0.0"] {
            let err = check_package("delta-desktop", "0.6.0", &v060(), installed).unwrap_err();
            assert!(
                matches!(err, Refusal::NotNewer { .. }),
                "{installed}: {err:?}"
            );
        }
    }

    #[test]
    fn an_unreadable_installed_version_is_refused() {
        let err = check_package("delta-desktop", "0.6.0", &v060(), "").unwrap_err();
        assert!(
            matches!(err, Refusal::InstalledVersionMalformed { .. }),
            "{err:?}"
        );
    }
}
