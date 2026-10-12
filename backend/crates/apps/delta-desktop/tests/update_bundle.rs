//! The `.deb` ships Delta's update helper where the server runs it from, and
//! a polkit action that allows exactly that program, asking for an
//! administrator's password every time.

use std::path::{Path, PathBuf};

use update_installer::UPDATE_HELPER_PATH;

/// Where polkit reads action definitions from.
const POLKIT_ACTIONS_DIR: &str = "/usr/share/polkit-1/actions/";

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `bundle.linux.deb.files` of `tauri.linux.conf.json`: installed path →
/// source path (relative to this crate).
fn deb_files() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(crate_dir().join("tauri.linux.conf.json")).unwrap();
    let config: serde_json::Value = serde_json::from_str(&text).unwrap();
    config["bundle"]["linux"]["deb"]["files"]
        .as_object()
        .expect("tauri.linux.conf.json lists bundle.linux.deb.files")
        .iter()
        .map(|(installed, source)| (installed.clone(), source.as_str().unwrap().to_owned()))
        .collect()
}

#[test]
fn the_deb_installs_the_helper_at_the_path_the_server_runs() {
    assert!(Path::new(UPDATE_HELPER_PATH).is_absolute());
    let files = deb_files();
    let (_, source) = files
        .iter()
        .find(|(installed, _)| installed == UPDATE_HELPER_PATH)
        .unwrap_or_else(|| panic!("the .deb does not install {UPDATE_HELPER_PATH}: {files:?}"));
    // The release build of the helper crate, which `make desktop-build` and
    // the bundle workflow build before bundling.
    assert_eq!(source, "../../../target/release/delta-update-helper");
}

#[test]
fn the_polkit_policy_allows_exactly_the_helper_with_an_admin_password_each_time() {
    let files = deb_files();
    let policies: Vec<&(String, String)> = files
        .iter()
        .filter(|(installed, _)| installed.starts_with(POLKIT_ACTIONS_DIR))
        .collect();
    let [(installed, source)] = policies.as_slice() else {
        panic!("the .deb ships one polkit policy, not {policies:?}");
    };
    assert!(installed.ends_with(".policy"), "{installed}");
    let policy = std::fs::read_to_string(crate_dir().join(source)).unwrap();

    let exec_paths: Vec<&str> = policy
        .split(r#"<annotate key="org.freedesktop.policykit.exec.path">"#)
        .skip(1)
        .map(|rest| rest.split("</annotate>").next().unwrap().trim())
        .collect();
    assert_eq!(exec_paths, [UPDATE_HELPER_PATH]);
    assert_eq!(policy.matches("<action id=").count(), 1, "one action only");

    for key in ["allow_any", "allow_inactive", "allow_active"] {
        let value = policy
            .split(&format!("<{key}>"))
            .nth(1)
            .and_then(|rest| rest.split(&format!("</{key}>")).next())
            .unwrap_or_else(|| panic!("the policy sets no {key}"));
        assert_eq!(value.trim(), "auth_admin", "{key}");
    }
    assert!(!policy.contains("auth_admin_keep"));
    // The dialog names what the password is for.
    let message = policy
        .split("<message>")
        .nth(1)
        .and_then(|rest| rest.split("</message>").next())
        .expect("the policy sets the dialog's message");
    assert!(
        message.contains("Delta") && message.contains("update"),
        "{message}"
    );
}
