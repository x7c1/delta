//! An `item/fileChange/requestApproval` carries only
//! `{ itemId, startedAtMs, threadId, turnId, grantRoot?, reason? }` — no path, no
//! kind, no diff. Everything the user needs to answer it travelled a moment
//! earlier, on the `item/started` for the same item. These cases pin the
//! correlation the adapter performs between the two, including what happens when
//! it cannot be made.

use agent_contract::launch_request;
use delta_usecase::{
    AgentAdapter, AgentEvent, AgentFileChange, AgentFileChangeKind, PermissionDecision,
    SendRequest, SessionEndReason,
};

use crate::support::{adapter_with, collect_until, turn_scenario};

/// A `fileChange` item's `changes` array, as the real server sends it.
fn file_change_item(id: &str, changes: &str) -> String {
    format!(
        r#"{{ "type": "item_started", "item": {{ "id": "{id}", "type": "fileChange",
              "status": "inProgress", "changes": [{changes}] }} }},"#
    )
}

/// One `FileUpdateChange`: its `kind` is an object tagged by `type`, not a bare
/// string.
fn change(path: &str, kind: &str, diff: &str) -> String {
    format!(r#"{{ "path": "{path}", "kind": {{ "type": "{kind}" }}, "diff": "{diff}" }}"#)
}

/// A non-blocking file-change approval naming `item_id`.
fn approval_for(item_id: &str) -> String {
    format!(
        r#"{{ "type": "request_approval", "method": "item/fileChange/requestApproval",
              "params": {{ "itemId": "{item_id}", "reason": "write access" }} }},"#
    )
}

/// The same approval, additionally asking for writes under `root` for the rest
/// of the session — the optional `grantRoot` the real params may carry.
fn approval_granting_root(item_id: &str, root: &str) -> String {
    format!(
        r#"{{ "type": "request_approval", "method": "item/fileChange/requestApproval",
              "params": {{ "itemId": "{item_id}", "reason": "write access",
                           "grantRoot": "{root}" }} }},"#
    )
}

/// Drive a scenario and return every `PermissionRequested` it raised, in order.
async fn approvals_of(scenario: &str) -> Vec<delta_usecase::AgentPermissionRequest> {
    let (adapter, _guard) = adapter_with(scenario).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "change a file".to_owned(),
            },
        )
        .await
        .expect("send");
    collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::TurnCompleted { .. })
    })
    .await
    .into_iter()
    .filter_map(|event| match event {
        AgentEvent::PermissionRequested { request } => Some(request),
        _ => None,
    })
    .collect()
}

/// The approval arrives with the paths, kinds and diffs its item announced — the
/// whole point of the correlation, since its own params carry none of them.
#[tokio::test]
async fn a_file_change_approval_carries_its_items_paths_kinds_and_diffs() {
    let scenario = turn_scenario(&format!(
        "{}{}",
        file_change_item(
            "fc1",
            &format!(
                "{},{}",
                change("src/lib.rs", "update", "@@ -1 +1 @@"),
                change("src/new.rs", "add", "+fresh")
            ),
        ),
        approval_for("fc1"),
    ));

    let approvals = approvals_of(&scenario).await;

    let [approval] = approvals.as_slice() else {
        panic!("expected exactly one approval, got {approvals:?}");
    };
    let detail = approval
        .file_change
        .as_ref()
        .expect("the approval carries its item's file-change detail");
    assert_eq!(
        detail.changes,
        vec![
            AgentFileChange {
                path: "src/lib.rs".to_owned(),
                kind: Some(AgentFileChangeKind::Update),
                diff: "@@ -1 +1 @@".to_owned(),
            },
            AgentFileChange {
                path: "src/new.rs".to_owned(),
                kind: Some(AgentFileChangeKind::Add),
                diff: "+fresh".to_owned(),
            },
        ],
    );
    assert_eq!(
        detail.reason.as_deref(),
        Some("write access"),
        "the provider's own explanation rides the detail"
    );
    assert_eq!(
        approval.grant_root, None,
        "these params ask for no write root, so none is invented"
    );
}

