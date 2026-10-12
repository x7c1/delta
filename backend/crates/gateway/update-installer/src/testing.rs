//! Fixtures shared by the macOS installer's tests: app bundles and mounted
//! disk images written to a temporary directory.

use std::path::Path;

use crate::image_check::{APP_NAME, BUNDLE_IDENTIFIER};

/// Write an `Info.plist` stating `identifier` and `version` into the app
/// bundle `app`, creating it.
pub(crate) fn write_app(app: &Path, identifier: &str, version: &str) {
    let contents = app.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).unwrap();
    let mut info = plist::Dictionary::new();
    info.insert("CFBundleIdentifier".into(), identifier.into());
    info.insert("CFBundleShortVersionString".into(), version.into());
    info.insert("CFBundleExecutable".into(), "delta-desktop".into());
    plist::Value::Dictionary(info)
        .to_file_xml(contents.join("Info.plist"))
        .unwrap();
    std::fs::write(contents.join("MacOS").join("delta-desktop"), version).unwrap();
}

/// A fake mounted Delta disk image of `version` at `root`: `Delta.app`
/// and the `Applications` symlink a drag-to-install image carries.
pub(crate) fn write_image(root: &Path, version: &str) {
    write_app(&root.join(APP_NAME), BUNDLE_IDENTIFIER, version);
    std::os::unix::fs::symlink("/Applications", root.join("Applications")).unwrap();
}
