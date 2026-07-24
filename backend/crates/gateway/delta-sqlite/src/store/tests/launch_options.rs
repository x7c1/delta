//! Launch-option registry round trips.

use delta_model::AgentProvider;

use super::super::SqliteStore;

#[tokio::test]
async fn launch_options_round_trip_create_list_delete() {
    let store = SqliteStore::open_in_memory().unwrap();

    // A fresh store has no registered launch options.
    assert!(store.list_launch_options().await.unwrap().is_empty());

    // A flag with a label and a value persists every field, including the
    // pre-checked `default_enabled` flag.
    let plugin = store
        .create_launch_option(
            Some("My plugins"),
            "--plugin-dir",
            Some("/opt/plugins"),
            true,
            AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert_eq!(plugin.label.as_deref(), Some("My plugins"));
    assert_eq!(plugin.name, "--plugin-dir");
    assert_eq!(plugin.value.as_deref(), Some("/opt/plugins"));
    assert!(plugin.default_enabled);
    assert_eq!(plugin.provider, AgentProvider::Claude);
    assert!(!plugin.created_at.is_empty());

    // A valueless, unlabeled flag stores NULL for both — never a sentinel — and
    // `default_enabled` defaults to off.
    let valueless = store
        .create_launch_option(
            None,
            "--dangerously-skip-permissions",
            None,
            false,
            AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert_eq!(valueless.label, None);
    assert_eq!(valueless.value, None);
    assert!(!valueless.default_enabled);
    assert_ne!(valueless.id, plugin.id, "ids are distinct");

    // The persisted `default_enabled` round-trips through `list`.
    let listed_plugin = store
        .list_launch_options()
        .await
        .unwrap()
        .into_iter()
        .find(|o| o.id == plugin.id)
        .unwrap();
    assert!(listed_plugin.default_enabled);

    // The list is newest-first (descending id), so the second insert leads.
    let listed = store.list_launch_options().await.unwrap();
    let ids: Vec<i64> = listed.iter().map(|o| o.id).collect();
    assert_eq!(ids, vec![valueless.id, plugin.id]);

    // Deleting one leaves the other untouched.
    store.delete_launch_option(plugin.id).await.unwrap();
    let remaining = store.list_launch_options().await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, valueless.id);

    // Deleting an unknown id is a silent no-op (idempotent), not an error.
    store.delete_launch_option(9999).await.unwrap();
    assert_eq!(store.list_launch_options().await.unwrap().len(), 1);
}

/// A Codex option persists and reads back with its provider preserved (not
/// coerced to the Claude default), while a Claude option keeps Claude — so the
/// registry holds both providers' options and the picker can filter them.
#[tokio::test]
async fn launch_option_provider_round_trips_per_provider() {
    let store = SqliteStore::open_in_memory().unwrap();

    let claude = store
        .create_launch_option(
            None,
            "--permission-mode",
            Some("auto"),
            false,
            AgentProvider::Claude,
        )
        .await
        .unwrap();
    let codex = store
        .create_launch_option(None, "model", Some("gpt-5"), false, AgentProvider::Codex)
        .await
        .unwrap();
    assert_eq!(codex.provider, AgentProvider::Codex);

    let listed = store.list_launch_options().await.unwrap();
    assert_eq!(
        listed.iter().find(|o| o.id == claude.id).unwrap().provider,
        AgentProvider::Claude,
    );
    assert_eq!(
        listed.iter().find(|o| o.id == codex.id).unwrap().provider,
        AgentProvider::Codex,
    );
}

/// A legacy row — inserted directly with no `provider`, exercising the column's
/// `DEFAULT 'claude'` exactly as a pre-multi-provider database would — reads
/// back as `AgentProvider::Claude`, so existing installs keep working.
#[tokio::test]
async fn legacy_launch_option_row_reads_as_claude() {
    let store = SqliteStore::open_in_memory().unwrap();
    {
        let conn = store.conn.lock().await;
        conn.execute(
            "INSERT INTO launch_option (label, name, value, default_enabled, created_at)
             VALUES (NULL, '--verbose', NULL, 0, '2020-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    }

    let listed = store.list_launch_options().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "--verbose");
    assert_eq!(listed[0].provider, AgentProvider::Claude);
}

#[tokio::test]
async fn set_launch_option_default_enabled_toggles_in_place() {
    let store = SqliteStore::open_in_memory().unwrap();
    let option = store
        .create_launch_option(
            None,
            "--plugin-dir",
            Some("/opt/plugins"),
            false,
            AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert!(!option.default_enabled);

    // Toggling on returns the updated row, preserving id and created_at.
    let updated = store
        .set_launch_option_default_enabled(option.id, true)
        .await
        .unwrap()
        .expect("an existing option");
    assert_eq!(updated.id, option.id);
    assert_eq!(updated.created_at, option.created_at);
    assert!(updated.default_enabled);

    // The change persists.
    let listed = store.list_launch_options().await.unwrap();
    assert!(listed[0].default_enabled);

    // Toggling back off works too.
    let updated = store
        .set_launch_option_default_enabled(option.id, false)
        .await
        .unwrap()
        .expect("an existing option");
    assert!(!updated.default_enabled);

    // Toggling an unknown id returns None rather than erroring.
    assert!(store
        .set_launch_option_default_enabled(9999, true)
        .await
        .unwrap()
        .is_none());
}
