use std::io;

use crate::image_check::{APP_NAME, BUNDLE_IDENTIFIER};

/// Why a mounted disk image does not hold the app to install, or could not
/// be checked ([`check_image`](crate::image_check::check_image)).
#[derive(Debug, thiserror::Error)]
pub enum ImageCheckError {
    /// The image's root could not be listed.
    #[error("could not list the disk image's contents")]
    Unlisted(#[source] io::Error),
    /// No app is at the image's root.
    #[error("the disk image holds no {APP_NAME}")]
    NoApp,
    /// The one app at the image's root is not named [`APP_NAME`].
    #[error("the disk image holds {name}, not {APP_NAME}")]
    OtherApp { name: String },
    /// More than one app is at the image's root, sorted by name.
    #[error(
        "the disk image holds {} apps ({}), not only {APP_NAME}",
        names.len(),
        names.join(", ")
    )]
    ManyApps { names: Vec<String> },
    /// The [`APP_NAME`] at the image's root is not a directory (a symlink
    /// included).
    #[error("the disk image's {APP_NAME} is not a directory")]
    NotADirectory,
    /// The app's `Contents/Info.plist` could not be read as a property
    /// list.
    #[error("could not read the disk image's {APP_NAME} Info.plist")]
    InfoPlistUnreadable(#[source] plist::Error),
    /// The app's `Info.plist` is a property list but not a dictionary.
    #[error("the disk image's {APP_NAME} Info.plist is not a dictionary")]
    InfoPlistNotADictionary,
    /// The app's `CFBundleIdentifier` is not [`BUNDLE_IDENTIFIER`]; `None`
    /// where it states none.
    #[error(
        "the disk image's {APP_NAME} is {}, not {BUNDLE_IDENTIFIER}",
        identifier.as_deref().unwrap_or("an app without a CFBundleIdentifier")
    )]
    OtherIdentifier { identifier: Option<String> },
    /// The app's `CFBundleShortVersionString` is not the requested version
    /// (`expected`, without its `v`); `None` where it states none.
    #[error(
        "the disk image's {APP_NAME} is version {}, not {expected}",
        actual.as_deref().unwrap_or("(none stated)")
    )]
    OtherVersion {
        expected: String,
        actual: Option<String>,
    },
}

impl ImageCheckError {
    /// Whether the update itself is not one to install, by any means: every
    /// failure but [`Self::Unlisted`], where reading the mounted image failed
    /// and says nothing about what the image holds.
    pub fn rejects_file(&self) -> bool {
        !matches!(self, Self::Unlisted(_))
    }
}
