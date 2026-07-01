//! Launch-option registry use-case tests.

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
        .create_launch_option(Some("plugins"), "--plugin-dir", Some("/opt/p"), true)
        .await
        .unwrap();
    let second = ix
        .create_launch_option(None, "--permission-mode", Some("auto"), false)
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
}

/// Setting `default_enabled` toggles it in place and returns the updated row;
/// an unknown id returns `None`.
#[tokio::test]
async fn set_default_enabled_toggles_in_place() {
    let ix = interactor();
    let option = ix
        .create_launch_option(None, "--plugin-dir", Some("/opt/p"), false)
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
        .create_launch_option(None, "--dangerously-skip-permissions", None, false)
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
        .create_launch_option(None, "--model", Some("opus"), false)
        .await
        .unwrap();
    let drop = ix
        .create_launch_option(None, "--model", Some("sonnet"), false)
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
