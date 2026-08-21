import { useEffect, useState } from 'react';
import { Button, Collapsible } from '@delta/ui-kit';
import { ApiError } from '@delta/api-client';
import type {
  FileChangeDetail,
  FileChangeKind,
  PermissionDecision,
} from '@delta/wire-gen';
import { useApiClient } from '../../data/apiContext';
import type { PermissionNotice as Notice } from '../../store/liveStore';

/** How much of the tool-input summary is shown before truncation. */
const SUMMARY_MAX_CHARS = 120;

/**
 * A one-line, human-first summary of a tool's input JSON: the `command` of a
 * Bash call, the `file_path`/`path`/`url` of a file/web tool — falling back to
 * the compact JSON itself — truncated to a notice-sized line.
 */
export function toolInputSummary(toolInputJson: string): string {
  let summary = toolInputJson;
  try {
    const input: unknown = JSON.parse(toolInputJson);
    if (typeof input === 'object' && input !== null) {
      const record = input as Record<string, unknown>;
      for (const key of ['command', 'file_path', 'path', 'url']) {
        if (typeof record[key] === 'string') {
          summary = record[key];
          break;
        }
      }
    }
  } catch {
    // Not JSON (should not happen); show the raw text.
  }
  return summary.length > SUMMARY_MAX_CHARS
    ? `${summary.slice(0, SUMMARY_MAX_CHARS)}…`
    : summary;
}

/**
 * The human word for each change kind. A kind the server could not classify
 * arrives as `null` and simply gets no label — the path and diff are what the
 * answer turns on, and inventing a word for an unmodeled kind would be worse
 * than saying nothing.
 */
const CHANGE_KIND_LABEL: Record<FileChangeKind, string> = {
  add: 'new file',
  update: 'edit',
  delete: 'delete',
};

/**
 * What a file-change request would do on disk: the affected paths and their
 * change kinds always visible, the provider's stated reason when it gave one,
 * and the diff behind an expand control.
 *
 * The diff is deliberately NOT inline. A single approval can carry hundreds of
 * diff lines, and the question the user is answering — "should this agent write
 * to these files?" — is usually settled by the paths alone; burying the
 * Allow/Deny buttons under a wall of patch text would make the common answer
 * harder to give, not easier.
 */
function FileChangeSummary({ detail }: { detail: FileChangeDetail }) {
  const diffs = detail.changes.filter((change) => change.diff.length > 0);
  return (
    <div
      className="flex flex-col gap-1.5"
      data-testid="permission-notice-file-change"
    >
      <ul className="flex flex-col gap-0.5">
        {detail.changes.map((change, index) => (
          <li
            key={`${change.path}-${index}`}
            className="flex items-baseline gap-1.5"
          >
            <span className="break-all font-mono text-code text-fg">
              {change.path}
            </span>
            {change.kind !== null && (
              <span className="shrink-0 text-caption text-fg-muted">
                {CHANGE_KIND_LABEL[change.kind]}
              </span>
            )}
          </li>
        ))}
      </ul>
      {detail.reason !== null && (
        <p className="text-fg-muted" data-testid="permission-notice-reason">
          {detail.reason}
        </p>
      )}
      {diffs.length > 0 && (
        <Collapsible
          summary={`Diff (${diffs.length} ${diffs.length === 1 ? 'file' : 'files'})`}
        >
          {diffs.map((change, index) => (
            <div key={`${change.path}-${index}`} className="mb-1.5 last:mb-0">
              <div className="break-all font-mono text-code text-fg-subtle">
                {change.path}
              </div>
              <pre className="whitespace-pre-wrap font-mono text-code text-fg-muted">
                {change.diff}
              </pre>
            </div>
          ))}
        </Collapsible>
      )}
    </div>
  );
}

export interface PermissionNoticeCardProps {
  notice: Notice;
  /**
   * Whether the session's provider offers an attachable terminal
   * (`ProviderCapabilities.has_terminal`), which decides what the fallback can
   * honestly tell the user — never a `provider === '…'` check. `undefined` means
   * the capability is not known: the providers query is still in flight, it
   * failed, or it does not list this session's provider; see
   * {@link HAS_TERMINAL_WHEN_UNKNOWN} for how that case is resolved.
   */
  providerHasTerminal?: boolean;
  /**
   * Whether the session's provider accepts a session-scoped allow
   * (`ProviderCapabilities.has_allow_for_session`), which decides whether the
   * "Allow for session" button is offered at all — never a `provider === '…'`
   * check. `undefined` means the capability is not known (the providers query is
   * still in flight, it failed, or it does not list this session's provider);
   * see {@link HAS_ALLOW_FOR_SESSION_WHEN_UNKNOWN}.
   */
  providerHasAllowForSession?: boolean;
  /** Open the embedded terminal (the fallback's "answer there" affordance). */
  onOpenTerminal: () => void;
  /** Dismiss the notice without deciding. */
  onDismiss: () => void;
}

