//! Generates the frontend's `@delta/wire-gen` package from the Rust wire
//! contract.
//!
//! Writes into `frontend/packages/gateway/wire-gen/src/generated/`, or into
//! the directory given as the single optional argument (`export-ts [OUT_DIR]`):
//!
//! - `SessionEvent.ts` — the `/ws` discriminated union, exported by ts-rs from
//!   [`WireSessionEvent`].
//! - `CommsFrame.ts` — the `/comms` observability stream's frame, exported by
//!   ts-rs from [`WireCommsFrame`].
//! - `event-kinds.ts` — the `EVENT_KINDS` const listing every `kind`
//!   discriminant, derived from the same enum.
//! - One file per REST request/response shape (and each wire twin they are
//!   composed of), exported by ts-rs from the `delta_wire::rest` types.
//!
//! Run via `make gen` at the repo root. `make gen-check` (part of `make check`
//! and of CI) generates into a temporary directory and fails when it differs
//! from the files on disk, so stale bindings cannot land.

use std::fmt::Write as _;
use std::path::PathBuf;

use delta_wire::rest::{
    WireCloneRepositoryRequest, WireCloneRootsResponse, WireCreateCloneRootRequest,
    WireCreateLaunchOptionRequest, WireCreatePromptTemplateRequest, WireCreateSendRequest,
    WireDeleteSnapshotRequest, WireEraseResponse, WireErrorBody, WireGitBranchesResponse,
    WireGitRepoResponse, WireLaunchOptionsResponse, WireMessagesResponse, WireNewSessionResponse,
    WireOpenCwdRequest, WirePermissionDecisionRequest, WirePromptTemplatesResponse,
    WireProvidersResponse, WirePrunePreviewResponse, WirePruneSessionsRequest,
    WirePruneSessionsResponse, WirePullRequestsResponse, WireQuestionAnswerRequest,
    WireQuestionCancelRequest, WireRemoveWorktreeRequest, WireRepositoriesResponse,
    WireSendResponse, WireSendsResponse, WireSessionsResponse, WireStorageResponse,
    WireStorageWorktreesResponse, WireThreadsResponse, WireUpdateLaunchOptionRequest,
    WireUpdatePromptTemplateRequest, WireVersionResponse, WireWorkdirListResponse,
    WireWorkdirRecentResponse,
};
use delta_wire::{event_kinds, export_config, WireCommsFrame, WireSessionEvent};
use ts_rs::TS;

/// Where the committed generated files live, relative to this crate's
/// manifest: the default output directory.
const OUT_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../frontend/packages/gateway/wire-gen/src/generated"
);

