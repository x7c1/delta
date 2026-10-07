use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The directory macOS runs a translocated app from: a random, read-only
/// copy under `/private/var/folders/…/AppTranslocation/`.
const TRANSLOCATION_DIR: &str = "AppTranslocation";

/// Where the running app is, as its executable's path tells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleLocation {
    /// The app runs from the bundle at this path (`<dir>/Delta.app`), which
    /// an update may replace.
    Bundle(PathBuf),
    /// The app runs from a copy macOS made of it to run it from where it
    /// was downloaded or mounted (App Translocation): the copy at this path
    /// is read-only and random, so replacing it would change nothing.
    Translocated(PathBuf),
    /// The executable is not inside an app bundle (a development build, the
    /// CLI server): there is no bundle to replace.
    NotInBundle(PathBuf),
}

impl BundleLocation {
    /// Where an app whose executable is at `exe` runs from: the bundle
    /// `<dir>/<name>.app` when `exe` is `<dir>/<name>.app/Contents/MacOS/<exe>`.
    pub fn of_executable(exe: &Path) -> Self {
        let Some(bundle) = bundle_of(exe) else {
            return Self::NotInBundle(exe.to_path_buf());
        };
        if bundle
            .components()
            .any(|component| component.as_os_str() == TRANSLOCATION_DIR)
        {
            Self::Translocated(bundle)
        } else {
            Self::Bundle(bundle)
        }
    }
}

/// The bundle `exe` is the executable of, if it sits at
/// `<bundle>/Contents/MacOS/<exe>` and `<bundle>` is named `<name>.app`.
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_app = bundle
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.len() > ".app".len() && name.ends_with(".app"));
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && is_app)
        .then(|| bundle.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_executable_inside_a_bundle_yields_the_bundle() {
        assert_eq!(
            BundleLocation::of_executable(Path::new(
                "/Applications/Delta.app/Contents/MacOS/delta-desktop"
            )),
            BundleLocation::Bundle(PathBuf::from("/Applications/Delta.app"))
        );
        // Wherever the bundle is: nothing assumes /Applications.
        assert_eq!(
            BundleLocation::of_executable(Path::new(
                "/Users/u/Applications/Delta.app/Contents/MacOS/delta-desktop"
            )),
            BundleLocation::Bundle(PathBuf::from("/Users/u/Applications/Delta.app"))
        );
    }

    #[test]
    fn a_bundle_under_app_translocation_is_translocated() {
        let exe = "/private/var/folders/xy/abc123/T/AppTranslocation/0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0/d/Delta.app/Contents/MacOS/delta-desktop";
        assert_eq!(
            BundleLocation::of_executable(Path::new(exe)),
            BundleLocation::Translocated(PathBuf::from(
                "/private/var/folders/xy/abc123/T/AppTranslocation/0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0/d/Delta.app"
            ))
        );
    }

    #[test]
    fn an_executable_outside_any_bundle_yields_nothing() {
        for exe in [
            "/usr/local/bin/delta-server",
            "/Users/u/delta/backend/target/release/delta-desktop",
            // Not the bundle layout: a `.app` further up, or misnamed parts.
            "/Applications/Delta.app/Contents/Resources/delta-desktop",
            "/Applications/Delta.app/Contents/MacOS/bin/delta-desktop",
            "/Applications/Delta/Contents/MacOS/delta-desktop",
            "/Applications/.app/Contents/MacOS/delta-desktop",
            "delta-desktop",
            "/",
        ] {
            assert_eq!(
                BundleLocation::of_executable(Path::new(exe)),
                BundleLocation::NotInBundle(PathBuf::from(exe)),
                "{exe}"
            );
        }
    }
}
