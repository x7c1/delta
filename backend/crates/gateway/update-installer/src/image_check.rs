use std::path::{Path, PathBuf};

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
/// `Err` is the one-line reason the image is rejected.
pub fn check_image(root: &Path, version: &str) -> Result<PathBuf, String> {
    let entries = std::fs::read_dir(root)
        .map_err(|err| format!("could not list the disk image's contents: {err}"))?;
    let mut apps = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|err| format!("could not list the disk image's contents: {err}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.to_ascii_lowercase().ends_with(".app") {
            apps.push(name);
        }
    }
    apps.sort();
    match apps.as_slice() {
        [] => return Err(format!("the disk image holds no {APP_NAME}")),
        [only] if only == APP_NAME => {}
        [only] => return Err(format!("the disk image holds {only}, not {APP_NAME}")),
        many => {
            return Err(format!(
                "the disk image holds {} apps ({}), not only {APP_NAME}",
                many.len(),
                many.join(", ")
            ))
        }
    }
    let app = root.join(APP_NAME);
    let is_directory = std::fs::symlink_metadata(&app)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false);
    if !is_directory {
        return Err(format!("the disk image's {APP_NAME} is not a directory"));
    }
    check_info_plist(&app.join("Contents").join("Info.plist"), version)?;
    Ok(app)
}

/// Check that the `Info.plist` at `path` names Delta at `version`.
fn check_info_plist(path: &Path, version: &str) -> Result<(), String> {
    let info = plist::Value::from_file(path)
        .map_err(|err| format!("could not read the disk image's {APP_NAME} Info.plist: {err}"))?;
    let info = info
        .as_dictionary()
        .ok_or_else(|| format!("the disk image's {APP_NAME} Info.plist is not a dictionary"))?;
    let field = |key: &str| info.get(key).and_then(plist::Value::as_string);

    let identifier = field("CFBundleIdentifier");
    if identifier != Some(BUNDLE_IDENTIFIER) {
        return Err(format!(
            "the disk image's {APP_NAME} is {}, not {BUNDLE_IDENTIFIER}",
            identifier.unwrap_or("an app without a CFBundleIdentifier")
        ));
    }
    let expected = version.strip_prefix('v').unwrap_or(version);
    let actual = field("CFBundleShortVersionString");
    if actual != Some(expected) {
        return Err(format!(
            "the disk image's {APP_NAME} is version {}, not {expected}",
            actual.unwrap_or("(none stated)")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{write_app, write_image};

    #[test]
    fn exactly_one_delta_app_of_the_version_passes() {
        let root = tempfile::tempdir().unwrap();
        write_image(root.path(), "0.6.0");
        assert_eq!(
            check_image(root.path(), "v0.6.0"),
            Ok(root.path().join("Delta.app"))
        );
    }

    #[test]
    fn a_missing_app_or_another_one_is_rejected() {
        let empty = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/Applications", empty.path().join("Applications")).unwrap();
        assert_eq!(
            check_image(empty.path(), "v0.6.0"),
            Err("the disk image holds no Delta.app".to_owned())
        );

        let other = tempfile::tempdir().unwrap();
        write_app(&other.path().join("Other.app"), BUNDLE_IDENTIFIER, "0.6.0");
        assert_eq!(
            check_image(other.path(), "v0.6.0"),
            Err("the disk image holds Other.app, not Delta.app".to_owned())
        );
    }

    #[test]
    fn two_apps_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        write_image(root.path(), "0.6.0");
        write_app(&root.path().join("Helper.app"), BUNDLE_IDENTIFIER, "0.6.0");
        assert_eq!(
            check_image(root.path(), "v0.6.0"),
            Err(
                "the disk image holds 2 apps (Delta.app, Helper.app), not only Delta.app"
                    .to_owned()
            )
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
        assert_eq!(
            check_image(root.path(), "v0.6.0"),
            Err("the disk image's Delta.app is not a directory".to_owned())
        );
    }

    #[test]
    fn another_identifier_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        write_app(&root.path().join("Delta.app"), "com.example.delta", "0.6.0");
        assert_eq!(
            check_image(root.path(), "v0.6.0"),
            Err(
                "the disk image's Delta.app is com.example.delta, not io.github.x7c1.delta"
                    .to_owned()
            )
        );
    }

    #[test]
    fn another_version_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        write_image(root.path(), "0.5.0");
        assert_eq!(
            check_image(root.path(), "v0.6.0"),
            Err("the disk image's Delta.app is version 0.5.0, not 0.6.0".to_owned())
        );
    }

    #[test]
    fn an_unreadable_info_plist_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        write_image(root.path(), "0.6.0");
        let info = root.path().join("Delta.app/Contents/Info.plist");
        std::fs::write(&info, "not a property list").unwrap();
        let reason = check_image(root.path(), "v0.6.0").unwrap_err();
        assert!(
            reason.starts_with("could not read the disk image's Delta.app Info.plist"),
            "{reason}"
        );

        std::fs::remove_file(&info).unwrap();
        let reason = check_image(root.path(), "v0.6.0").unwrap_err();
        assert!(
            reason.starts_with("could not read the disk image's Delta.app Info.plist"),
            "{reason}"
        );
    }
}
