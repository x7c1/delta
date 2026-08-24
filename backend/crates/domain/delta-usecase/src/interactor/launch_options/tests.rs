//! Launch-option registry use-case tests.

use delta_model::LaunchOptionPreset;

use crate::interactor::launch_options::expand_leading_tilde;
use crate::interactor::testing::*;

/// A value of exactly `~` expands to the home directory.
#[test]
fn expand_leading_tilde_bare_tilde_becomes_home() {
    assert_eq!(expand_leading_tilde("~", Some("/home/u")), "/home/u");
}

/// A `~/`-prefixed value has its `~` replaced by home.
#[test]
fn expand_leading_tilde_prefixed_value_uses_home() {
    assert_eq!(
        expand_leading_tilde("~/repos/x", Some("/home/u")),
        "/home/u/repos/x"
    );
}

/// A trailing slash on home does not produce a double slash.
#[test]
fn expand_leading_tilde_trims_trailing_slash_on_home() {
    assert_eq!(expand_leading_tilde("~/x", Some("/home/u/")), "/home/u/x");
}

/// An absolute path is left unchanged.
#[test]
fn expand_leading_tilde_leaves_absolute_path_unchanged() {
    assert_eq!(expand_leading_tilde("/opt/p", Some("/home/u")), "/opt/p");
}

/// A plain non-path value passes through untouched.
#[test]
fn expand_leading_tilde_leaves_plain_value_unchanged() {
    assert_eq!(expand_leading_tilde("auto", Some("/home/u")), "auto");
}

/// `~user` (tilde-user, not a leading `~/`) is not expanded.
#[test]
fn expand_leading_tilde_leaves_tilde_user_unchanged() {
    assert_eq!(expand_leading_tilde("~user/x", Some("/home/u")), "~user/x");
}

/// An embedded (non-leading) `~` is not expanded.
#[test]
fn expand_leading_tilde_leaves_embedded_tilde_unchanged() {
    assert_eq!(
        expand_leading_tilde("/opt/~/x", Some("/home/u")),
        "/opt/~/x"
    );
}

/// With no home (HOME unset) the value is left as-is rather than failing.
#[test]
fn expand_leading_tilde_without_home_leaves_value_unchanged() {
    assert_eq!(expand_leading_tilde("~/x", None), "~/x");
}