/// A `grantRoot` asks for something far broader than the files the item lists:
/// writes anywhere under that root for the rest of the session. It rides the
/// approval's own params, not the item, so it must reach the card whether or not
/// the change set could be correlated — and the uncorrelated case is the one it
/// matters most in, because there the card has nothing else to show.
#[tokio::test]
async fn a_grant_root_reaches_the_card_correlated_or_not() {
    let scenario = turn_scenario(&format!(
        "{}{}{}",
        file_change_item("fc1", &change("src/lib.rs", "update", "@@ -1 +1 @@")),
        approval_granting_root("fc1", "/repo"),
        approval_granting_root("never-announced", "/elsewhere"),
    ));

    let approvals = approvals_of(&scenario).await;

    let [correlated, uncorrelated] = approvals.as_slice() else {
        panic!("expected exactly two approvals, got {approvals:?}");
    };
    assert_eq!(correlated.grant_root.as_deref(), Some("/repo"));
    assert!(
        correlated.file_change.is_some(),
        "the root does not displace the change set: both are carried"
    );
    assert_eq!(
        uncorrelated.grant_root.as_deref(),
        Some("/elsewhere"),
        "the fallback keeps the broadest ask instead of hiding it"
    );
    assert_eq!(uncorrelated.file_change, None);
}

/// Two file-change items in flight at once: each approval must get ITS item's
/// changes. A correlation that crossed them would show the user one file's diff
/// while gating another's write.
#[tokio::test]
async fn concurrent_file_change_items_never_cross_correlate() {
    let scenario = turn_scenario(&format!(
        "{}{}{}{}",
        file_change_item("fc1", &change("a.rs", "update", "diff-a")),
        file_change_item("fc2", &change("b.rs", "delete", "diff-b")),
        // Answered out of order relative to the items, so passing cannot be an
        // artefact of the two arriving in step.
        approval_for("fc2"),
        approval_for("fc1"),
    ));

    let approvals = approvals_of(&scenario).await;

    let paths: Vec<Vec<String>> = approvals
        .iter()
        .map(|approval| {
            approval
                .file_change
                .as_ref()
                .expect("each approval carries its own item's detail")
                .changes
                .iter()
                .map(|change| change.path.clone())
                .collect()
        })
        .collect();
    assert_eq!(
        paths,
        vec![vec!["b.rs".to_owned()], vec!["a.rs".to_owned()]],
        "each approval shows its own item's files: {approvals:?}"
    );
}

/// `item/fileChange/patchUpdated` restates an item's whole change set. An
/// approval raised afterwards must show the revised patch, not the one
/// `item/started` announced — otherwise the user approves a diff they were never
/// shown.
#[tokio::test]
async fn a_revised_patch_replaces_the_diff_the_approval_shows() {
    let scenario = turn_scenario(&format!(
        r#"{}{{ "type": "notification", "method": "item/fileChange/patchUpdated",
                "params": {{ "itemId": "fc1", "changes": [{}] }} }},{}"#,
        file_change_item("fc1", &change("a.rs", "update", "stale")),
        change("a.rs", "update", "revised"),
        approval_for("fc1"),
    ));

    let approvals = approvals_of(&scenario).await;

    let [approval] = approvals.as_slice() else {
        panic!("expected exactly one approval, got {approvals:?}");
    };
    assert_eq!(
        approval
            .file_change
            .as_ref()
            .expect("the approval carries the item's detail")
            .changes,
        vec![AgentFileChange {
            path: "a.rs".to_owned(),
            kind: Some(AgentFileChangeKind::Update),
            diff: "revised".to_owned(),
        }],
    );
}

/// An approval whose item was never seen (a missed notification, an out-of-order
/// frame, a session resumed mid-turn) keeps NO detail. That `None` is the
/// deliberate fallback: the card renders the request's params exactly as it did
/// before this correlation existed, rather than an empty detail block.
#[tokio::test]
async fn an_approval_with_no_known_item_falls_back_to_no_detail() {
    let scenario = turn_scenario(&approval_for("never-announced"));

    let approvals = approvals_of(&scenario).await;

    let [approval] = approvals.as_slice() else {
        panic!("expected exactly one approval, got {approvals:?}");
    };
    assert_eq!(
        approval.file_change, None,
        "an uncorrelated approval invents no detail"
    );
    assert_eq!(
        approval.tool_name, "file_change",
        "it still names the interaction by its kind"
    );
    assert_eq!(
        approval.input_json.get("itemId").and_then(|id| id.as_str()),
        Some("never-announced"),
        "and its params still ride the request, which is what the card falls back to"
    );
}

/// An item that names no file at all takes the SAME fallback as an unknown one.
///
/// A detail carrying an empty `changes` array would say nothing the input
/// summary does not, while breaking the promise both wire surfaces make — that a
/// detail, when present, names the files that would change. Every client reading
/// that contract would then need an empty-state branch it was told it would never
/// need; the browser card guards against one anyway, but the guard must not be
/// what the contract rests on.
#[tokio::test]
async fn an_item_naming_no_file_falls_back_like_an_unknown_one() {
    let scenario = turn_scenario(&format!(
        "{}{}",
        file_change_item("fc1", ""),
        approval_for("fc1"),
    ));

    let approvals = approvals_of(&scenario).await;

    let [approval] = approvals.as_slice() else {
        panic!("expected exactly one approval, got {approvals:?}");
    };
    assert_eq!(
        approval.file_change, None,
        "an empty change set is no detail, not an empty one"
    );
}

