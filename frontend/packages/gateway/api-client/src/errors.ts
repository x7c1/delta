
/**
 * Stable machine-readable error code the server may attach to an error body.
 *
 * `resume_unavailable` means a closed session cannot be resumed because its
 * local transcript file is gone, so `claude --resume` has nothing to replay.
 * Callers branch on this to keep the session closed and show a specific message
 * rather than a generic failure.
 *
 * `session_spawning` means a request named a session whose launch has not bound
 * yet. Two requests meet it: a **branch** send, which has no message to branch
 * from (a plain send to a starting session is instead accepted as a `queued` row
 * and dispatched when the launch binds), and a removal of a session that started
 * again since the list was fetched. The composer cannot compose a branch send
 * against a session with no messages, so in the browser only the navigator's
 * `Remove` reaches this code, which reports it as a session that has started
 * again and can be removed once it is closed.
 *
 * `session_open` means a removal named a session that is currently open. The
 * navigator offers `Remove` only on a closed card, so this answers a card
 * another tab reopened. Kept apart from `session_spawning` because the two ask
 * the user for different things: close it first, versus wait for it to come up.
 *
 * `session_hooks_unreachable` means a send (or a held send's release) named a
 * session open on a pane re-adopted after a restart whose agent can no longer
 * deliver hooks to the server (the session row's `hooks_unreachable`). Nothing
 * was typed: without the prompt's echo the server would interrupt the running
 * turn and type the prompt twice. The composer refuses such a session itself,
 * so callers meet this from a stale view; they refetch the session list so the
 * lost-contact notice shows, and point the user at it (use the terminal, or
 * Close the session and send again).
 *
 * `permission_not_pending` means a permission decision can no longer take
 * effect: the request was already decided, or its hook wait timed out and the
 * interactive TUI prompt owns it now. Callers branch on this to swap the
 * Allow/Deny buttons for guidance chosen by the provider's `has_terminal`
 * capability.
 *
 * `permission_decision_unsupported` means the request is still pending but this
 * session's provider has no meaning for the decision value sent — today, a
 * session-scoped allow (`allow_for_session`) against a provider whose
 * `has_allow_for_session` capability is false. Nothing was mutated, so callers
 * drop the control that produced it and leave the remaining decisions usable,
 * rather than falling back as they do for `permission_not_pending`.
 *
 * `question_not_pending` means an `AskUserQuestion` can no longer be answered
 * from the UI (already answered, its turn ended, or no live pane). Callers
 * branch on this to keep the answer-in-the-terminal fallback, unless the
 * session is closed: then the card says so and offers only Dismiss. The 409
 * also makes the browser refetch the session list, so a tab that missed the
 * close learns of it.
 *
 * `question_blocked_by_permission` means an `AskUserQuestion` answer or cancel
 * was refused because a permission request is pending in the same session:
 * its dialog sits over the question in the terminal, so no keys were sent.
 * The question is still pending, so callers keep their controls for a retry
 * once the permission is answered.
 *
 * `send_not_cancellable` means a send can no longer be cancelled: it never
 * existed, is already terminal (matched a transcript line, or already
 * cancelled), or is dispatched but its echo already arrived so the in-flight
 * turn owns it. Callers may surface the refusal to the user (e.g.
 * `SendActivityStrip` routes it through `onError` to the notification store)
 * and let the send-activity strip reconcile from the next refetch.
 *
 * `send_not_releasable` means a send is not awaiting a release: it never
 * existed, never carried the `held_at` marker, was already released, or has
 * since been cancelled. Callers surface the refusal like a refused cancel and
 * let the send-activity strip reconcile from the next refetch.
 *
 * `clone_root_duplicate` means a clone root was registered twice with the same
 * path. The Settings dialog shows an inline "already registered" hint on this
 * code instead of a generic failure toast.
 *
 * `clone_root_not_registered` means a clone was requested into a directory that
 * is not one of the registered clone roots — the root must be spelled exactly as
 * `GET /api/clone-roots` returns it, so a path that was only just typed has to
 * be registered first and cloned under the spelling that registration returned.
 *
 * `clone_dest_exists` means `<clone_root>/<repo_name>` already exists, so the
 * clone was refused with no job started (there is no fallback naming, so the way
 * past it is a different clone root).
 *
 * Both surface inline on the PR row that asked for the clone, showing the
 * server's own message; the codes keep the two cases apart from a generic
 * failure.
 *
 * `launch_option_rejected` means a launch option selected for a new session is
 * one the provider's adapter will not apply: it names a field Delta fills in
 * itself, it names the same field twice, or two selected Codex `config` rows
 * disagree about one setting inside the object they merge into. The message is
 * the only thing that says which — it names the offending field or key path —
 * so callers show it verbatim on the failed send instead of their generic
 * "could not be sent" copy.
 *
 * `worktree_outside_base`, `worktree_in_use`, `worktree_dirty` and
 * `worktree_not_registered` are the refusals of a Settings → Storage worktree
 * removal (`DELETE /api/storage/worktrees`): the path is not a directory
 * directly under the worktree base; a listed session still works in it; or,
 * without `force`, it has uncommitted or untracked changes, or git no longer
 * knows it as a worktree. The last two are what the forced removal, confirmed
 * by the user, gets past.
 *
 * `update_cli_launcher`, `update_local_build`, `update_unavailable`,
 * `update_no_newer_release` and `update_unsupported` are the refusals of
 * `POST /api/latest-release/download`: the server is the CLI's, the desktop
 * app was built locally, it cannot download, no newer release is known, or
 * the release has nothing this platform may download. The footer offers
 * Update only when `offer` is `update`, which rules out every refusal unless
 * the newer release changed in between, and shows the message of any refusal
 * as the failure's cause. `update_install_unsupported` and `update_not_ready`
 * are the refusals of `POST /api/latest-release/install` beyond those (the
 * app installs nothing itself on this platform, or no verified download is
 * ready), and `update_not_installed` the refusal of
 * `POST /api/latest-release/restart` (nothing is installed yet).
 */
