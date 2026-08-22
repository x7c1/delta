//! Prompt-template registry use-case tests.

use crate::interactor::testing::*;
use crate::Error;

/// Create then list returns the registered templates with both fields
/// preserved, oldest first — the order the picker relies on staying stable.
#[tokio::test]
async fn create_then_list_returns_registered_templates_oldest_first() {
    let ix = interactor();

    let first = ix
        .create_prompt_template("Merge and log", "Once CI is green, merge.")
        .await
        .unwrap();
    let second = ix
        .create_prompt_template("Review", "Review the diff for correctness.")
        .await
        .unwrap();

    let listed = ix.list_prompt_templates().await.unwrap();
    let ids: Vec<i64> = listed.iter().map(|t| t.id).collect();
    assert_eq!(ids, vec![first.id, second.id], "oldest first");

    let merge = listed.iter().find(|t| t.id == first.id).unwrap();
    assert_eq!(merge.label, "Merge and log");
    assert_eq!(merge.text, "Once CI is green, merge.");
    assert_eq!(
        merge.updated_at, merge.created_at,
        "a never-edited template reads as updated when it was created"
    );
}

/// A template's text is stored byte for byte: leading and trailing newlines are
/// content (a template may deliberately end with one), not noise to tidy away.
#[tokio::test]
async fn create_stores_the_text_verbatim() {
    let ix = interactor();

    let text = "\n  first line\n  second line\n";
    let created = ix.create_prompt_template("Multi", text).await.unwrap();
    assert_eq!(created.text, text);

    let listed = ix.list_prompt_templates().await.unwrap();
    assert_eq!(listed[0].text, text, "the round trip preserves it too");
}

/// A blank `label` — empty or nothing but whitespace — is refused: an unnamed
/// template is unpickable.
#[tokio::test]
async fn create_rejects_a_blank_label() {
    let ix = interactor();

    for label in ["", "   ", "\n\t "] {
        let err = ix
            .create_prompt_template(label, "some text")
            .await
            .expect_err("a blank label is refused");
        assert!(
            matches!(err, Error::InvalidPromptTemplate(_)),
            "expected InvalidPromptTemplate, got {err:?}"
        );
    }
    assert!(ix.list_prompt_templates().await.unwrap().is_empty());
}

/// A blank `text` is refused too: a template that inserts nothing is not worth
/// storing. Whitespace-only counts as blank even though whitespace is otherwise
/// preserved — the trim applies to the check, not to what is stored.
#[tokio::test]
async fn create_rejects_a_blank_text() {
    let ix = interactor();

    for text in ["", "   ", "\n\n"] {
        let err = ix
            .create_prompt_template("Label", text)
            .await
            .expect_err("a blank text is refused");
        assert!(
            matches!(err, Error::InvalidPromptTemplate(_)),
            "expected InvalidPromptTemplate, got {err:?}"
        );
    }
    assert!(ix.list_prompt_templates().await.unwrap().is_empty());
}

/// An update replaces both fields in place, keeps the id and `created_at`, and
/// re-stamps `updated_at`.
#[tokio::test]
async fn update_replaces_the_content_in_place() {
    let ix = interactor();
    let created = ix
        .create_prompt_template("Draft", "first wording")
        .await
        .unwrap();

    let updated = ix
        .update_prompt_template(created.id, "Final", "second wording\n")
        .await
        .unwrap()
        .expect("an existing template");
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.created_at, created.created_at);
    assert_eq!(updated.label, "Final");
    assert_eq!(updated.text, "second wording\n");
    assert_ne!(
        updated.updated_at, created.updated_at,
        "an edit re-stamps updated_at"
    );

    let listed = ix.list_prompt_templates().await.unwrap();
    assert_eq!(listed.len(), 1, "the edit did not create a second row");
    assert_eq!(listed[0].label, "Final");
}

/// Updating an unknown id reports absence as `None` rather than erroring, so the
/// transport can answer `404` without inventing an error variant.
#[tokio::test]
async fn update_of_an_unknown_id_is_none() {
    let ix = interactor();
    assert!(ix
        .update_prompt_template(9999, "Label", "text")
        .await
        .unwrap()
        .is_none());
}

/// An edit is held to the same non-blank rule as a create, and leaves the stored
/// row untouched when it is refused.
#[tokio::test]
async fn update_rejects_blank_content() {
    let ix = interactor();
    let created = ix
        .create_prompt_template("Draft", "first wording")
        .await
        .unwrap();

    let err = ix
        .update_prompt_template(created.id, "  ", "text")
        .await
        .expect_err("a blank label is refused");
    assert!(matches!(err, Error::InvalidPromptTemplate(_)));
    let err = ix
        .update_prompt_template(created.id, "Draft", "\t")
        .await
        .expect_err("a blank text is refused");
    assert!(matches!(err, Error::InvalidPromptTemplate(_)));

    let listed = ix.list_prompt_templates().await.unwrap();
    assert_eq!(listed[0].label, "Draft");
    assert_eq!(listed[0].text, "first wording");
}

/// Delete removes exactly one row, and deleting an unknown id is a silent no-op.
#[tokio::test]
async fn delete_removes_one_template_and_is_idempotent() {
    let ix = interactor();
    let first = ix.create_prompt_template("A", "a").await.unwrap();
    let second = ix.create_prompt_template("B", "b").await.unwrap();

    ix.delete_prompt_template(first.id).await.unwrap();
    let listed = ix.list_prompt_templates().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, second.id);

    ix.delete_prompt_template(9999).await.unwrap();
    assert_eq!(ix.list_prompt_templates().await.unwrap().len(), 1);
}