/// The correlation map is live state with a lifecycle, not a log: an entry goes
/// when its item completes and the whole map goes when the turn ends. A session
/// running for hours must not accumulate every diff it ever proposed.
#[tokio::test]
async fn the_correlation_map_is_emptied_on_item_completion_and_at_turn_end() {
    // Two items; one completes, the other is still open when the turn parks on a
    // blocking approval — so the count at that moment distinguishes "the
    // completed item was forgotten" from "nothing is tracked at all".
    let scenario = format!(
        r#"{{
            "thread_id": "thr_fc_lifecycle",
            "turn": {{
                "turn_id": "turn_fc_lifecycle",
                "emit": [
                    {{ "type": "turn_started" }},
                    {}{}
                    {{ "type": "item_completed", "item": {{ "id": "fc1", "type": "fileChange",
                       "status": "completed", "changes": [{}] }} }},
                    {{ "type": "request_approval", "blocking": true,
                       "method": "item/fileChange/requestApproval",
                       "params": {{ "itemId": "fc2" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}
        }}"#,
        file_change_item("fc1", &change("a.rs", "update", "diff-a")),
        file_change_item("fc2", &change("b.rs", "update", "diff-b")),
        change("a.rs", "update", "diff-a"),
    );
    let (adapter, _guard) = adapter_with(&scenario).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "change files".to_owned(),
            },
        )
        .await
        .expect("send");

    let raised = collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::PermissionRequested { .. })
    })
    .await;
    let request_id = raised
        .iter()
        .find_map(|e| match e {
            AgentEvent::PermissionRequested { request } => Some(request.request_id.clone()),
            _ => None,
        })
        .expect("a PermissionRequested to answer");
    assert_eq!(
        adapter.tracked_file_change_items(&handle.key),
        1,
        "the completed item was forgotten; the open one is still correlated"
    );

    let adapter_dyn: &dyn AgentAdapter = &adapter;
    adapter_dyn
        .resolve_permission(&handle, &request_id, PermissionDecision::Allow)
        .await
        .expect("resolve_permission");
    collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::TurnCompleted { .. })
    })
    .await;

    assert_eq!(
        adapter.tracked_file_change_items(&handle.key),
        0,
        "the turn ending leaves nothing tracked"
    );
}

/// A lost connection releases the correlation too: the diffs are the bulky part
/// of the map, and nothing tracked can still be answered on a dead wire.
#[tokio::test]
async fn a_lost_connection_empties_the_correlation_map() {
    let scenario = format!(
        r#"{{
            "thread_id": "thr_fc_death",
            "turn": {{
                "turn_id": "turn_fc_death",
                "emit": [
                    {{ "type": "turn_started" }},
                    {}
                    {{ "type": "request_approval", "blocking": true,
                       "method": "item/fileChange/requestApproval",
                       "params": {{ "itemId": "fc1" }} }},
                    {{ "type": "exit" }}
                ]
            }}
        }}"#,
        file_change_item("fc1", &change("a.rs", "update", "diff-a")),
    );
    let (adapter, _guard) = adapter_with(&scenario).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "change a file".to_owned(),
            },
        )
        .await
        .expect("send");

    let raised = collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::PermissionRequested { .. })
    })
    .await;
    let request_id = raised
        .iter()
        .find_map(|e| match e {
            AgentEvent::PermissionRequested { request } => Some(request.request_id.clone()),
            _ => None,
        })
        .expect("a PermissionRequested to answer");
    assert_eq!(
        adapter.tracked_file_change_items(&handle.key),
        1,
        "the item is correlated while the approval is open"
    );

    // Answering releases the fake, which then exits where a real app-server
    // would die: the client sees the EOF as a lost connection.
    let adapter_dyn: &dyn AgentAdapter = &adapter;
    adapter_dyn
        .resolve_permission(&handle, &request_id, PermissionDecision::Allow)
        .await
        .expect("resolve_permission");
    collect_until(&mut stream, |e| {
        matches!(
            e,
            AgentEvent::SessionEnded {
                reason: SessionEndReason::ProcessExited
            }
        )
    })
    .await;

    assert_eq!(
        adapter.tracked_file_change_items(&handle.key),
        0,
        "the death released the tracked diffs"
    );
}
