//! Prompt-template registry round trips.

use super::super::SqliteStore;

/// Insert a template with an explicit `created_at`, bypassing the store so a
/// test can pin the ordering the wall clock cannot resolve (`now_iso8601` has
/// second resolution, so two creates in one test would tie).
async fn insert_at(store: &SqliteStore, label: &str, text: &str, created_at: &str) -> i64 {
    let conn = store.conn.lock().await;
    conn.execute(
        "INSERT INTO prompt_template (label, text, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)",
        rusqlite::params![label, text, created_at],
    )
    .unwrap();
    conn.last_insert_rowid()
}

#[tokio::test]
async fn prompt_templates_round_trip_create_list_delete() {
    let store = SqliteStore::open_in_memory().unwrap();

    // A fresh store has no registered templates.
    assert!(store.list_prompt_templates().await.unwrap().is_empty());

    let merge = store
        .create_prompt_template("Merge and log", "Once CI is green, merge.")
        .await
        .unwrap();
    assert_eq!(merge.label, "Merge and log");
    assert_eq!(merge.text, "Once CI is green, merge.");
    assert!(!merge.created_at.is_empty());
    assert_eq!(
        merge.updated_at, merge.created_at,
        "a never-edited template reads as updated when it was created"
    );

    // A multi-line body is stored byte for byte: the leading and trailing
    // newlines are content the composer will insert, not noise to tidy away.
    let verbatim = "\nfirst line\n\nsecond paragraph\n";
    let multi = store
        .create_prompt_template("Multi", verbatim)
        .await
        .unwrap();
    assert_eq!(multi.text, verbatim);
    assert_ne!(multi.id, merge.id, "ids are distinct");

    let listed = store.list_prompt_templates().await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(
        listed.iter().find(|t| t.id == multi.id).unwrap().text,
        verbatim,
        "the round trip through SQLite preserves the whitespace"
    );

    // Deleting one leaves the other untouched.
    store.delete_prompt_template(merge.id).await.unwrap();
    let remaining = store.list_prompt_templates().await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, multi.id);

    // Deleting an unknown id is a silent no-op (idempotent), not an error.
    store.delete_prompt_template(9999).await.unwrap();
    assert_eq!(store.list_prompt_templates().await.unwrap().len(), 1);
}

/// The list is ordered by `created_at` ascending, with `id` ascending as the
/// tiebreak — registration order, so a picker's list never reshuffles under the
/// user.
#[tokio::test]
async fn prompt_templates_list_is_ordered_by_created_at_then_id() {
    let store = SqliteStore::open_in_memory().unwrap();

    // Inserted out of chronological order, and with two sharing a timestamp so
    // the id tiebreak is genuinely exercised.
    let newest = insert_at(&store, "C", "c", "2026-03-01T00:00:00Z").await;
    let tie_second = insert_at(&store, "B2", "b2", "2026-02-01T00:00:00Z").await;
    let oldest = insert_at(&store, "A", "a", "2026-01-01T00:00:00Z").await;
    let tie_first = insert_at(&store, "B1", "b1", "2026-02-01T00:00:00Z").await;

    let ids: Vec<i64> = store
        .list_prompt_templates()
        .await
        .unwrap()
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(
        ids,
        vec![oldest, tie_second, tie_first, newest],
        "created_at ascending, id ascending on the tie"
    );
}

#[tokio::test]
async fn update_prompt_template_replaces_content_and_restamps_updated_at() {
    let store = SqliteStore::open_in_memory().unwrap();
    // Seeded with an old timestamp so the re-stamp is observable: `now_iso8601`
    // has second resolution, so a create+update in one test would otherwise
    // land on the same value.
    let id = insert_at(&store, "Draft", "first wording", "2020-01-01T00:00:00Z").await;

    let updated = store
        .update_prompt_template(id, "Final", "second wording\n")
        .await
        .unwrap()
        .expect("an existing template");
    assert_eq!(updated.id, id);
    assert_eq!(updated.label, "Final");
    assert_eq!(updated.text, "second wording\n");
    assert_eq!(
        updated.created_at, "2020-01-01T00:00:00Z",
        "an edit preserves created_at"
    );
    assert_ne!(
        updated.updated_at, updated.created_at,
        "an edit re-stamps updated_at"
    );

    // The change persists, and did not create a second row.
    let listed = store.list_prompt_templates().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].label, "Final");
    assert_eq!(listed[0].text, "second wording\n");

    // Updating an unknown id returns None rather than erroring.
    assert!(store
        .update_prompt_template(9999, "Label", "text")
        .await
        .unwrap()
        .is_none());
}
