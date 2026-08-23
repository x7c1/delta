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

/// A row created through the registry API is never flagged as Delta's own, and
/// it is readable back by id (the read the delete refusal is decided on).
#[tokio::test]
async fn a_user_registered_option_carries_no_builtin_key() {
    let store = SqliteStore::open_in_memory().unwrap();
    let option = store
        .create_launch_option(None, "--verbose", None, false, AgentProvider::Claude)
        .await
        .unwrap();
    assert_eq!(option.builtin_key, None);

    let read = store.launch_option(option.id).await.unwrap().unwrap();
    assert_eq!(read.builtin_key, None);
    assert_eq!(read.name, "--verbose");
    assert!(store.launch_option(9999).await.unwrap().is_none());
}

/// Materializing a preset inserts it with `default_enabled` **off**; declaring
/// it again updates the content the catalog owns, keeps the id, and leaves the
/// user's `default_enabled` exactly as they set it.
///
/// The preserved flag is the property the whole design rests on: it is what
/// makes it safe for startup to overwrite `label`/`name`/`value` from the
/// catalog every time.
#[tokio::test]
async fn upserting_a_builtin_updates_content_in_place_and_preserves_default_enabled() {
    let store = SqliteStore::open_in_memory().unwrap();

    let inserted = store
        .upsert_builtin_launch_option(
            "claude:model-opus",
            "Opus",
            "--model",
            Some("opus"),
            AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert_eq!(inserted.builtin_key.as_deref(), Some("claude:model-opus"));
    assert_eq!(inserted.label.as_deref(), Some("Opus"));
    assert!(
        !inserted.default_enabled,
        "a freshly materialized preset is offered, not imposed"
    );

    // The user ticks it.
    store
        .set_launch_option_default_enabled(inserted.id, true)
        .await
        .unwrap()
        .expect("an existing option");

    // The catalog re-declares it with different content.
    let updated = store
        .upsert_builtin_launch_option(
            "claude:model-opus",
            "Opus (latest)",
            "--model",
            Some("opus-latest"),
            AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert_eq!(
        updated.id, inserted.id,
        "the id survives, so a saved selection does"
    );
    assert_eq!(updated.created_at, inserted.created_at);
    assert_eq!(updated.label.as_deref(), Some("Opus (latest)"));
    assert_eq!(updated.value.as_deref(), Some("opus-latest"));
    assert!(
        updated.default_enabled,
        "the user's own flag is never overwritten by a reconcile"
    );
    assert_eq!(store.list_launch_options().await.unwrap().len(), 1);
}

/// The catalog sweep removes only the shipped rows it no longer names — never a
/// row the user registered, whatever the key set holds (including empty).
#[tokio::test]
async fn the_builtin_sweep_spares_user_rows() {
    let store = SqliteStore::open_in_memory().unwrap();
    let mine = store
        .create_launch_option(
            None,
            "--plugin-dir",
            Some("/opt/p"),
            true,
            AgentProvider::Claude,
        )
        .await
        .unwrap();
    let kept = store
        .upsert_builtin_launch_option(
            "keep",
            "Keep",
            "--model",
            Some("opus"),
            AgentProvider::Claude,
        )
        .await
        .unwrap();
    let retired = store
        .upsert_builtin_launch_option(
            "retire",
            "Retire",
            "--model",
            Some("sonnet"),
            AgentProvider::Claude,
        )
        .await
        .unwrap();

    let removed = store
        .delete_builtin_launch_options_except(&["keep"])
        .await
        .unwrap();
    assert_eq!(removed, 1);
    let ids: Vec<i64> = store
        .list_launch_options()
        .await
        .unwrap()
        .iter()
        .map(|o| o.id)
        .collect();
    assert_eq!(ids, vec![kept.id, mine.id]);
    assert!(!ids.contains(&retired.id));

    // An empty catalog retires every shipped row and still spares the user's.
    assert_eq!(
        store
            .delete_builtin_launch_options_except(&[])
            .await
            .unwrap(),
        1
    );
    let ids: Vec<i64> = store
        .list_launch_options()
        .await
        .unwrap()
        .iter()
        .map(|o| o.id)
        .collect();
    assert_eq!(ids, vec![mine.id]);
}

/// The list order: every shipped row first in declared-catalog order, then the
/// user's own newest first.
///
/// The leading block is fixed-length, so a shipped row's position never moves as
/// the user adds or removes their own.
#[tokio::test]
async fn shipped_options_lead_the_list_in_catalog_order() {
    let store = SqliteStore::open_in_memory().unwrap();
    let first_shipped = store
        .upsert_builtin_launch_option("a", "A", "--model", Some("opus"), AgentProvider::Claude)
        .await
        .unwrap();
    let older = store
        .create_launch_option(None, "--verbose", None, false, AgentProvider::Claude)
        .await
        .unwrap();
    let second_shipped = store
        .upsert_builtin_launch_option("b", "B", "--model", Some("sonnet"), AgentProvider::Claude)
        .await
        .unwrap();
    let newer = store
        .create_launch_option(None, "--debug", None, false, AgentProvider::Claude)
        .await
        .unwrap();

    let ids: Vec<i64> = store
        .list_launch_options()
        .await
        .unwrap()
        .iter()
        .map(|o| o.id)
        .collect();
    assert_eq!(
        ids,
        vec![first_shipped.id, second_shipped.id, newer.id, older.id],
        "shipped rows lead in insertion order; the user's follow newest first"
    );
}
