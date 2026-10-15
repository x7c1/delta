import { useState } from 'react';
import { Button, cn } from '@delta/ui-kit';
import { ApiError } from '@delta/api-client';
import type { PermissionDecision } from '@delta/wire-gen';
import { useApiClient } from '../../data/apiContext';
import type { PermissionNotice as Notice } from '../../store/liveStore';
import { escapeInvisible, headingText } from './permissionText';
import {
  FileChangeSummary,
  PermissionArguments,
  requestHasInvisible,
} from './PermissionRequestDetail';
import { useRequestSwitchGuard } from './useRequestSwitchGuard';

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
  /**
   * Whether the session is closed. A closed session's agent is gone and its
   * pane with it, so a `409` there means the close settled the request, not
   * that the terminal prompt took it over — the fallback must not point at a
   * terminal that no longer exists. Resolved by the pane that hosts the card,
   * which already knows the focused session's state; `false` when omitted.
   */
  sessionClosed?: boolean;
  /**
   * Called when a decision comes back `409 permission_not_pending`. The host
   * refreshes whatever {@link sessionClosed} is derived from: a tab whose live
   * connection went silently half-open never heard `session_closed`, so its
   * idea that the session is open may be stale, and the 409 is the first sign.
   * Once the refetch says the session is closed, the card shows the
   * closed-session copy instead of pointing at a terminal that no longer exists.
   */
  onNotPending?: () => void;
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
 * The permission notice card: tool name, every argument of the request, and
 * Allow/Deny buttons wired to `POST /api/permissions/{id}/decision`. A successful
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
 * Both of those assume the session is still open. A closed one gets neither:
 * closing it denied every request still pending, and whatever terminal it had
 * went with it, so the card says the session was closed and offers only
 * Dismiss, whichever provider it runs. The `permission_resolved` that arrives
 * with the close normally removes the card before anyone clicks. A reconnect
 * the tab notices removes it too: the resync drops the permission notice and
 * re-seeds it from the server, which has nothing pending for a closed session.
 * So this branch is what a tab sees when it missed that `permission_resolved`
 * without noticing the gap — typically a live socket gone silently half-open.
 * The 409 itself triggers the session-list refetch that reveals the close (see
 * {@link PermissionNoticeCardProps.onNotPending}); until it lands the card shows
 * the open-session guidance, and switches once the refetched session is closed.
 *
 * The card shows what will actually run: every field of the tool input, each
 * under its name, with whitespace, line breaks, padding, length and invisible
 * characters made visible rather than trimmed (see {@link PermissionArguments}
 * and `permissionText.ts`). Anything on the card that holds an invisible or
 * direction-changing character — including the heading, where a Codex command
 * is the tool name — shows it as an escape, and a warning line says so.
 *
 * When the request would change files and says how — a correlated Codex patch,
 * or a Claude Write / Edit / MultiEdit / NotebookEdit whose input carries the
 * change — the card also shows those paths and their change kinds, with the
 * diff behind an expand control (see {@link FileChangeSummary}). That is a
 * property of the data, not a provider test.
 *
 * A request that also asks for a write root states it on its own line, with or
 * without a change set. It is the broadest thing an Allow here grants — writes
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
 *
 * Answering one request can promote the next into the same place, so when the
 * card switches to a different request its buttons stay disabled for a moment
 * and the card marks the change (see {@link useRequestSwitchGuard}): a quick
 * second click cannot answer a request the user has not seen.
 */
export function PermissionNoticeCard({
  notice,
  providerHasTerminal,
  providerHasAllowForSession,
  sessionClosed = false,
  onNotPending,
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

  const { guarded, switched } = useRequestSwitchGuard(notice.requestId);
  // A new request resets the card: the previous request's posting/fallback
  // state belongs to the previous question. Reset while rendering, together
  // with the switch guard, so no frame shows the old state on the new request.
  if (switched) {
    setPosting(false);
    setFallback(false);
    setScopeRefused(false);
  }
  const buttonsDisabled = posting || guarded;

  const decide = (decision: PermissionDecision) => {
    if (buttonsDisabled) {
      return;
    }
    setPosting(true);
    client.decidePermission(notice.requestId, decision).catch((err: unknown) => {
      setPosting(false);
      if (err instanceof ApiError && err.code === 'permission_not_pending') {
        setFallback(true);
        onNotPending?.();
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
  // arguments do not already say.
  const fileChange =
    notice.fileChange && notice.fileChange.changes.length > 0
      ? notice.fileChange
      : undefined;

  return (
    <div
      className={cn(
        'flex flex-col gap-2 rounded-md border bg-warning/10 px-3 py-2 text-secondary transition-shadow',
        guarded ? 'border-warning ring-2 ring-warning/60' : 'border-warning/30',
      )}
      data-testid="permission-notice"
      role="alert"
    >
      <p className="break-all text-caption font-medium text-warning">
        {guarded && (
          <span
            className="mr-1.5 rounded-sm bg-warning/20 px-1 font-semibold"
            data-testid="permission-notice-new-request"
          >
            New request
          </span>
        )}
        Permission requested:{' '}
        <span data-testid="permission-notice-tool-name">
          {headingText(notice.toolName)}
        </span>
        {remaining > 0 && (
          <span
            className="ml-1 font-normal text-fg-muted"
            data-testid="permission-notice-remaining"
          >
            (+{remaining} more)
          </span>
        )}
      </p>
      {requestHasInvisible(notice) && (
        <p
          className="text-caption text-warning"
          data-testid="permission-notice-invisible-warning"
        >
          This request contains invisible or direction-changing characters.
          They are shown as escapes such as {'\\u{202E}'}.
        </p>
      )}
      {fileChange && <FileChangeSummary detail={fileChange} />}
      <PermissionArguments toolInput={notice.toolInput} />
      {/* Separate from the change set on purpose: a write root can arrive with
          or without one, and it outlives this one request either way. */}
      {notice.grantRoot && (
        <p className="text-warning" data-testid="permission-notice-grant-root">
          Also asks to allow writes anywhere under{' '}
          <span className="break-all font-mono text-code">
            {escapeInvisible(notice.grantRoot)}
          </span>{' '}
          for the rest of the session.
        </p>
      )}
      {fallback ? (
        sessionClosed ? (
          <>
            <p
              className="text-fg-muted"
              data-testid="permission-notice-session-closed"
            >
              This request can no longer be answered — the session was closed.
            </p>
            <div className="flex flex-wrap items-center gap-2">
              <Button size="sm" variant="ghost" onClick={onDismiss}>
                Dismiss
              </Button>
            </div>
          </>
        ) : canAnswerInTerminal ? (
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
          <Button
            size="sm"
            disabled={buttonsDisabled}
            onClick={() => decide('allow')}
          >
            Allow
          </Button>
          {canAllowForSession && !scopeRefused && (
            <Button
              size="sm"
              disabled={buttonsDisabled}
              data-testid="permission-notice-allow-for-session"
              onClick={() => decide('allow_for_session')}
            >
              Allow for session
            </Button>
          )}
          <Button
            size="sm"
            disabled={buttonsDisabled}
            onClick={() => decide('deny')}
          >
            Deny
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={guarded}
            onClick={onDismiss}
          >
            Dismiss
          </Button>
        </div>
      )}
    </div>
  );
}
