//! The release key is written in one place: `PUBLIC_KEY` in this crate,
//! which the desktop app and the update helper both read. A second copy in
//! another crate could drift from it, or be rotated without the other.

use std::path::{Path, PathBuf};

use release_signature::PUBLIC_KEY;

/// Every `.rs` and `Cargo.toml` file under `dir`, skipping build output.
fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            || path.file_name().is_some_and(|name| name == "Cargo.toml")
        {
            found.push(path);
        }
    }
}

#[test]
fn the_release_key_is_written_only_in_this_crate() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let crates = manifest.join("../..").canonicalize().unwrap();
    let definition = manifest.join("src/lib.rs").canonicalize().unwrap();
    let mut files = Vec::new();
    sources(&crates, &mut files);
    assert!(files.len() > 10, "found too few sources under {crates:?}");

    let holding: Vec<PathBuf> = files
        .into_iter()
        .filter(|file| std::fs::read_to_string(file).unwrap().contains(PUBLIC_KEY))
        .map(|file| file.canonicalize().unwrap())
        .collect();
    assert_eq!(holding, [definition]);
}

#[test]
fn the_app_and_the_update_helper_both_check_with_this_crates_key() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for user in [
        "gateway/release-feed/src/github_asset_downloader/mod.rs",
        "apps/delta-update-helper/src/main.rs",
    ] {
        let source = std::fs::read_to_string(crates.join(user)).unwrap();
        assert!(
            source.contains("ReleaseKey::production()"),
            "{user} does not check with the release key"
        );
    }
}
