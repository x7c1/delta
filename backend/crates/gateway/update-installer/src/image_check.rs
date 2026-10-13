use std::path::{Path, PathBuf};

use crate::image_check_error::ImageCheckError;

/// The app a Delta disk image holds at its root.
pub const APP_NAME: &str = "Delta.app";

/// Delta's bundle identifier (`identifier` in the desktop app's
/// `tauri.conf.json`).
pub const BUNDLE_IDENTIFIER: &str = "io.github.x7c1.delta";

/// Check what the disk image mounted at `root` holds before anything is
/// copied out of it, and answer the app to copy: exactly one app at the
/// image's root, a directory named [`APP_NAME`] (not a symlink), whose
/// `Contents/Info.plist` states `CFBundleIdentifier` [`BUNDLE_IDENTIFIER`]
/// and `CFBundleShortVersionString` the requested `version` (`v<version>`).
///
/// `Err` is why the image is rejected, or why it could not be checked
/// ([`ImageCheckError::rejects_file`]).
pub fn check_image(root: &Path, version: &str) -> Result<PathBuf, ImageCheckError> {
    let entries = std::fs::read_dir(root).map_err(ImageCheckError::Unlisted)?;
    let mut apps = Vec::new();
    for entry in entries {
        let entry = entry.map_err(ImageCheckError::Unlisted)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.to_ascii_lowercase().ends_with(".app") {
            apps.push(name);
        }
    }
    apps.sort();
    match apps.as_slice() {
        [] => return Err(ImageCheckError::NoApp),
        [only] if only == APP_NAME => {}
        [only] => return Err(ImageCheckError::OtherApp { name: only.clone() }),
        _ => return Err(ImageCheckError::ManyApps { names: apps }),
    }
    let app = root.join(APP_NAME);
    let is_directory = std::fs::symlink_metadata(&app)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false);
    if !is_directory {
        return Err(ImageCheckError::NotADirectory);
    }
    check_info_plist(&app.join("Contents").join("Info.plist"), version)?;
    Ok(app)
}