export type ApiErrorCode =
  | 'resume_unavailable'
  | 'session_spawning'
  | 'session_open'
  | 'session_hooks_unreachable'
  | 'permission_not_pending'
  | 'permission_decision_unsupported'
  | 'question_not_pending'
  | 'question_blocked_by_permission'
  | 'send_not_cancellable'
  | 'send_not_releasable'
  | 'clone_root_duplicate'
  | 'clone_root_not_registered'
  | 'clone_dest_exists'
  | 'open_cwd_path_not_allowed'
  | 'open_cwd_unknown_handler'
  | 'open_cwd_command_not_found'
  | 'open_cwd_spawn_failed'
  | 'launch_option_rejected'
  | 'worktree_outside_base'
  | 'worktree_in_use'
  | 'worktree_dirty'
  | 'worktree_not_registered'
  | 'erase_in_progress'
  | 'update_cli_launcher'
  | 'update_local_build'
  | 'update_unavailable'
  | 'update_no_newer_release'
  | 'update_unsupported'
  | 'update_install_unsupported'
  | 'update_not_ready'
  | 'update_not_installed';

/** An error raised when the server responds with a non-2xx status. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
    /** The server's machine-readable error code, when present. */
    readonly code?: ApiErrorCode,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

/** The parsed error payload: a human message and an optional stable code. */
interface ParsedError {
  message: string;
  code?: ApiErrorCode;
}

export async function readError(response: Response): Promise<ParsedError> {
  const text = await response.text();
  if (!text) {
    return { message: response.statusText };
  }
  try {
    const body = JSON.parse(text) as { error?: string; code?: string };
    return {
      message: body.error ?? text,
      code: body.code as ApiErrorCode | undefined,
    };
  } catch {
    return { message: text };
  }
}
