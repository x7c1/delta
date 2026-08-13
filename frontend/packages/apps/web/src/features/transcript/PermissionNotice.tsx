import { useEffect, useState } from 'react';
import { Button } from '@delta/ui-kit';
import { ApiError } from '@delta/api-client';
import type { PermissionDecision } from '@delta/wire-gen';
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
 */
export function PermissionNoticeCard({
  notice,
  providerHasTerminal,
  onOpenTerminal,
  onDismiss,
}: PermissionNoticeCardProps) {
  const client = useApiClient();
  // posting: a decision POST is in flight (buttons disabled).
  // fallback: the decision can no longer take effect; show the guidance.
  const [posting, setPosting] = useState(false);
  const [fallback, setFallback] = useState(false);
  const canAnswerInTerminal = providerHasTerminal ?? HAS_TERMINAL_WHEN_UNKNOWN;

  // A new request resets the card: the previous request's posting/fallback
  // state belongs to the previous question.
  useEffect(() => {
    setPosting(false);
    setFallback(false);
  }, [notice.requestId]);

  const decide = (decision: PermissionDecision) => {
    setPosting(true);
    client.decidePermission(notice.requestId, decision).catch((err: unknown) => {
      setPosting(false);
      if (err instanceof ApiError && err.code === 'permission_not_pending') {
        setFallback(true);
        return;
      }
      // A transient failure: log and leave the buttons usable for a retry.
      console.error('permission decision failed', err);
    });
  };

  // How many other requests are still waiting behind this one. Shown so the
  // user knows the queue is not empty when this card clears.
  const remaining = Math.max(notice.pendingCount - 1, 0);

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
      <p className="break-all font-mono text-code text-fg-muted">
        {toolInputSummary(notice.toolInput)}
      </p>
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
