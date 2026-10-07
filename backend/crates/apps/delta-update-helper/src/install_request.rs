//! The one command line the helper accepts.

use std::ffi::OsString;
use std::path::PathBuf;

use semver::Version;

use crate::Refusal;

/// What the caller asks for: which release to install, and which file holds
/// it. Everything else the helper finds out itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    /// The release's tag, `v<version>`, exactly as GitHub names it.
    pub tag: String,
    /// The version the tag names.
    pub version: Version,
    /// The file to install, an absolute path.
    pub file: PathBuf,
}

impl InstallRequest {
    /// Parse the arguments after the program name: exactly
    /// `install --version v<version> --file <absolute path>`, in that order.
    ///
    /// The tag must be `v` followed by a version written the way SemVer
    /// writes it, so it can go into GitHub's URL as it is; the path must be
    /// absolute, so it does not depend on the directory `pkexec` started in.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, Refusal> {
        let args: Vec<OsString> = args.into_iter().collect();
        let [command, version_flag, tag, file_flag, file] = args.as_slice() else {
            return Err(Refusal::Usage(format!(
                "expected 5 arguments, got {}",
                args.len()
            )));
        };
        for (arg, expected) in [
            (command, "install"),
            (version_flag, "--version"),
            (file_flag, "--file"),
        ] {
            if arg != expected {
                return Err(Refusal::Usage(format!("expected {expected}, got {arg:?}")));
            }
        }
        let tag = tag
            .to_str()
            .ok_or_else(|| Refusal::Usage(format!("the tag {tag:?} is not UTF-8")))?;
        let version = parse_tag(tag)?;
        let file = PathBuf::from(file);
        if !file.is_absolute() {
            return Err(Refusal::Usage(format!(
                "the file {} is not an absolute path",
                file.display()
            )));
        }
        Ok(Self {
            tag: tag.to_owned(),
            version,
            file,
        })
    }
}

/// The version a `v<version>` tag names, refusing any tag SemVer would write
/// differently.
fn parse_tag(tag: &str) -> Result<Version, Refusal> {
    let bad = || Refusal::Usage(format!("the tag {tag:?} is not v<version>"));
    let version = tag
        .strip_prefix('v')
        .and_then(|bare| Version::parse(bare).ok())
        .ok_or_else(bad)?;
    if format!("v{version}") != tag {
        return Err(bad());
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<InstallRequest, Refusal> {
        InstallRequest::parse(args.iter().map(OsString::from))
    }

    #[test]
    fn the_install_command_is_accepted() {
        let request = parse(&[
            "install",
            "--version",
            "v0.6.0",
            "--file",
            "/home/u/.local/share/io.github.x7c1.delta/updates/delta-desktop_0.6.0_amd64.deb",
        ])
        .unwrap();
        assert_eq!(request.tag, "v0.6.0");
        assert_eq!(request.version, Version::new(0, 6, 0));
        assert_eq!(
            request.file,
            PathBuf::from(
                "/home/u/.local/share/io.github.x7c1.delta/updates/delta-desktop_0.6.0_amd64.deb"
            )
        );
    }

    #[test]
    fn anything_else_is_refused() {
        for args in [
            &[][..],
            &["install"],
            &["install", "--version", "v0.6.0"],
            // Another command, flag order or flag spelling.
            &["remove", "--version", "v0.6.0", "--file", "/a.deb"],
            &["install", "--file", "/a.deb", "--version", "v0.6.0"],
            &["install", "--version=v0.6.0", "--file", "/a.deb", ""],
            &["install", "-v", "v0.6.0", "--file", "/a.deb"],
            // A trailing argument, which apt would otherwise never see.
            &["install", "--version", "v0.6.0", "--file", "/a.deb", "--x"],
            // A relative path.
            &["install", "--version", "v0.6.0", "--file", "a.deb"],
            &["install", "--version", "v0.6.0", "--file", "./a.deb"],
            &["install", "--version", "v0.6.0", "--file", ""],
            // A tag that is not v<version>, or that SemVer writes otherwise.
            &["install", "--version", "0.6.0", "--file", "/a.deb"],
            &["install", "--version", "v0.6", "--file", "/a.deb"],
            &["install", "--version", "v00.6.0", "--file", "/a.deb"],
            &["install", "--version", "v0.6.0/../..", "--file", "/a.deb"],
            &["install", "--version", "v0.6.0?x=1", "--file", "/a.deb"],
            &["install", "--version", " v0.6.0", "--file", "/a.deb"],
        ] {
            let err = parse(args).unwrap_err();
            assert!(matches!(err, Refusal::Usage(_)), "{args:?}: {err:?}");
        }
    }
}
