//! The icons the `.deb` installs come from `bundle.icon` of `tauri.conf.json`
//! (`tauri.macos.conf.json` replaces the list for the macOS bundle).

use std::path::PathBuf;

fn bundle_icons(config_file: &str) -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(config_file);
    let text = std::fs::read_to_string(&path).unwrap();
    let config: serde_json::Value = serde_json::from_str(&text).unwrap();
    config["bundle"]["icon"]
        .as_array()
        .unwrap_or_else(|| panic!("{config_file} lists bundle.icon"))
        .iter()
        .map(|icon| icon.as_str().unwrap().to_owned())
        .collect()
}

/// Tauri's Debian bundler installs an `@2x` PNG under the hicolor theme as
/// `<size>@2` (`128x128@2x.png`, a 256 px file, as `256x256@2`). GNOME Shell
/// then takes it for a 512 px icon and scales it down from the wrong size,
/// which blurs the icon in the Activities overview and the app switcher at a
/// fractional scale. The `@2x` icon is for the macOS bundle only.
#[test]
fn the_linux_bundle_lists_no_hidpi_png() {
    let icons = bundle_icons("tauri.conf.json");
    let hidpi: Vec<&String> = icons
        .iter()
        .filter(|icon| icon.ends_with("@2x.png"))
        .collect();
    assert!(hidpi.is_empty(), "tauri.conf.json lists {hidpi:?}");
}
