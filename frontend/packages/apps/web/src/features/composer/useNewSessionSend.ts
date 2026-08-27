import { useCallback } from 'react';
import type { NewSessionLaunch } from '../../store/liveStore';
import { newSessionSendBody } from './newSessionRequest';
import { useSubmitSend } from './useSubmitSend';

/**
 * A new-session send: its first prompt plus the {@link NewSessionLaunch}
 * configuration the session starts with. A failed-spawn chip retains all of it
 * so Retry can re-attempt the identical launch.
 */
export type NewSessionSend = NewSessionLaunch & { text: string };

/**
 * Re-attempt a new-session send (used by the failed-spawn Retry action). It
 * runs the same submission path as the composer's new-session Send — the same
 * `sending` chip, and the same `POST /api/sends` body, built by the shared
 * {@link newSessionSendBody} so a retry can never start a session configured
 * differently from the one that failed. On success the accepted send (real
 * session/thread/send ids) is tracked until its turn ends and the spawn is
 * registered for focus/failure handling. A rejected POST leaves a recoverable
 * `failed` chip, so the error is swallowed here. The original failed chip is
 * removed by the caller before retrying, so the strip never shows the stale
 * failure alongside the fresh attempt.
 */
export function useNewSessionSend(): (send: NewSessionSend) => void {
  const submitSend = useSubmitSend();

  return useCallback(
    ({ text, ...launch }: NewSessionSend) => {
      const trimmed = text.trim();
      if (!trimmed) {
        return;
      }
      void submitSend({
        target: { kind: 'new-session', ...launch },
        text: trimmed,
        body: newSessionSendBody(trimmed, launch),
      }).catch(() => {
        // Already surfaced as a failed chip by the submission path.
      });
    },
    [submitSend],
  );
}
