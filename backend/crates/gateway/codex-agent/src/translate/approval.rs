//! Classifying a server → client request, and projecting the approvals Delta
//! models into neutral permission requests.

use delta_usecase::AgentPermissionRequest;

use crate::wire::ServerRequest;

use super::{
    request_id_of, string_field, ServerRequestKind, COMMAND_EXECUTION_TOOL_NAME,
    FILE_CHANGE_TOOL_NAME, METHOD_COMMAND_EXECUTION_APPROVAL, METHOD_FILE_CHANGE_APPROVAL,
};

/// Classify a server-originated request as a modeled approval or an unmodeled
/// interaction.
///
/// The fan-out is an explicit allowlist of the two approval methods whose
/// response is a binary decision (`item/commandExecution/requestApproval`,
/// `item/fileChange/requestApproval`) — matched by exact method string, not a
/// `*/requestApproval` suffix heuristic. Everything else, including
/// `item/permissions/requestApproval` (whose response is a
/// `GrantedPermissionProfile`, not a decision Delta can produce), is unsupported:
/// the adapter answers it and surfaces it, so the turn never hangs.
pub fn classify_server_request(r: &ServerRequest) -> ServerRequestKind {
    match r.method.as_str() {
        METHOD_COMMAND_EXECUTION_APPROVAL => {
            ServerRequestKind::Approval(command_execution_approval(r))
        }
        METHOD_FILE_CHANGE_APPROVAL => ServerRequestKind::Approval(file_change_approval(r)),
        _ => ServerRequestKind::Unsupported {
            method: r.method.clone(),
            detail_json: r.params.clone(),
        },
    }
}

/// The neutral permission request a command-execution approval projects to. The
/// command being run (`command`) names the tool — falling back to a stable kind
/// label when the server omits it — `itemId` is the tool-use id, and the full
/// params ride `input_json` so `cwd` / `commandActions` / the proposed amendments
/// are preserved for the UI. The `request_id` is the server request id rendered
/// as a string — the same value the adapter maps back to the verbatim wire id
/// when it answers.
fn command_execution_approval(r: &ServerRequest) -> AgentPermissionRequest {
    AgentPermissionRequest {
        request_id: request_id_of(&r.id),
        tool_name: string_field(&r.params, "command")
            .unwrap_or_else(|| COMMAND_EXECUTION_TOOL_NAME.to_owned()),
        input_json: r.params.clone(),
        tool_use_id: string_field(&r.params, "itemId"),
        // A command execution changes no files up front, so there is nothing to
        // correlate: the command itself already names what it would do.
        file_change: None,
        // Only a file-change approval asks for a write root.
        grant_root: None,
    }
}

/// The neutral permission request a file-change approval projects to. Its params
/// carry no command or file path (only `itemId` / `grantRoot` / `reason`), so a
/// stable kind label names the interaction and the full params ride `input_json`.
///
/// The paths, kinds and diffs the card actually wants are **not** here: they
/// arrived earlier, on the `item/started` for the same `itemId`. Correlating the
/// two needs per-session state, which a pure translator has none of, so this
/// leaves [`AgentPermissionRequest::file_change`] unset and the adapter completes
/// it through [`with_file_change_detail`] when it knows the item. An approval
/// whose item was never seen keeps the `None` — the deliberate fallback to the
/// input summary.
///
/// `grantRoot` is the exception that is read here and not correlated: it belongs
/// to the approval itself, not to the item, so it is lifted onto
/// [`AgentPermissionRequest::grant_root`] straight away and reaches the card
/// even when no item is ever found for the `itemId`. It is also the broadest
/// thing the request asks for — writes anywhere under that root for the rest of
/// the session, well past the files the item lists — so it is the last thing
/// that may go missing.
///
/// [`with_file_change_detail`]: super::with_file_change_detail
fn file_change_approval(r: &ServerRequest) -> AgentPermissionRequest {
    AgentPermissionRequest {
        request_id: request_id_of(&r.id),
        tool_name: FILE_CHANGE_TOOL_NAME.to_owned(),
        input_json: r.params.clone(),
        tool_use_id: string_field(&r.params, "itemId"),
        file_change: None,
        grant_root: string_field(&r.params, "grantRoot"),
    }
}
