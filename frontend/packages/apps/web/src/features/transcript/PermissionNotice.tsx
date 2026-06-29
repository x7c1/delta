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
  /** Open the embedded terminal (the fallback's "answer there" affordance). */
  onOpenTerminal: () => void;
  /** Dismiss the notice without deciding. */
  onDismiss: () => void;
}

/**
 * The floating permission notice: tool name, an input summary, and Allow/Deny
 * buttons wired to `POST /api/permissions/{id}/decision`. A successful
 * decision needs no local cleanup — the broadcast `permission_resolved`
 * clears the store notice and unmounts this card.
 *
 * When the decision endpoint answers `409 permission_not_pending` (the hook's
 * browser-decision wait timed out, so the interactive TUI prompt owns the
 * question now), the card swaps the buttons for the answer-in-the-terminal
 * guidance — the exact behavior the notice always had before decisions moved
 * into the UI.
 */
export function PermissionNoticeCard({
  notice,
  onOpenTerminal,
  onDismiss,
}: PermissionNoticeCardProps) {
  const client = useApiClient();
  // posting: a decision POST is in flight (buttons disabled).
  // fallback: the decision can no longer take effect; show the TUI guidance.
  const [posting, setPosting] = useState(false);
  const [fallback, setFallback] = useState(false);

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

  return (
    <div
      className="pointer-events-auto absolute right-overlay-inset top-overlay-inset max-w-xs space-y-1 rounded border border-warning/30 bg-warning/10 px-2 py-1 text-xs shadow-md"
      data-testid="permission-notice"
      role="alert"
    >
      <p className="font-medium text-warning">
        Permission requested: {notice.toolName}
      </p>
      <p className="break-all font-mono text-fg-muted">
        {toolInputSummary(notice.toolInput)}
      </p>
      {fallback ? (
        <>
          <p className="text-fg-muted">Answer the prompt in the terminal.</p>
          <div className="flex gap-2">
            <Button size="sm" onClick={onOpenTerminal}>
              Open terminal
            </Button>
            <Button size="sm" variant="ghost" onClick={onDismiss}>
              Dismiss
            </Button>
          </div>
        </>
      ) : (
        <div className="flex gap-2">
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
