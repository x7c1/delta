import { useCallback } from 'react';
import type { Thread } from '@delta/wire-gen';
import { useQueryClient } from '@tanstack/react-query';
import { invalidateSessions } from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import { noticeOf, useLiveStore } from '../../store/liveStore';
import { PermissionNoticeCard } from './PermissionNotice';
import { QuestionCard } from './QuestionCard';

interface TranscriptTailCardsProps {
  /** The active thread, or null. */
  activeThread: Thread | null;
  /** True when the focused session is closed. */
  readOnly: boolean;
  /** True while the focused session is still starting. */
  spawning: boolean;
  /** Whether the focused session's provider offers an attachable terminal. */
  providerHasTerminal?: boolean;
  /** Whether the focused session's provider accepts a session-scoped allow. */
  providerHasAllowForSession?: boolean;
}

/**
 * The interactive cards at the conversation tail: a pending AskUserQuestion
 * and a pending permission request.
 */
export function TranscriptTailCards({
  activeThread,
  readOnly,
  spawning,
  providerHasTerminal,
  providerHasAllowForSession,
}: TranscriptTailCardsProps) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  // A `409 …_not_pending` from a permission or question card is the first sign
  // this tab's idea of the session may be stale: a live socket gone silently
  // half-open never delivers `session_closed`. `readOnly` comes from the session
  // list, so refetch it; once it says closed, the cards show their closed-session
  // copy instead of pointing at a terminal that no longer exists.
  const refetchSessions = useCallback(
    () => invalidateSessions(queryClient),
    [queryClient],
  );
  const setTerminalOpen = useNavStore((state) => state.setTerminalOpen);
  // The focused session's pending permission prompt, if any. Emitted by the
  // `PermissionRequest` hook, which fires only when an interactive dialog
  // actually appears, so it is a genuine "answer needed" signal and is shown
  // directly — no debounce. It clears on dismiss, on resolution, or when the
  // turn completes.
  const permission = useLiveStore((state) =>
    activeThread
      ? noticeOf(state.notices, activeThread.session_id, 'permission')
      : null,
  );
  const dismissPermission = useLiveStore((state) => state.dismissPermission);
  // The focused session's pending AskUserQuestion, if any. Emitted by the
  // `PreToolUse` hook for that built-in tool, so it is a genuine "answer
  // needed" signal shown directly as a readable question card. It clears on
  // dismiss, when the correlated tool_result resolves it (the user picked in
  // the terminal), or when the turn ends.
  const question = useLiveStore((state) =>
    activeThread
      ? noticeOf(state.notices, activeThread.session_id, 'question')
      : null,
  );
  const dismissQuestion = useLiveStore((state) => state.dismissQuestion);

  // The permission notice renders INLINE at the conversation tail, exactly like
  // the question card below: the request interrupts the turn the user is
  // reading, so Allow/Deny sit in the flow where their eyes already are, over
  // the pane's own background (a floating top-right card was translucent over
  // transcript text and collided with the Timeline/pane-toggle cluster pinned
  // to the same corner). Unlike the question card it is NOT gated by thread:
  // `permission_requested` carries no thread id, so the notice is session-level
  // and shows on whichever of the session's threads is active. It clears on
  // dismiss (the entry stays, flagged, so a refetch cannot resurrect the card),
  // on a decision/resolution, or when the turn completes.
  const permissionCard = permission && !permission.dismissed && activeThread && (
    <PermissionNoticeCard
      notice={permission}
      providerHasTerminal={providerHasTerminal}
      providerHasAllowForSession={providerHasAllowForSession}
      // `readOnly` alone also covers a session that is still starting, which
      // was never closed; see `spawning`.
      sessionClosed={readOnly && !spawning}
      onNotPending={refetchSessions}
      onOpenTerminal={() => setTerminalOpen(true)}
      onDismiss={() => dismissPermission(activeThread.session_id)}
    />
  );

  // The interactive question card renders INLINE at the conversation tail (in
  // the scrolling body, after the rendered messages and the live-streamed
  // bubble), not in the bottom overlay — so the choices "hang off" the
  // conversation right after the assistant's streamed preamble, where the user
  // is reading, rather than floating over it. The user answers from here: a
  // single-select click, a multi-select toggle + Submit, or a per-question
  // choice across a multi-question call, all POSTed to the answer endpoint,
  // which injects the selection keystrokes into the session's TUI pane. The
  // authoritative clear stays the existing resolution path (the `tool_result`
  // resolving the question's request row), so no extra clear logic is needed.
  // An "Open terminal" fallback remains in the card for a misfired injection.
  // Gate the question card to the thread the question was asked on, mirroring
  // how `showExternalInput` (`TranscriptComposerStack`) and `showStreaming`
  // (`TranscriptPane`) gate by `=== activeThread.id`:
  // AskUserQuestion belongs to the in-flight turn's thread, so the card must
  // not show on the session's other threads.
  const questionCard = question &&
    !question.dismissed &&
    activeThread &&
    question.threadId === activeThread.id && (
    <QuestionCard
      notice={question}
      onAnswer={(selections) =>
        // Return the POST so the card can await it: a 409 (already answered /
        // stale), a 400 (malformed), or a network failure rejects, and the card
        // surfaces an inline error, re-enables its controls for a retry, and
        // emphasizes the terminal fallback. On success the authoritative clear
        // still arrives via the resolution path.
        client.answerQuestion(
          activeThread.session_id,
          question.requestId,
          selections,
        )
      }
      onCancel={() =>
        // Cancel the question in the TUI itself (a single Escape cancels the
        // whole call). Return the POST so the card can await it: a 409 (already
        // resolved / stale) or a network failure rejects, and the card surfaces
        // an inline error, re-enables its controls for a retry, and emphasizes
        // the terminal fallback. On success the authoritative clear still
        // arrives via the resolution path (the `is_error` tool_result).
        client.cancelQuestion(activeThread.session_id, question.requestId)
      }
      sessionClosed={readOnly && !spawning}
      onNotPending={refetchSessions}
      onOpenTerminal={() => setTerminalOpen(true)}
      onDismiss={() => dismissQuestion(activeThread.session_id)}
    />
  );

  // The interactive cards, inline at the very tail of the conversation:
  // after the rendered messages and the live-streamed bubble. The question
  // card comes first so its choices appear right after the assistant's
  // streamed preamble; the permission notice follows at the very end.
  // Inset to align with the prose, in their own block so the bottom
  // padding (pb-composer-reserve) keeps them clear of the floating
  // composer.
  return (
    (questionCard || permissionCard) && (
      <div className="space-y-2 px-3 pt-1.5 pb-2">
        {questionCard}
        {permissionCard}
      </div>
    )
  );
}
