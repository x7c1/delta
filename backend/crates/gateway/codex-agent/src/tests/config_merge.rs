//! Merging the selected `config` options.

use super::*;

/// Two selected `config` options whose keys do not overlap add up: `config` is
/// one field holding many independent settings, so selecting the shipped preset
/// alongside a row of your own is two settings, not a contradiction.
#[test]
fn two_disjoint_config_options_merge_into_one_object() {
    let params = thread_start_params(
        "/work",
        &[
            option("config", Some(r#"{"model_reasoning_summary":"auto"}"#)),
            option("config", Some(r#"{"model_reasoning_effort":"high"}"#)),
        ],
        None,
    )
    .expect("disjoint settings are not a conflict");

    assert_eq!(
        params,
        json!({
            "cwd": "/work",
            "config": {
                "model_reasoning_summary": "auto",
                "model_reasoning_effort": "high",
            },
        })
    );
}

/// The merge is **deep**: two selections that both state keys inside one nested
/// table keep both, rather than the later table replacing the earlier one.
#[test]
fn nested_config_tables_merge_key_by_key() {
    let params = thread_start_params(
        "/work",
        &[
            option("config", Some(r#"{"tools":{"web_search":true}}"#)),
            option("config", Some(r#"{"tools":{"view_image":false}}"#)),
        ],
        None,
    )
    .expect("disjoint keys inside one table are not a conflict");

    assert_eq!(
        params["config"],
        json!({ "tools": { "web_search": true, "view_image": false } })
    );
}

/// An **empty** table states no setting, so it collides with nothing: a row
/// carrying `{"tools":{}}` — how a half-cleared copy of a shipped row looks —
/// merges with a row that does state something in that table, instead of being
/// rejected as one setting sitting inside another.
#[test]
fn an_empty_table_states_nothing_and_collides_with_nothing() {
    let params = thread_start_params(
        "/work",
        &[
            option("config", Some(r#"{"tools":{}}"#)),
            option("config", Some(r#"{"tools":{"web_search":true}}"#)),
        ],
        None,
    )
    .expect("an empty table sets nothing to disagree about");

    assert_eq!(params["config"], json!({ "tools": { "web_search": true } }));
}

/// Two `writable_roots` lists **union**, earlier selection first, with an entry
/// both rows name listed once. This is the one list setting that merges instead
/// of having to match: `writable_roots` from two machines' rows is one list of
/// roots.
#[test]
fn two_writable_roots_lists_union_without_duplicates() {
    let params = thread_start_params(
        "/work",
        &[
            option(
                "config",
                Some(r#"{"sandbox_workspace_write.writable_roots":["/a","/shared"]}"#),
            ),
            option(
                "config",
                Some(r#"{"sandbox_workspace_write.writable_roots":["/shared","/b"]}"#),
            ),
        ],
        None,
    )
    .expect("two lists union");

    assert_eq!(params["config"][GRANT_KEY], json!(["/a", "/shared", "/b"]));
}

/// The union is keyed on the *setting*, not on the spelling that named it: two
/// rows both using the nested table union exactly as two dotted rows do.
#[test]
fn two_nested_writable_roots_lists_also_union() {
    let params = thread_start_params(
        "/work",
        &[
            option(
                "config",
                Some(r#"{"sandbox_workspace_write":{"writable_roots":["/a"]}}"#),
            ),
            option(
                "config",
                Some(r#"{"sandbox_workspace_write":{"writable_roots":["/b"]}}"#),
            ),
        ],
        None,
    )
    .expect("two lists under the nested spelling union too");

    assert_eq!(
        params["config"]["sandbox_workspace_write"]["writable_roots"],
        json!(["/a", "/b"])
    );
}

/// Every *other* list is a sequence, not a set, so two differing ones are a
/// disagreement like any other — never spliced together. Concatenating an MCP
/// server's `args` would hand the launch a command line neither row asked for,
/// with no `400` and nothing in the transcript to explain it.
#[test]
fn differing_lists_under_any_other_key_are_rejected() {
    let err = thread_start_params(
        "/work",
        &[
            option(
                "config",
                Some(r#"{"mcp_servers":{"docs":{"args":["--port","1"]}}}"#),
            ),
            option(
                "config",
                Some(r#"{"mcp_servers":{"docs":{"args":["--port","2"]}}}"#),
            ),
        ],
        None,
    )
    .expect_err("an ordered list is not a set to union");

    assert!(matches!(err, UsecaseError::LaunchOptionRejected(_)));
    let message = err.to_string();
    assert!(
        message.contains("mcp_servers.docs.args"),
        "the error names the full key path, got: {message}"
    );
}

/// Two rows stating the *same* list under such a key agree, so they pass — the
/// rule outside `writable_roots` is that the lists must match, not that lists
/// are forbidden.
#[test]
fn equal_lists_under_any_other_key_are_not_a_conflict() {
    let params = thread_start_params(
        "/work",
        &[
            option(
                "config",
                Some(r#"{"mcp_servers":{"docs":{"args":["--port","1"]}}}"#),
            ),
            option(
                "config",
                Some(r#"{"mcp_servers":{"docs":{"args":["--port","1"]}}}"#),
            ),
        ],
        None,
    )
    .expect("agreeing on a list is not a disagreement");

    assert_eq!(
        params["config"]["mcp_servers"]["docs"]["args"],
        json!(["--port", "1"])
    );
}

/// Two selections agreeing on a setting are not a conflict: the same value
/// twice is one value.
#[test]
fn equal_scalars_under_one_key_are_not_a_conflict() {
    let params = thread_start_params(
        "/work",
        &[
            option("config", Some(r#"{"model_reasoning_effort":"high"}"#)),
            option(
                "config",
                Some(r#"{"model_reasoning_effort":"high","tools":{"web_search":true}}"#),
            ),
        ],
        None,
    )
    .expect("agreeing on a value is not a disagreement");

    assert_eq!(
        params["config"],
        json!({
            "model_reasoning_effort": "high",
            "tools": { "web_search": true },
        })
    );
}

/// Two different values for one setting are rejected, naming the full key path
/// — never resolved by letting the last selection win, because a typo'd
/// duplicate must surface rather than silently take effect.
#[test]
fn differing_scalars_under_one_key_are_rejected() {
    let err = thread_start_params(
        "/work",
        &[
            option("config", Some(r#"{"tools":{"web_search":true}}"#)),
            option("config", Some(r#"{"tools":{"web_search":false}}"#)),
        ],
        None,
    )
    .expect_err("one setting cannot have two values");

    assert!(matches!(err, UsecaseError::LaunchOptionRejected(_)));
    let message = err.to_string();
    assert!(
        message.contains("tools.web_search"),
        "the error names the full key path, got: {message}"
    );
}

/// A setting one selection states as a scalar and another as a table is the same
/// disagreement in a different shape, and is rejected naming both paths.
#[test]
fn a_scalar_against_a_table_under_one_key_is_rejected() {
    let err = thread_start_params(
        "/work",
        &[
            option("config", Some(r#"{"tools":"everything"}"#)),
            option("config", Some(r#"{"tools":{"web_search":true}}"#)),
        ],
        None,
    )
    .expect_err("a scalar and a table cannot both be the value of one key");

    assert!(matches!(err, UsecaseError::LaunchOptionRejected(_)));
    let message = err.to_string();
    assert!(
        message.contains("tools") && message.contains("tools.web_search"),
        "the error names both paths, got: {message}"
    );
}

/// The same setting written in Codex's two spellings — a dotted key on one side,
/// the nested table on the other — is a duplicate, not two settings, and is
/// rejected naming the path and both spellings. Delta does not normalise one
/// into the other: the dotted spelling is the one the real-Codex canary pins, so
/// whichever the user wrote is what gets sent.
#[test]
fn the_same_setting_in_both_spellings_is_rejected() {
    let err = thread_start_params(
        "/work",
        &[
            option(
                "config",
                Some(r#"{"sandbox_workspace_write.writable_roots":["/a"]}"#),
            ),
            option(
                "config",
                Some(r#"{"sandbox_workspace_write":{"writable_roots":["/b"]}}"#),
            ),
        ],
        None,
    )
    .expect_err("one setting spelled two ways is a duplicate");

    assert!(matches!(err, UsecaseError::LaunchOptionRejected(_)));
    let message = err.to_string();
    assert!(
        message.contains("sandbox_workspace_write.writable_roots"),
        "the error names the path, got: {message}"
    );
}

/// Merging needs objects to merge. A single `config` is still passed through
/// whatever it is (the server is the authority on its value), but a non-object
/// among two or more is rejected, naming the offending row by the value it
/// registered — the only thing that tells two `config` rows apart here.
#[test]
fn a_non_object_among_several_config_options_is_rejected() {
    let err = thread_start_params(
        "/work",
        &[
            option("config", Some(r#"{"tools":{"web_search":true}}"#)),
            option("config", Some("not-an-object")),
        ],
        None,
    )
    .expect_err("there is nothing to merge a bare string into");

    assert!(matches!(err, UsecaseError::LaunchOptionRejected(_)));
    let message = err.to_string();
    assert!(
        message.contains("not-an-object"),
        "the error names the offending row, got: {message}"
    );
}

/// The merge is `config`'s alone: every other duplicated field is still rejected,
/// because every other `thread/start` field holds one setting.
#[test]
fn a_duplicated_non_config_field_is_still_rejected_alongside_merged_configs() {
    let err = thread_start_params(
        "/work",
        &[
            option("config", Some(r#"{"tools":{"web_search":true}}"#)),
            option("model", Some("gpt-5.6-sol")),
            option("config", Some(r#"{"model_reasoning_effort":"high"}"#)),
            option("model", Some("o3")),
        ],
        None,
    )
    .expect_err("a duplicate `model` is ambiguous however many configs are merged");

    assert!(matches!(err, UsecaseError::LaunchOptionRejected(_)));
    let message = err.to_string();
    assert!(
        message.contains("`model` is selected more than once"),
        "the rejection is the duplicate-field one, not a disagreement between \
         the merged configs, got: {message}"
    );
}

/// Three selections fold in order, and the order is the selection order: the
/// unioned `writable_roots` come out earliest-first.
#[test]
fn three_config_options_merge_in_selection_order() {
    let params = thread_start_params(
        "/work",
        &[
            option(
                "config",
                Some(r#"{"sandbox_workspace_write.writable_roots":["/first"]}"#),
            ),
            option(
                "config",
                Some(r#"{"sandbox_workspace_write.writable_roots":["/second"]}"#),
            ),
            option(
                "config",
                Some(r#"{"sandbox_workspace_write.writable_roots":["/third"]}"#),
            ),
        ],
        None,
    )
    .expect("three lists union");

    assert_eq!(
        params["config"][GRANT_KEY],
        json!(["/first", "/second", "/third"])
    );
}
