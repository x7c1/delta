//! Check a bundle against its signature with the compiled-in release key,
//! exactly as the desktop app and the update helper do:
//!
//! ```text
//! cargo run -p release-signature --example verify-bundle -- \
//!     <bundle> <signature> <version>
//! ```
//!
//! The bundle's file name is the one its signature must name. For checking a
//! release, or a bundle signed with the release key by hand, outside the app.

use std::fs::File;
use std::path::Path;
use std::process::ExitCode;

use release_signature::{BundleSignature, ReleaseKey};
use semver::Version;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [bundle, signature, version] = args.as_slice() else {
        eprintln!("usage: verify-bundle <bundle> <signature> <version>");
        return ExitCode::from(2);
    };
    match verify(Path::new(bundle), Path::new(signature), version) {
        Ok(()) => {
            println!("{bundle}: signed by the release key as version {version}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{bundle}: {err}");
            ExitCode::FAILURE
        }
    }
}

fn verify(bundle: &Path, signature: &Path, version: &str) -> Result<(), String> {
    let version = Version::parse(version).map_err(|err| format!("version {version:?}: {err}"))?;
    let name = bundle
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("the bundle path names no file")?;
    let text = std::fs::read(signature).map_err(|err| format!("{}: {err}", signature.display()))?;
    let file = File::open(bundle).map_err(|err| err.to_string())?;
    BundleSignature::parse(&ReleaseKey::production(), &text, name, &version)
        .and_then(|signature| signature.verify_reader(file))
        .map_err(|err| with_causes(&err))
}

fn with_causes(err: &(dyn std::error::Error + 'static)) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}