/// Create then list returns the registered option with every field preserved,
/// and the list is newest-first.
#[tokio::test]
async fn create_then_list_returns_registered_options_newest_first() {
    let ix = interactor();

    let first = ix
        .create_launch_option(
            Some("plugins"),
            "--plugin-dir",
            Some("/opt/p"),
            true,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    let second = ix
        .create_launch_option(
            None,
            "--permission-mode",
            Some("auto"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();

    let listed = ix.list_launch_options().await.unwrap();
    let ids: Vec<i64> = listed.iter().map(|o| o.id).collect();
    assert_eq!(ids, vec![second.id, first.id], "newest first");

    let plugins = listed.iter().find(|o| o.id == first.id).unwrap();
    assert_eq!(plugins.label.as_deref(), Some("plugins"));
    assert_eq!(plugins.name, "--plugin-dir");
    assert_eq!(plugins.value.as_deref(), Some("/opt/p"));
    assert!(plugins.default_enabled);
    assert_eq!(plugins.provider, crate::AgentProvider::Claude);
}

/// The registry holds options for every provider (the list is not filtered
/// server-side); each option round-trips with its own provider preserved.
#[tokio::test]
async fn create_preserves_each_options_provider() {
    let ix = interactor();

    let claude = ix
        .create_launch_option(
            None,
            "--permission-mode",
            Some("auto"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    let codex = ix
        .create_launch_option(
            None,
            "model",
            Some("gpt-5"),
            false,
            crate::AgentProvider::Codex,
        )
        .await
        .unwrap();

    assert_eq!(claude.provider, crate::AgentProvider::Claude);
    assert_eq!(codex.provider, crate::AgentProvider::Codex);

    let listed = ix.list_launch_options().await.unwrap();
    assert_eq!(listed.len(), 2, "the list carries both providers' options");
    assert_eq!(
        listed.iter().find(|o| o.id == codex.id).unwrap().provider,
        crate::AgentProvider::Codex,
    );
}

/// Setting `default_enabled` toggles it in place and returns the updated row;
/// an unknown id returns `None`.
#[tokio::test]
async fn set_default_enabled_toggles_in_place() {
    let ix = interactor();
    let option = ix
        .create_launch_option(
            None,
            "--plugin-dir",
            Some("/opt/p"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert!(!option.default_enabled);

    let updated = ix
        .set_launch_option_default_enabled(option.id, true)
        .await
        .unwrap()
        .expect("an existing option");
    assert_eq!(updated.id, option.id);
    assert!(updated.default_enabled);

    let listed = ix.list_launch_options().await.unwrap();
    assert!(
        listed
            .iter()
            .find(|o| o.id == option.id)
            .unwrap()
            .default_enabled
    );

    assert!(ix
        .set_launch_option_default_enabled(9999, true)
        .await
        .unwrap()
        .is_none());
}

/// A valueless, unlabeled flag round-trips with `None` for both optionals.
#[tokio::test]
async fn create_valueless_flag_keeps_label_and_value_none() {
    let ix = interactor();
    let option = ix
        .create_launch_option(
            None,
            "--dangerously-skip-permissions",
            None,
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert_eq!(option.label, None);
    assert_eq!(option.value, None);
    assert_eq!(option.name, "--dangerously-skip-permissions");
    assert!(!option.default_enabled);
}

/// Delete removes only the named option; deleting an unknown id is a no-op.
#[tokio::test]
async fn delete_removes_only_the_named_option() {
    let ix = interactor();
    let keep = ix
        .create_launch_option(
            None,
            "--model",
            Some("opus"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    let drop = ix
        .create_launch_option(
            None,
            "--model",
            Some("sonnet"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();

    ix.delete_launch_option(drop.id).await.unwrap();
    let remaining = ix.list_launch_options().await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, keep.id);

    // Deleting an id that no longer exists is silently fine.
    ix.delete_launch_option(drop.id).await.unwrap();
    assert_eq!(ix.list_launch_options().await.unwrap().len(), 1);
}

/// A shipped preset, for the reconcile tests below.
fn preset(key: &'static str, label: &'static str, value: &'static str) -> LaunchOptionPreset {
    LaunchOptionPreset {
        key,
        label,
        name: "--model",
        value: Some(value),
        provider: crate::AgentProvider::Claude,
    }
}

/// Reconciliation materializes a declared preset that is not in the registry,
/// with `default_enabled` off: a shipped option is offered, never imposed.
#[tokio::test]
async fn reconcile_inserts_a_declared_preset_as_a_real_row() {
    let ix = interactor();
    let catalog = [preset("claude:model-opus", "Opus", "opus")];

    ix.reconcile_builtin_launch_options(&catalog).await.unwrap();

    let listed = ix.list_launch_options().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].builtin_key.as_deref(), Some("claude:model-opus"));
    assert_eq!(listed[0].label.as_deref(), Some("Opus"));
    assert_eq!(listed[0].name, "--model");
    assert_eq!(listed[0].value.as_deref(), Some("opus"));
    assert!(!listed[0].default_enabled);
}

/// Reconciliation is idempotent, ids included.
///
/// Ids matter more than row counts here: a shipped row's id can already sit in a
/// saved composer selection, so a reconcile that recreated rows would quietly
/// invalidate the user's selection on every restart.
#[tokio::test]
async fn reconcile_is_idempotent_including_ids() {
    let ix = interactor();
    let catalog = [
        preset("claude:model-opus", "Opus", "opus"),
        preset("claude:model-sonnet", "Sonnet", "sonnet"),
    ];

    ix.reconcile_builtin_launch_options(&catalog).await.unwrap();
    let first = ix.list_launch_options().await.unwrap();
    ix.reconcile_builtin_launch_options(&catalog).await.unwrap();
    let second = ix.list_launch_options().await.unwrap();

    assert_eq!(first, second);
}

/// Reconciliation preserves `default_enabled` — and only `default_enabled`.
///
/// This is the property the whole design rests on: it is what makes it safe for
/// startup to overwrite a shipped row's `label`/`name`/`value` from the catalog
/// every single time, because those three cannot be edited through the API
/// anyway while this one can.
#[tokio::test]
async fn reconcile_preserves_a_ticked_builtin() {
    let ix = interactor();
    let catalog = [preset("claude:model-opus", "Opus", "opus")];
    ix.reconcile_builtin_launch_options(&catalog).await.unwrap();
    let shipped = ix.list_launch_options().await.unwrap().remove(0);

    ix.set_launch_option_default_enabled(shipped.id, true)
        .await
        .unwrap()
        .expect("the shipped row exists");

    ix.reconcile_builtin_launch_options(&catalog).await.unwrap();

    let listed = ix.list_launch_options().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, shipped.id);
    assert!(
        listed[0].default_enabled,
        "the user's tick survives a reconcile"
    );
}

/// Reconciliation rewrites a shipped row's `label`, `name` and `value` when the
/// declared catalog changes them, in place.
#[tokio::test]
async fn reconcile_updates_declared_content_in_place() {
    let ix = interactor();
    ix.reconcile_builtin_launch_options(&[preset("claude:model-opus", "Opus", "opus")])
        .await
        .unwrap();
    let before = ix.list_launch_options().await.unwrap().remove(0);

    ix.reconcile_builtin_launch_options(&[LaunchOptionPreset {
        key: "claude:model-opus",
        label: "Opus (latest)",
        name: "--model-alias",
        value: Some("opus-latest"),
        provider: crate::AgentProvider::Claude,
    }])
    .await
    .unwrap();

    let after = ix.list_launch_options().await.unwrap();
    assert_eq!(after.len(), 1, "the row was updated, not replaced");
    assert_eq!(after[0].id, before.id);
    assert_eq!(after[0].label.as_deref(), Some("Opus (latest)"));
    assert_eq!(after[0].name, "--model-alias");
    assert_eq!(after[0].value.as_deref(), Some("opus-latest"));
}

/// A preset dropped from the catalog is retired from the registry, and the
/// user's own rows are untouched by the sweep.
#[tokio::test]
async fn reconcile_retires_an_undeclared_builtin_and_spares_user_rows() {
    let ix = interactor();
    let mine = ix
        .create_launch_option(
            Some("mine"),
            "--plugin-dir",
            Some("/opt/p"),
            true,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    ix.reconcile_builtin_launch_options(&[
        preset("claude:model-opus", "Opus", "opus"),
        preset("claude:model-sonnet", "Sonnet", "sonnet"),
    ])
    .await
    .unwrap();
    assert_eq!(ix.list_launch_options().await.unwrap().len(), 3);

    ix.reconcile_builtin_launch_options(&[preset("claude:model-opus", "Opus", "opus")])
        .await
        .unwrap();

    let listed = ix.list_launch_options().await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].builtin_key.as_deref(), Some("claude:model-opus"));
    assert_eq!(listed[1].id, mine.id, "the user's own row is out of scope");
    assert!(listed[1].default_enabled, "and keeps its flag");

    // An empty catalog retires every shipped row and still spares the user's.
    ix.reconcile_builtin_launch_options(&[]).await.unwrap();
    let listed = ix.list_launch_options().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, mine.id);
}

/// A shipped row is not the user's to delete: the refusal is
/// [`Error::LaunchOptionIsBuiltin`] and the row survives. Their own rows, and an
/// id nobody has, are unaffected.
#[tokio::test]
async fn delete_refuses_a_shipped_option() {
    let ix = interactor();
    ix.reconcile_builtin_launch_options(&[preset("claude:model-opus", "Opus", "opus")])
        .await
        .unwrap();
    let shipped = ix.list_launch_options().await.unwrap().remove(0);

    let err = ix.delete_launch_option(shipped.id).await.unwrap_err();
    assert!(
        matches!(err, crate::Error::LaunchOptionIsBuiltin(id) if id == shipped.id),
        "expected a built-in refusal naming the id, got {err:?}"
    );
    assert_eq!(ix.list_launch_options().await.unwrap().len(), 1);

    // An unknown id is still a silent no-op, not a refusal.
    ix.delete_launch_option(9999).await.unwrap();
}