/**
 * What an unknown `has_terminal` capability falls back to: `true`, i.e. today's
 * answer-in-the-terminal guidance.
 *
 * This is the safe default because of which provider each mistake hurts. A
 * terminal provider (Claude) reaches this 409 routinely — the permission hook's
 * browser-decision wait times out and the interactive prompt takes over — and
 * for it the terminal guidance is the only actionable instruction; telling that
 * user the request cannot be answered would leave a live prompt hanging. A
 * terminal-less provider (Codex) reaches the 409 only via rarer routes (a lost
 * agent connection, or a double answer from two tabs), and its capability is
 * resolved as soon as the providers query answers. Guess in favour of the
 * common, higher-cost case.
 *
 * The guess is usually short-lived but not bounded: the providers query does not
 * retry, so a `GET /api/providers` that failed keeps the capability unknown for
 * the rest of the page's life and a Codex session then reads this terminal
 * guidance until a reload. That is accepted rather than fixed here — with no
 * capability in hand there is no honest alternative to the historical default.
 */
const HAS_TERMINAL_WHEN_UNKNOWN = true;

/**
 * What an unknown `has_allow_for_session` capability falls back to: `false`,
 * i.e. no session-scoped button.
 *
 * Deliberately the OPPOSITE default to {@link HAS_TERMINAL_WHEN_UNKNOWN}, and
 * the asymmetry is the point: that flag chooses between two pieces of *advice*,
 * so guessing wrong costs a sentence of misleading text. This one gates a button
 * that performs an action, and a provider without the capability answers
 * `400 permission_decision_unsupported` when it is pressed. A control that fails
 * on click is worse than one that was never there, so the button appears only
 * where the capability is known to be present.
 *
 * The cost of guessing wrong in this direction is small and self-correcting: the
 * user answers with a plain Allow, exactly as before this button existed, and
 * the button appears as soon as the providers query resolves.
 */
const HAS_ALLOW_FOR_SESSION_WHEN_UNKNOWN = false;

/**
 * The permission notice card: tool name, an input summary, and Allow/Deny
 * buttons wired to `POST /api/permissions/{id}/decision`. A successful
 * decision needs no local cleanup — the broadcast `permission_resolved`
 * clears the store notice and unmounts this card, or re-points it at the next
 * pending request when several are outstanding.
 *
 * Like the question card, it renders INLINE at the conversation tail (not in a
 * floating overlay): the request interrupts the turn the user is reading, so
 * the Allow/Deny controls sit in the flow right where their eyes already are,
 * over the pane's own background instead of over transcript text.
 *
 * Several CAN be outstanding: a provider running tool calls in parallel raises
 * N approvals at once. The card always shows the oldest unanswered one and says
 * how many more are waiting, so answering walks the queue front to back instead
 * of leaving the rest invisible (and, in the field, unanswered forever).
 *
 * When the decision endpoint answers `409 permission_not_pending` the decision
 * can no longer take effect, and the card swaps the buttons for guidance. Which
 * guidance depends on the provider's `has_terminal` capability, because the 409
 * means different things on either side of it:
 *
 * - A terminal provider: the hook's browser-decision wait timed out, so the
 *   interactive TUI prompt owns the question now. Point at the terminal and
 *   offer to open it — the exact behavior the notice always had before
 *   decisions moved into the UI.
 * - A terminal-less provider: there is no prompt anywhere to answer. Either the
 *   request was already resolved (a second tab answered it) or the agent
 *   connection died with the dialog open. Say so, and offer only Dismiss —
 *   an "Open terminal" button would open a pane this provider does not have.
 *
 * When the request would change files and the provider said which, the card
 * shows those paths and their change kinds instead of a truncated blob of
 * request params, with the diff behind an expand control (see
 * {@link FileChangeSummary}). Without that detail — a Claude permission, a
 * command execution, or a file change the server could not correlate — it falls
 * back to summarizing the tool input, exactly as it always has. That is a
 * property of the data, not a provider test.
 *
 * A request that also asks for a write root states it on its own line, in BOTH
 * of those branches. It is the broadest thing an Allow here grants — writes
 * anywhere under that root for the rest of the session, not just the listed
 * files — so it is deliberately not folded into the path list, where it would
 * read as one more file being edited, and deliberately not hidden when the
 * change set is missing, which is when the user has least else to go on.
 *
 * A provider that accepts a session-scoped allow gets a third affirmative
 * button, which answers this request AND tells the provider to stop asking for
 * comparable ones for the rest of the session. It is the remedy for the case
 * that motivated it — a single turn raising a dozen approvals in a row, each
 * needing its own click — and it is gated on the capability, not the provider
 * name, so it appears wherever the capability is declared.
 */
