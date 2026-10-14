import { useCallback } from 'react';
import { useApiClient } from '../../data/apiContext';
import type { NewSessionLaunch } from '../../store/liveStore';
import { confirmLaunchTrust } from '../launch-trust/confirmLaunchTrust';
import { newSessionSendBody } from './newSessionRequest';
import { useSubmitSend } from './useSubmitSend';

/**
 * A new-session send: its first prompt plus the {@link NewSessionLaunch}
 * configuration the session starts with. A failed spawn's tracked entry retains
 * all of it so Retry can re-attempt the identical launch.
 */
export type NewSessionSend = NewSessionLaunch & { text: string };

/**
 * Re-attempt a new-session send (used by the failed session's Retry action). It
 * runs the same submission path as the composer's new-session Send — the same
 * `sending` chip, and the same `POST /api/sends` body, built by the shared
 * {@link newSessionSendBody} so a retry can never start a session configured
 * differently from the one that failed. On success the accepted send (real
 * session/thread/send ids) is tracked until its turn ends and the spawn is
 * registered for focus/failure handling. A rejected POST leaves a recoverable
 * `failed` chip, so the error is swallowed here. The row the retry replaces is
 * removed by the caller, so the failed session never lingers beside the fresh
 * attempt.
 *
 * Like the composer's Send, a worktree launch first runs the launch-trust
 * check ({@link confirmLaunchTrust}) — a retry asks again about what the
 * branch holds now, never reusing an earlier confirmation — and a cancelled
 * confirmation sends nothing. Resolves to whether the send went out, so the
 * caller removes the row it replaces only when there is a fresh attempt to
 * replace it with.
 */
export function useNewSessionSend(): (
  send: NewSessionSend,
) => Promise<boolean> {
  const submitSend = useSubmitSend();
  const client = useApiClient();

  return useCallback(
    async ({ text, ...launch }: NewSessionSend) => {
      const trimmed = text.trim();
      if (!trimmed) {
        return false;
      }
      const decision = await confirmLaunchTrust(client, launch);
      if (!decision.proceed) {
        return false;
      }
      void submitSend({
        target: { kind: 'new-session', ...launch },
        text: trimmed,
        body: newSessionSendBody(trimmed, launch, decision.confirmedCommit),
      }).catch(() => {
        // Already surfaced as a failed chip by the submission path.
      });
      return true;
    },
    [client, submitSend],
  );
}