fn main() {
    let out_dir = out_dir_from_args();
    std::fs::create_dir_all(&out_dir)
        .unwrap_or_else(|err| panic!("create {}: {err}", out_dir.display()));
    let out_dir = out_dir.as_path();
    let config = export_config().with_out_dir(out_dir);

    WireSessionEvent::export_all(&config).expect("export SessionEvent.ts");

    // The `/comms` stream: one frame shape, pulling in its direction/kind enums.
    WireCommsFrame::export_all(&config).expect("export CommsFrame.ts");

    // The REST surface: exporting each endpoint's top-level request/response
    // shape pulls in every wire twin it is composed of (Session, Thread,
    // Message, ContentBlock, Send, …) via `export_all`.
    WireSessionsResponse::export_all(&config).expect("export SessionsResponse.ts");
    WireNewSessionResponse::export_all(&config).expect("export NewSessionResponse.ts");
    WireThreadsResponse::export_all(&config).expect("export ThreadsResponse.ts");
    WireMessagesResponse::export_all(&config).expect("export MessagesResponse.ts");
    WireCreateSendRequest::export_all(&config).expect("export CreateSendRequest.ts");
    WireSendResponse::export_all(&config).expect("export SendResponse.ts");
    WireSendsResponse::export_all(&config).expect("export SendsResponse.ts");
    WireOpenCwdRequest::export_all(&config).expect("export OpenCwdRequest.ts");
    WirePermissionDecisionRequest::export_all(&config)
        .expect("export PermissionDecisionRequest.ts");
    WireQuestionAnswerRequest::export_all(&config).expect("export QuestionAnswerRequest.ts");
    WireQuestionCancelRequest::export_all(&config).expect("export QuestionCancelRequest.ts");
    WireWorkdirListResponse::export_all(&config).expect("export WorkdirListResponse.ts");
    WireWorkdirRecentResponse::export_all(&config).expect("export WorkdirRecentResponse.ts");
    WireRepositoriesResponse::export_all(&config).expect("export RepositoriesResponse.ts");
    WireCloneRootsResponse::export_all(&config).expect("export CloneRootsResponse.ts");
    WireCreateCloneRootRequest::export_all(&config).expect("export CreateCloneRootRequest.ts");
    WireCloneRepositoryRequest::export_all(&config).expect("export CloneRepositoryRequest.ts");
    WirePullRequestsResponse::export_all(&config).expect("export PullRequestsResponse.ts");
    WireProvidersResponse::export_all(&config).expect("export ProvidersResponse.ts");
    WireGitRepoResponse::export_all(&config).expect("export GitRepoResponse.ts");
    WireGitBranchesResponse::export_all(&config).expect("export GitBranchesResponse.ts");
    WireLaunchOptionsResponse::export_all(&config).expect("export LaunchOptionsResponse.ts");
    WireCreateLaunchOptionRequest::export_all(&config)
        .expect("export CreateLaunchOptionRequest.ts");
    WireUpdateLaunchOptionRequest::export_all(&config)
        .expect("export UpdateLaunchOptionRequest.ts");
    WirePromptTemplatesResponse::export_all(&config).expect("export PromptTemplatesResponse.ts");
    WireCreatePromptTemplateRequest::export_all(&config)
        .expect("export CreatePromptTemplateRequest.ts");
    WireUpdatePromptTemplateRequest::export_all(&config)
        .expect("export UpdatePromptTemplateRequest.ts");
    WireErrorBody::export_all(&config).expect("export ErrorBody.ts");
    WireVersionResponse::export_all(&config).expect("export VersionResponse.ts");
    WireStorageResponse::export_all(&config).expect("export StorageResponse.ts");
    WireStorageWorktreesResponse::export_all(&config).expect("export StorageWorktreesResponse.ts");
    WireRemoveWorktreeRequest::export_all(&config).expect("export RemoveWorktreeRequest.ts");
    WireDeleteSnapshotRequest::export_all(&config).expect("export DeleteSnapshotRequest.ts");
    WirePruneSessionsRequest::export_all(&config).expect("export PruneSessionsRequest.ts");
    WirePruneSessionsResponse::export_all(&config).expect("export PruneSessionsResponse.ts");
    WireEraseResponse::export_all(&config).expect("export EraseResponse.ts");
    WirePrunePreviewResponse::export_all(&config).expect("export PrunePreviewResponse.ts");

    let event_kinds_path = out_dir.join("event-kinds.ts");
    std::fs::write(&event_kinds_path, render_event_kinds())
        .unwrap_or_else(|err| panic!("write {}: {err}", event_kinds_path.display()));

    println!("generated TypeScript bindings in {}", out_dir.display());
}

/// The output directory: the one optional argument, or [`OUT_DIR`].
///
/// Exits with a usage message on anything else, so a mistyped flag is not
/// silently taken as a directory name.
fn out_dir_from_args() -> PathBuf {
    let mut args = std::env::args_os().skip(1);
    let out_dir = match args.next() {
        None => PathBuf::from(OUT_DIR),
        Some(arg) if arg.to_string_lossy().starts_with('-') => usage(),
        Some(arg) => PathBuf::from(arg),
    };
    if args.next().is_some() {
        usage();
    }
    out_dir
}

fn usage() -> ! {
    eprintln!(
        "usage: export-ts [OUT_DIR]\n\n\
         Writes the TypeScript wire bindings into OUT_DIR (default: the \
         committed frontend/packages/gateway/wire-gen/src/generated)."
    );
    std::process::exit(2);
}

/// Renders `event-kinds.ts`: the `EVENT_KINDS` const and the kind union type.
///
/// The `satisfies` clause makes the TypeScript compiler reject any entry that
/// is not a `kind` of the generated `SessionEvent` union, so the two generated
/// files cross-check each other.
fn render_event_kinds() -> String {
    let mut out = String::from(
        "// This file was generated by delta-wire's `export-ts` binary. \
         Do not edit this file manually.\n\
         \n\
         import type { SessionEvent } from './SessionEvent';\n\
         \n\
         /** Every `kind` discriminant the server can put on the `/ws` stream. */\n\
         export const EVENT_KINDS = [\n",
    );
    for kind in event_kinds() {
        writeln!(out, "  '{kind}',").expect("write to string");
    }
    out.push_str(
        "] as const satisfies readonly SessionEvent['kind'][];\n\
         \n\
         export type SessionEventKind = (typeof EVENT_KINDS)[number];\n",
    );
    out
}