export function PermissionNoticeCard({
  notice,
  providerHasTerminal,
  providerHasAllowForSession,
  onOpenTerminal,
  onDismiss,
}: PermissionNoticeCardProps) {
  const client = useApiClient();
  // posting: a decision POST is in flight (buttons disabled).
  // fallback: the decision can no longer take effect; show the guidance.
  // scopeRefused: the server rejected the session-scoped decision as one this
  //   provider cannot express — only reachable on stale capability data, and
  //   handled by retiring that one button rather than the whole card, since
  //   Allow and Deny are still perfectly good answers.
  const [posting, setPosting] = useState(false);
  const [fallback, setFallback] = useState(false);
  const [scopeRefused, setScopeRefused] = useState(false);
  const canAnswerInTerminal = providerHasTerminal ?? HAS_TERMINAL_WHEN_UNKNOWN;
  const canAllowForSession =
    providerHasAllowForSession ?? HAS_ALLOW_FOR_SESSION_WHEN_UNKNOWN;

  // A new request resets the card: the previous request's posting/fallback
  // state belongs to the previous question.
  useEffect(() => {
    setPosting(false);
    setFallback(false);
    setScopeRefused(false);
  }, [notice.requestId]);

  const decide = (decision: PermissionDecision) => {
    setPosting(true);
    client.decidePermission(notice.requestId, decision).catch((err: unknown) => {
      setPosting(false);
      if (err instanceof ApiError && err.code === 'permission_not_pending') {
        setFallback(true);
        return;
      }
      if (
        err instanceof ApiError &&
        err.code === 'permission_decision_unsupported'
      ) {
        // The capability the button was offered on is not (or no longer) true
        // for this session's provider. The request itself is untouched and
        // still pending, so drop just the control that cannot work and leave
        // the user with the answers that can.
        setScopeRefused(true);
        return;
      }
      // A transient failure: log and leave the buttons usable for a retry.
      console.error('permission decision failed', err);
    });
  };

  // How many other requests are still waiting behind this one. Shown so the
  // user knows the queue is not empty when this card clears.
  const remaining = Math.max(notice.pendingCount - 1, 0);

  // The file-change detail, if this request has one worth rendering. A detail
  // that names no file is treated as no detail at all: the block's whole job is
  // to say WHICH files are affected, so with none it could say nothing the
  // input summary does not already say.
  const fileChange =
    notice.fileChange && notice.fileChange.changes.length > 0
      ? notice.fileChange
      : undefined;

  return (
    <div
      className="flex flex-col gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-secondary"
      data-testid="permission-notice"
      role="alert"
    >
      <p className="text-caption font-medium text-warning">
        Permission requested: {notice.toolName}
        {remaining > 0 && (
          <span
            className="ml-1 font-normal text-fg-muted"
            data-testid="permission-notice-remaining"
          >
            (+{remaining} more)
          </span>
        )}
      </p>
      {fileChange ? (
        <FileChangeSummary detail={fileChange} />
      ) : (
        <p className="break-all font-mono text-code text-fg-muted">
          {toolInputSummary(notice.toolInput)}
        </p>
      )}
      {/* Outside the branch above on purpose: a write root can arrive with or
          without a change set, and it outlives this one request either way. */}
      {notice.grantRoot && (
        <p className="text-warning" data-testid="permission-notice-grant-root">
          Also asks to allow writes anywhere under{' '}
          <span className="break-all font-mono text-code">
            {notice.grantRoot}
          </span>{' '}
          for the rest of the session.
        </p>
      )}
      {fallback ? (
        canAnswerInTerminal ? (
          <>
            <p className="text-fg-muted">Answer the prompt in the terminal.</p>
            <div className="flex flex-wrap items-center gap-2">
              <Button size="sm" onClick={onOpenTerminal}>
                Open terminal
              </Button>
              <Button size="sm" variant="ghost" onClick={onDismiss}>
                Dismiss
              </Button>
            </div>
          </>
        ) : (
          <>
            <p
              className="text-fg-muted"
              data-testid="permission-notice-unanswerable"
            >
              This request can no longer be answered — it was already resolved,
              or the agent connection was lost.
            </p>
            <div className="flex flex-wrap items-center gap-2">
              <Button size="sm" variant="ghost" onClick={onDismiss}>
                Dismiss
              </Button>
            </div>
          </>
        )
      ) : (
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" disabled={posting} onClick={() => decide('allow')}>
            Allow
          </Button>
          {canAllowForSession && !scopeRefused && (
            <Button
              size="sm"
              disabled={posting}
              data-testid="permission-notice-allow-for-session"
              onClick={() => decide('allow_for_session')}
            >
              Allow for session
            </Button>
          )}
          <Button size="sm" disabled={posting} onClick={() => decide('deny')}>
            Deny
          </Button>
          <Button size="sm" variant="ghost" onClick={onDismiss}>
            Dismiss
          </Button>
        </div>
      )}
    </div>
  );
}