/// Check that the `Info.plist` at `path` names Delta at `version`.
fn check_info_plist(path: &Path, version: &str) -> Result<(), ImageCheckError> {
    let info = plist::Value::from_file(path).map_err(ImageCheckError::InfoPlistUnreadable)?;
    let info = info
        .as_dictionary()
        .ok_or(ImageCheckError::InfoPlistNotADictionary)?;
    let field = |key: &str| info.get(key).and_then(plist::Value::as_string);

    let identifier = field("CFBundleIdentifier");
    if identifier != Some(BUNDLE_IDENTIFIER) {
        return Err(ImageCheckError::OtherIdentifier {
            identifier: identifier.map(str::to_owned),
        });
    }
    let expected = version.strip_prefix('v').unwrap_or(version);
    let actual = field("CFBundleShortVersionString");
    if actual != Some(expected) {
        return Err(ImageCheckError::OtherVersion {
            expected: expected.to_owned(),
            actual: actual.map(str::to_owned),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;
    use crate::testing::{write_app, write_image};

    /// The rejection `check_image` answers for the image at `root`, with its
    /// text.
    fn rejection(root: &Path) -> (ImageCheckError, String) {
        let err = check_image(root, "v0.6.0").unwrap_err();
        let text = err.to_string();
        (err, text)
    }

    #[test]
    fn exactly_one_delta_app_of_the_version_passes() {
        let root = tempfile::tempdir().unwrap();
        write_image(root.path(), "0.6.0");
        assert_eq!(
            check_image(root.path(), "v0.6.0").unwrap(),
            root.path().join("Delta.app")
        );
    }

    #[test]
    fn a_missing_app_or_another_one_is_rejected() {
        let empty = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/Applications", empty.path().join("Applications")).unwrap();
        let (err, text) = rejection(empty.path());
        assert!(matches!(err, ImageCheckError::NoApp), "{err:?}");
        assert_eq!(text, "the disk image holds no Delta.app");

        let other = tempfile::tempdir().unwrap();
        write_app(&other.path().join("Other.app"), BUNDLE_IDENTIFIER, "0.6.0");
        let (err, text) = rejection(other.path());
        assert!(
            matches!(&err, ImageCheckError::OtherApp { name } if name == "Other.app"),
            "{err:?}"
        );
        assert_eq!(text, "the disk image holds Other.app, not Delta.app");
    }

    #[test]
    fn two_apps_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        write_image(root.path(), "0.6.0");
        write_app(&root.path().join("Helper.app"), BUNDLE_IDENTIFIER, "0.6.0");
        let (err, text) = rejection(root.path());
        assert!(
            matches!(&err, ImageCheckError::ManyApps { names } if names == &["Delta.app", "Helper.app"]),
            "{err:?}"
        );
        assert_eq!(
            text,
            "the disk image holds 2 apps (Delta.app, Helper.app), not only Delta.app"
        );
    }

    #[test]
    fn a_symlinked_app_is_rejected() {
        let elsewhere = tempfile::tempdir().unwrap();
        write_app(
            &elsewhere.path().join("Delta.app"),
            BUNDLE_IDENTIFIER,
            "0.6.0",
        );
        let root = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(
            elsewhere.path().join("Delta.app"),
            root.path().join("Delta.app"),
        )
        .unwrap();
        let (err, text) = rejection(root.path());
        assert!(matches!(err, ImageCheckError::NotADirectory), "{err:?}");
        assert_eq!(text, "the disk image's Delta.app is not a directory");
    }

    #[test]
    fn another_identifier_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        write_app(&root.path().join("Delta.app"), "com.example.delta", "0.6.0");
        let (err, text) = rejection(root.path());
        assert!(
            matches!(
                &err,
                ImageCheckError::OtherIdentifier { identifier: Some(identifier) }
                    if identifier == "com.example.delta"
            ),
            "{err:?}"
        );
        assert_eq!(
            text,
            "the disk image's Delta.app is com.example.delta, not io.github.x7c1.delta"
        );
    }

    #[test]
    fn another_version_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        write_image(root.path(), "0.5.0");
        let (err, text) = rejection(root.path());
        assert!(
            matches!(
                &err,
                ImageCheckError::OtherVersion { expected, actual: Some(actual) }
                    if expected == "0.6.0" && actual == "0.5.0"
            ),
            "{err:?}"
        );
        assert_eq!(
            text,
            "the disk image's Delta.app is version 0.5.0, not 0.6.0"
        );
    }

    /// An unreadable `Info.plist` keeps the plist error as its source, and
    /// its own text leaves that error to the source.
    #[test]
    fn an_unreadable_info_plist_is_rejected_with_its_cause() {
        let root = tempfile::tempdir().unwrap();
        write_image(root.path(), "0.6.0");
        let info = root.path().join("Delta.app/Contents/Info.plist");
        std::fs::write(&info, "not a property list").unwrap();
        let (err, text) = rejection(root.path());
        assert!(
            matches!(err, ImageCheckError::InfoPlistUnreadable(_)),
            "{err:?}"
        );
        assert_eq!(text, "could not read the disk image's Delta.app Info.plist");
        assert!(
            err.source()
                .is_some_and(|source| source.is::<plist::Error>()),
            "{err:?}"
        );

        std::fs::remove_file(&info).unwrap();
        let (err, _) = rejection(root.path());
        assert!(
            matches!(err, ImageCheckError::InfoPlistUnreadable(_)),
            "{err:?}"
        );
    }

    /// A root that cannot be listed keeps the I/O error as its source.
    #[test]
    fn an_unlistable_root_is_rejected_with_its_cause() {
        let dir = tempfile::tempdir().unwrap();
        let (err, text) = rejection(&dir.path().join("missing"));
        assert!(matches!(err, ImageCheckError::Unlisted(_)), "{err:?}");
        assert_eq!(text, "could not list the disk image's contents");
        let source = err
            .source()
            .and_then(|source| source.downcast_ref::<std::io::Error>());
        assert_eq!(
            source.map(std::io::Error::kind),
            Some(std::io::ErrorKind::NotFound),
            "{err:?}"
        );
    }
}
