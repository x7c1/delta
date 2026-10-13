//! Launch options → `thread/start` fields.

use super::*;

/// The mapping rule: a launch option's `name` is the `thread/start` field and
/// its `value` is that field's value, passed through with no allowlist. A value
/// that is not valid JSON is the string it looks like; one that parses keeps its
/// real type (so a boolean/object/number field works); a valueless option is the
/// bare boolean `true`.
#[test]
fn launch_options_map_onto_thread_start_fields_by_name() {
    let params = thread_start_params(
        "/work",
        &[
            option("model", Some("gpt-5.6-sol")),
            option("sandbox", Some("read-only")),
            option("ephemeral", None),
            option("config", Some(r#"{"tools":{"web_search":true}}"#)),
            option("approvalPolicy", Some(r#"{"granular":{"edits":"never"}}"#)),
        ],
        None,
    )
    .expect("no delta-owned key is used");

    assert_eq!(
        params,
        json!({
            // Delta's own field is always present.
            "cwd": "/work",
            // Not valid JSON → the string itself.
            "model": "gpt-5.6-sol",
            "sandbox": "read-only",
            // Valueless → the field is switched on.
            "ephemeral": true,
            // Valid JSON → the parsed value, with its real type.
            "config": { "tools": { "web_search": true } },
            "approvalPolicy": { "granular": { "edits": "never" } },
        })
    );
}

/// A value that happens to parse as a JSON scalar keeps that type, and a quoted
/// value is the escape hatch for wanting the literal string back.
#[test]
fn a_json_scalar_value_keeps_its_type_and_a_quoted_value_stays_a_string() {
    let params = thread_start_params(
        "/work",
        &[
            option("ephemeral", Some("false")),
            option("serviceTier", Some(r#""5""#)),
        ],
        None,
    )
    .expect("no delta-owned key is used");

    assert_eq!(params["ephemeral"], json!(false));
    assert_eq!(params["serviceTier"], json!("5"));
}

/// A launch option naming a field Delta fills in itself is rejected, naming the
/// offending key — never silently dropped, and never allowed to overwrite the
/// value Delta recorded the session against.
#[test]
fn a_launch_option_naming_a_delta_owned_field_is_rejected() {
    let err = thread_start_params("/work", &[option("cwd", Some("/somewhere/else"))], None)
        .expect_err("cwd is Delta's to set");

    let message = err.to_string();
    assert!(
        message.contains("cwd"),
        "the error names the offending key, got: {message}"
    );
}

/// Two selected options naming the same field are rejected rather than one
/// silently winning: a JSON field, unlike a repeatable CLI flag, can only be set
/// once.
#[test]
fn two_launch_options_naming_the_same_field_are_rejected() {
    let err = thread_start_params(
        "/work",
        &[
            option("model", Some("gpt-5.6-sol")),
            option("model", Some("o3")),
        ],
        None,
    )
    .expect_err("a duplicate field is ambiguous");

    let message = err.to_string();
    assert!(
        message.contains("model"),
        "the error names the duplicated key, got: {message}"
    );
}

/// With nothing selected the launch is byte-identical to what it was before
/// launch options existed: `cwd` alone.
#[test]
fn no_launch_options_leaves_thread_start_params_as_just_the_workdir() {
    let params = thread_start_params("/work", &[], None).expect("no options to reject");
    assert_eq!(params, json!({ "cwd": "/work" }));
}
