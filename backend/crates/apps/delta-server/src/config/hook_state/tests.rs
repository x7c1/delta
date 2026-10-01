use super::*;

use tempfile::TempDir;

fn database_in(dir: &TempDir) -> String {
    dir.path().join("delta.db").to_string_lossy().into_owned()
}

fn state_path(dir: &TempDir) -> PathBuf {
    dir.path().join(STATE_FILE_NAME)
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
    let database = database_in(&dir);

    let mut first = HookStateFile::open_beside(&database).unwrap();
    assert_eq!(first.path(), state_path(&dir));
    let minted = first.settle_hook_secret(None).unwrap();
    assert!(minted.changed, "a minted secret is a changed endpoint");
    assert_eq!(minted.secret.len(), 64);
    assert_eq!(
        recorded_in(&state_path(&dir)).hook_secret.as_deref(),
        Some(minted.secret.as_str())
    );

    let mut second = HookStateFile::open_beside(&database).unwrap();
    let reread = second.settle_hook_secret(None).unwrap();
    assert_eq!(
        reread,
        SettledSecret {
            secret: minted.secret,
            changed: false,
        }
    );
}

#[test]
fn an_explicit_secret_wins_and_is_not_recorded() {
    let dir = TempDir::new().unwrap();
    let database = database_in(&dir);
    let recorded = HookStateFile::open_beside(&database)
        .unwrap()
        .settle_hook_secret(None)
        .unwrap()
        .secret;

    let mut state = HookStateFile::open_beside(&database).unwrap();
    let settled = state.settle_hook_secret(Some("override".into())).unwrap();
    assert_eq!(settled.secret, "override");
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
    let mut state = HookStateFile::open_beside(&database_in(&dir)).unwrap();
    let settled = state.settle_hook_secret(Some(String::new())).unwrap();
    assert_eq!(settled.secret.len(), 64);
    assert!(state_path(&dir).exists(), "the minted secret is recorded");
}

#[test]
fn deleting_the_file_rotates_the_secret() {
    let dir = TempDir::new().unwrap();
    let database = database_in(&dir);
    let first = HookStateFile::open_beside(&database)
        .unwrap()
        .settle_hook_secret(None)
        .unwrap();

    fs::remove_file(state_path(&dir)).unwrap();
    let second = HookStateFile::open_beside(&database)
        .unwrap()
        .settle_hook_secret(None)
        .unwrap();
    assert_ne!(first.secret, second.secret);
    assert!(second.changed);
}

#[test]
fn the_file_is_created_owner_only() {
    let dir = TempDir::new().unwrap();
    HookStateFile::open_beside(&database_in(&dir))
        .unwrap()
        .settle_hook_secret(None)
        .unwrap();
    assert_eq!(mode_of(&state_path(&dir)), 0o600);
}

#[test]
fn a_file_readable_by_others_is_tightened_and_still_read() {
    let dir = TempDir::new().unwrap();
    let path = state_path(&dir);
    fs::write(&path, r#"{ "hook_secret": "abc123", "port": 4321 }"#).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

    let mut state = HookStateFile::open_beside(&database_in(&dir)).unwrap();
    assert_eq!(mode_of(&path), 0o600);
    assert_eq!(state.port(), Some(4321));
    assert_eq!(state.settle_hook_secret(None).unwrap().secret, "abc123");
}

#[test]
fn recording_a_port_keeps_the_secret_and_the_mode() {
    let dir = TempDir::new().unwrap();
    let database = database_in(&dir);
    let mut state = HookStateFile::open_beside(&database).unwrap();
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
    assert_eq!(
        HookStateFile::open_beside(&database).unwrap().port(),
        Some(4321)
    );
}

#[test]
fn a_file_that_is_not_json_is_replaced_with_a_fresh_secret() {
    let dir = TempDir::new().unwrap();
    fs::write(state_path(&dir), "not json").unwrap();

    let mut state = HookStateFile::open_beside(&database_in(&dir)).unwrap();
    let settled = state.settle_hook_secret(None).unwrap();
    assert!(settled.changed);
    assert_eq!(
        recorded_in(&state_path(&dir)).hook_secret,
        Some(settled.secret)
    );
}

#[test]
fn a_secret_that_cannot_ride_a_url_and_port_zero_are_dropped() {
    let dir = TempDir::new().unwrap();
    fs::write(state_path(&dir), r#"{ "hook_secret": "a&b=c", "port": 0 }"#).unwrap();

    let mut state = HookStateFile::open_beside(&database_in(&dir)).unwrap();
    assert_eq!(state.port(), None);
    let settled = state.settle_hook_secret(None).unwrap();
    assert_ne!(settled.secret, "a&b=c");
    assert!(settled.changed);
}

#[test]
fn an_unreadable_file_is_a_read_error_naming_it() {
    let dir = TempDir::new().unwrap();
    // A directory where the file should be: present, but not readable as text.
    fs::create_dir(state_path(&dir)).unwrap();
    fs::set_permissions(state_path(&dir), fs::Permissions::from_mode(0o700)).unwrap();

    let err = HookStateFile::open_beside(&database_in(&dir)).unwrap_err();
    assert!(matches!(err, HookStateError::Read { ref path, .. } if *path == state_path(&dir)));
    assert!(err.to_string().contains(STATE_FILE_NAME));
}

#[test]
fn a_bare_database_name_puts_the_file_in_the_working_directory() {
    assert_eq!(
        path_beside("delta.db"),
        Path::new(".").join(STATE_FILE_NAME)
    );
    assert_eq!(
        path_beside("/data/app/delta.db"),
        Path::new("/data/app").join(STATE_FILE_NAME)
    );
}
