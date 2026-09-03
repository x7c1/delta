//! The endpoint table: every route the server serves, declared once.
//!
//! Only the declarations live here — the form they take is the
//! `declare_endpoints!` macro next door.

use super::declare_endpoints::declare_endpoints;
use super::{Endpoint, EndpointSpec, Method};
use crate::hooks::{
    MessageDisplayPayload, PermissionRequestPayload, PermissionRequestResponse, PostToolUsePayload,
    PreToolUsePayload, SessionEndPayload, SessionStartPayload, StatusLinePayload, StopPayload,
    UserPromptSubmitPayload, UserPromptSubmitResponse,
};
use crate::rest::{
    WireCloneRepositoryRequest, WireCloneRoot, WireCloneRootsResponse, WireCreateCloneRootRequest,
    WireCreateLaunchOptionRequest, WireCreatePromptTemplateRequest, WireCreateSendRequest,
    WireGitBranchesResponse, WireGitRepoResponse, WireLaunchOption, WireLaunchOptionsResponse,
    WireMessagesResponse, WireNewSessionResponse, WireOpenCwdRequest,
    WirePermissionDecisionRequest, WirePromptTemplate, WirePromptTemplatesResponse,
    WireProvidersResponse, WirePullRequestsResponse, WireQuestionAnswerRequest,
    WireQuestionCancelRequest, WireRepositoriesResponse, WireSendResponse, WireSendsResponse,
    WireSessionsResponse, WireThreadsResponse, WireUpdateLaunchOptionRequest,
    WireUpdatePromptTemplateRequest, WireVersionResponse, WireWorkdirListResponse,
    WireWorkdirRecentResponse,
};
use crate::{WireCommsFrame, WireSessionEvent};

