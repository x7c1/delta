use super::*;

use tempfile::TempDir;

fn state_path(dir: &TempDir) -> PathBuf {
    delta_bootstrap::DataLayout::new(dir.path()).hook_state()
}

fn open_in(dir: &TempDir) -> HookStateFile {
    HookStateFile::open(state_path(dir)).unwrap()
}

fn mode_of(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn recorded_in(path: &Path) -> Recorded {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn a_missing_file_mints_and_records_a_secret_read_back_on_the_next_start() {
    let dir = TempDir::new().unwrap();

    let mut first = open_in(&dir);
    assert_eq!(first.path(), state_path(&dir));
    let minted = first.settle_hook_secret(None).unwrap();
    assert!(minted.changed, "a minted secret is a changed endpoint");
    assert_eq!(minted.secret.len(), 64);
    assert_eq!(
        recorded_in(&state_path(&dir)).hook_secret.as_deref(),
        Some(minted.secret.as_str())
    );

    let mut second = open_in(&dir);
    let reread = second.settle_hook_secret(None).unwrap();
    assert_eq!(
        reread,
        SettledSecret {
            secret: minted.secret,
            changed: false,
        }
    );
}

/// A well-formed secret other than any Delta minted here.
const OVERRIDE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn an_explicit_secret_wins_and_is_not_recorded() {
    let dir = TempDir::new().unwrap();
    let recorded = open_in(&dir).settle_hook_secret(None).unwrap().secret;

    let mut state = open_in(&dir);
    let settled = state.settle_hook_secret(Some(OVERRIDE.into())).unwrap();
    assert_eq!(settled.secret, OVERRIDE);
    assert!(settled.changed, "differs from the recorded secret");
    assert_eq!(
        recorded_in(&state_path(&dir)).hook_secret,
        Some(recorded.clone()),
        "the recorded secret is left for a start without the override"
    );

    let same = state.settle_hook_secret(Some(recorded.clone())).unwrap();
    assert!(!same.changed, "matches the recorded secret");
}

#[test]
fn an_empty_explicit_secret_is_ignored() {
    let dir = TempDir::new().unwrap();
    let mut state = open_in(&dir);
    let settled = state.settle_hook_secret(Some(String::new())).unwrap();
    assert_eq!(settled.secret.len(), 64);
    assert!(state_path(&dir).exists(), "the minted secret is recorded");
}

#[test]
fn deleting_the_file_rotates_the_secret() {
    let dir = TempDir::new().unwrap();
    let first = open_in(&dir).settle_hook_secret(None).unwrap();

    fs::remove_file(state_path(&dir)).unwrap();
    let second = open_in(&dir).settle_hook_secret(None).unwrap();
    assert_ne!(first.secret, second.secret);
    assert!(second.changed);
}

#[test]
fn the_file_is_created_owner_only() {
    let dir = TempDir::new().unwrap();
    open_in(&dir).settle_hook_secret(None).unwrap();
    assert_eq!(mode_of(&state_path(&dir)), 0o600);
}

#[test]
fn a_file_readable_by_others_is_tightened_and_still_read() {
    let dir = TempDir::new().unwrap();
    let path = state_path(&dir);
    fs::write(
        &path,
        format!(r#"{{ "hook_secret": "{OVERRIDE}", "port": 4321 }}"#),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

    let mut state = open_in(&dir);
    assert_eq!(mode_of(&path), 0o600);
    assert_eq!(state.port(), Some(4321));
    assert_eq!(state.settle_hook_secret(None).unwrap().secret, OVERRIDE);
}

#[test]
fn recording_a_port_keeps_the_secret_and_the_mode() {
    let dir = TempDir::new().unwrap();
    let mut state = open_in(&dir);
    let secret = state.settle_hook_secret(None).unwrap().secret;

    state.record_port(4321).unwrap();

    assert_eq!(
        recorded_in(&state_path(&dir)),
        Recorded {
            hook_secret: Some(secret),
            port: Some(4321),
        }
    );
    assert_eq!(mode_of(&state_path(&dir)), 0o600);
    assert_eq!(open_in(&dir).port(), Some(4321));
}

#[test]
fn a_file_that_is_not_json_is_replaced_with_a_fresh_secret() {
    let dir = TempDir::new().unwrap();
    fs::write(state_path(&dir), "not json").unwrap();

    let mut state = open_in(&dir);
    let settled = state.settle_hook_secret(None).unwrap();
    assert!(settled.changed);
    assert_eq!(
        recorded_in(&state_path(&dir)).hook_secret,
        Some(settled.secret)
    );
}

#[test]
fn port_zero_is_dropped() {
    let dir = TempDir::new().unwrap();
    fs::write(
        state_path(&dir),
        format!(r#"{{ "hook_secret": "{OVERRIDE}", "port": 0 }}"#),
    )
    .unwrap();

    assert_eq!(open_in(&dir).port(), None);
}

/// Secrets of a form Delta never mints: too short, too long, upper case, not
/// hexadecimal, or carrying characters a URL or a `curl` configuration would
/// read as syntax.
fn malformed_secrets() -> Vec<String> {
    vec![
        "abc123".to_owned(),
        format!("{OVERRIDE}0"),
        OVERRIDE.to_uppercase(),
        OVERRIDE.replace('a', "g"),
        format!("{}&b=c", &OVERRIDE[..60]),
        format!("{}\"x", &OVERRIDE[..62]),
    ]
}

#[test]
fn a_malformed_recorded_secret_is_refused_naming_the_file() {
    for secret in malformed_secrets() {
        let dir = TempDir::new().unwrap();
        let recorded = serde_json::json!({ "hook_secret": secret }).to_string();
        fs::write(state_path(&dir), &recorded).unwrap();

        let err = open_in(&dir).settle_hook_secret(None).unwrap_err();

        assert!(
            matches!(err, HookStateError::MalformedRecordedSecret { ref path } if *path == state_path(&dir)),
            "{secret:?}: {err:?}"
        );
        assert!(err.to_string().contains("delta-hook-state.json"), "{err}");
        assert_eq!(
            fs::read_to_string(state_path(&dir)).unwrap(),
            recorded,
            "the file is left for the user to inspect or delete"
        );
    }
}

#[test]
fn a_malformed_explicit_secret_is_refused_naming_the_variable() {
    for secret in malformed_secrets() {
        let dir = TempDir::new().unwrap();

        let err = open_in(&dir)
            .settle_hook_secret(Some(secret.clone()))
            .unwrap_err();

        assert!(
            matches!(err, HookStateError::MalformedExplicitSecret),
            "{secret:?}: {err:?}"
        );
        assert!(err.to_string().contains("DELTA_HOOK_SECRET"), "{err}");
    }
}

#[test]
fn an_unreadable_file_is_a_read_error_naming_it() {
    let dir = TempDir::new().unwrap();
    // A directory where the file should be: present, but not readable as text.
    fs::create_dir(state_path(&dir)).unwrap();
    fs::set_permissions(state_path(&dir), fs::Permissions::from_mode(0o700)).unwrap();

    let err = HookStateFile::open(state_path(&dir)).unwrap_err();
    assert!(matches!(err, HookStateError::Read { ref path, .. } if *path == state_path(&dir)));
    assert!(err.to_string().contains("delta-hook-state.json"));
}