declare_endpoints! {
    /// Liveness probe. Answers `ok` as plain text, touching no state.
    Health: GET "/health";

    // Control plane: Claude Code HTTP hooks.

    /// Fires just before a prompt is processed. Delta matches it against the
    /// open-send FIFO to confirm a turn start, and answers with a body only
    /// when it has a locator quote to inject into that one prompt.
    HookUserPromptSubmit: POST "/hooks/user-prompt-submit",
        request = UserPromptSubmitPayload,
        response = UserPromptSubmitResponse;

    /// Fires when a response completes.
    HookStop: POST "/hooks/stop", request = StopPayload;

    /// Live assistant text streamed during generation, before the transcript is
    /// flushed. Buffered as a provisional preview and broadcast to the browser;
    /// deliberately passive, so it never mutates the TUI.
    HookMessageDisplay: POST "/hooks/message-display", request = MessageDisplayPayload;

    /// Fires for every tool call. Delta records the request (it carries the
    /// `tool_use_id` needed to resolve the notice later) and detects a subagent
    /// starting; the TUI decides allow/deny.
    HookPreToolUse: POST "/hooks/pre-tool-use", request = PreToolUsePayload;

    /// A tool call completed; used to close a subagent's running window.
    HookPostToolUse: POST "/hooks/post-tool-use", request = PostToolUsePayload;

    /// An interactive permission dialog appeared, so a human answer is
    /// genuinely pending. The response carries the browser's decision, or no
    /// body when none arrived before the deadline and the TUI prompt stands.
    HookPermissionRequest: POST "/hooks/permission-request",
        request = PermissionRequestPayload,
        response = PermissionRequestResponse;

    /// A session's TUI became ready (the launch-readiness signal): binds a
    /// fresh spawn on startup, releases the held first prompt on resume.
    HookSessionStart: POST "/hooks/session-start", request = SessionStartPayload;

    /// A session terminated; used to catch a spawn that died before binding.
    HookSessionEnd: POST "/hooks/session-end", request = SessionEndPayload;

    /// The latest Claude Code status-line snapshot (model / context-window
    /// usage / rate limits / cost), broadcast to the browser. None of this is
    /// in the transcript, so the snapshot is the only source for it. Not a
    /// hook: it is the `statusLine` command Delta injects into the session
    /// settings, which is why it posts the same way.
    HookStatusLine: POST "/hooks/status-line", request = StatusLinePayload;

    // Browser REST surface: queries and commands.

    /// The session list, open-first then newest first, paged by an opaque
    /// cursor.
    ListSessions: GET "/api/sessions", response = WireSessionsResponse;

    /// Starts a new session and reports how to reach it.
    CreateSession: POST "/api/sessions", response = WireNewSessionResponse;

    /// Opens a registered session's pane.
    OpenSession: POST "/api/sessions/{id}/open";

    /// Closes a session's pane, leaving its history intact.
    CloseSession: POST "/api/sessions/{id}/close";

    /// Interrupts the session's in-flight turn.
    InterruptSession: POST "/api/sessions/{id}/interrupt";

    /// The session's threads, in creation order (ascending `id`).
    ListThreads: GET "/api/sessions/{id}/threads", response = WireThreadsResponse;

    /// The session's sends, with the turn each one produced.
    ListSends: GET "/api/sessions/{id}/sends", response = WireSendsResponse;

    /// One thread's messages, oldest first.
    ListThreadMessages: GET "/api/threads/{id}/messages", response = WireMessagesResponse;

    /// Queues a prompt for a session.
    CreateSend: POST "/api/sends",
        request = WireCreateSendRequest,
        response = WireSendResponse;

    /// Cancels a still-queued send before it is dispatched into the pane.
    CancelSend: POST "/api/sends/{id}/cancel";

    /// Releases a held send — one recovered at boot from a dead process's
    /// dispatched state, or parked by the echo deadline — into the normal
    /// queued flow.
    ReleaseSend: POST "/api/sends/{id}/release";

    /// Answers a pending tool-permission request from the browser, waking the
    /// blocked `/hooks/permission-request` call.
    DecidePermission: POST "/api/permissions/{id}/decision",
        request = WirePermissionDecisionRequest;

    /// Answers a pending `AskUserQuestion` from the browser (keystroke
    /// injection into the session's TUI pane).
    AnswerQuestion: POST "/api/sessions/{id}/questions/{request_id}/answer",
        request = WireQuestionAnswerRequest;

    /// Cancels a pending `AskUserQuestion` from the browser (Escape injection
    /// into the session's TUI pane). The `request_id` rides in the body since a
    /// cancel carries no selection.
    CancelQuestion: POST "/api/sessions/{id}/questions/cancel",
        request = WireQuestionCancelRequest;

    /// Working-directory picker: browse one directory (read-only).
    ListWorkdir: GET "/api/workdir/list", response = WireWorkdirListResponse;

    /// Working-directory picker: the directories sessions were launched in,
    /// most recent first.
    RecentWorkdir: GET "/api/workdir/recent", response = WireWorkdirRecentResponse;

    /// Git detection for the worktree-at-start option (read-only): is the
    /// selected directory a git repo.
    WorkdirGit: GET "/api/workdir/git", response = WireGitRepoResponse;

    /// The remote branches a worktree can be based on.
    WorkdirGitBranches: GET "/api/workdir/git/branches", response = WireGitBranchesResponse;

    /// Opens a known cwd in an external tool (initially VS Code only). The
    /// registry lives in the interactor; the request takes an optional
    /// `handler` id for future disambiguation.
    OpenCwd: POST "/api/open-cwd", request = WireOpenCwdRequest;

    /// Registered repositories for the new-session Repository tab: every
    /// distinct repo Delta has launched a session under, with its known clones
    /// bundled by origin URL and ordered by recency.
    ListRepositories: GET "/api/repositories", response = WireRepositoriesResponse;

    /// Clones a repository the user has no local clone of into one of their
    /// registered clone roots, so a PR whose repository exists nowhere on this
    /// machine stops being a dead end. Answers `202` and runs the clone as a
    /// background job, which reports through the `repository_clone_completed` /
    /// `repository_clone_failed` events on `/ws`.
    CloneRepository: POST "/api/repositories/clone",
        request = WireCloneRepositoryRequest;

    /// The registered clone roots.
    ListCloneRoots: GET "/api/clone-roots", response = WireCloneRootsResponse;

    /// Registers a clone root: a directory where the user's git clones live,
    /// whose direct children every `/api/repositories` call probes for clones,
    /// surfacing ones the user has never launched a session in (the
    /// umbrella-session pattern).
    CreateCloneRoot: POST "/api/clone-roots",
        request = WireCreateCloneRootRequest,
        response = WireCloneRoot;

    /// Unregisters a clone root. The path is URL-safe base64 in the segment so
    /// its embedded `/` characters survive routing.
    DeleteCloneRoot: DELETE "/api/clone-roots/{path_b64}";

    /// Pull requests for the new-session PR tab (per lens): drives the PR
    /// search through the gh CLI gateway and tags each row with whether
    /// Delta has a local clone of the PR's repo.
    ListPullRequests: GET "/api/prs", response = WirePullRequestsResponse;

    /// Provider availability for the new-session selector: whether each
    /// provider's launch binary is present on this host, so an un-installed
    /// provider is disabled with a reason instead of failing at spawn.
    ListProviders: GET "/api/providers", response = WireProvidersResponse;

    /// The launch-option registry: the custom CLI flags (or request fields) the
    /// user can select when starting a session.
    ListLaunchOptions: GET "/api/launch-options", response = WireLaunchOptionsResponse;

    /// Registers a launch option.
    CreateLaunchOption: POST "/api/launch-options",
        request = WireCreateLaunchOptionRequest,
        response = WireLaunchOption;

    /// Updates a launch option — today, its `default_enabled` flag.
    UpdateLaunchOption: PATCH "/api/launch-options/{id}",
        request = WireUpdateLaunchOptionRequest,
        response = WireLaunchOption;

    /// Deletes a launch option.
    DeleteLaunchOption: DELETE "/api/launch-options/{id}";

    /// The prompt-template registry: the named blocks of instruction text the
    /// user inserts into the composer instead of retyping them. Global, not
    /// provider-scoped — the text is prose, so it reads the same on every
    /// provider.
    ListPromptTemplates: GET "/api/prompt-templates",
        response = WirePromptTemplatesResponse;

    /// Registers a prompt template.
    CreatePromptTemplate: POST "/api/prompt-templates",
        request = WireCreatePromptTemplateRequest,
        response = WirePromptTemplate;

    /// Replaces a prompt template's content (`label` and `text`) in place.
    UpdatePromptTemplate: PATCH "/api/prompt-templates/{id}",
        request = WireUpdatePromptTemplateRequest,
        response = WirePromptTemplate;

    /// Deletes a prompt template.
    DeletePromptTemplate: DELETE "/api/prompt-templates/{id}";

    /// The Delta workspace version for the browser footer. Pre-formatted
    /// server-side, so the browser never has to know how to render `+dev.<sha>`.
    GetVersion: GET "/api/version", response = WireVersionResponse;

    // Streams. Each upgrades to a WebSocket, so the declared response type is
    // the shape of one frame on the socket rather than a response body.

    /// The browser event stream: every session's events, multiplexed.
    SessionEventStream: GET "/ws", response = WireSessionEvent;

    /// The terminal bridge to a session's tmux pane. Carries raw terminal
    /// bytes, not JSON.
    PtyStream: GET "/pty";

    /// The comms-log stream: the JSON-RPC frames Delta exchanges with a
    /// headless provider, per session. The window a terminal-less session has
    /// instead of `/pty`.
    CommsStream: GET "/comms", response = WireCommsFrame;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::route_label;
    use std::collections::BTreeSet;

    #[test]
    fn the_table_row_and_the_marker_agree() {
        // Both come from one declaration, so this pins the macro rather than
        // the list: a table row whose method or path disagreed with its marker
        // would mean the server mounts one route while readers are told
        // another.
        let create_send = ENDPOINTS
            .iter()
            .find(|spec| spec.path == CreateSend::PATH && spec.method == CreateSend::METHOD)
            .expect("CreateSend is in the table");
        assert_eq!(create_send.request, Some("WireCreateSendRequest"));
        assert_eq!(create_send.response, Some("WireSendResponse"));

        let pty = ENDPOINTS
            .iter()
            .find(|spec| spec.path == PtyStream::PATH)
            .expect("PtyStream is in the table");
        assert_eq!(pty.request, None, "a stream of raw bytes carries no JSON");
        assert_eq!(pty.response, None);
    }

    #[test]
    fn every_route_is_declared_once() {
        // A duplicate (method, path) would make the server's coverage check
        // pass while one of the two declarations is unreachable.
        let unique: BTreeSet<_> = ENDPOINTS
            .iter()
            .map(|spec| (spec.method, spec.path))
            .collect();
        assert_eq!(
            unique.len(),
            ENDPOINTS.len(),
            "the same method and path is declared twice",
        );
    }

    #[test]
    fn every_path_is_absolute() {
        for spec in ENDPOINTS {
            assert!(
                spec.path.starts_with('/'),
                "{} is not rooted",
                route_label(spec.method, spec.path),
            );
        }
    }
}
