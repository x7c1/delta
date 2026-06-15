import { useCallback } from 'react';
import type { SendRequest } from '@delta/wire-gen';
import { useSubmitSend } from './useSubmitSend';

/**
 * The text, working directory, and selected launch options of a new-session
 * send. A failed-spawn chip retains all three so Retry can re-attempt the
 * identical launch.
 */
export interface NewSessionSend {
  text: string;
  workdir: string | null;
  launchOptionIds: number[];
}

/**
 * Re-attempt a new-session send (used by the failed-spawn Retry action). It
 * runs the same submission path as the composer's new-session Send: a
 * `sending` chip, then `POST /api/sends` with `{ new_session: true, text,
 * workdir?, launch_option_ids? }`; on success the accepted send (real
 * session/thread/send ids) is
 * tracked until its turn ends and the spawn is registered for focus/failure
 * handling. A rejected POST leaves a recoverable `failed` chip, so the error
 * is swallowed here. The original failed chip is removed by the caller before
 * retrying, so the strip never shows the stale failure alongside the fresh
 * attempt.
 */
export function useNewSessionSend(): (send: NewSessionSend) => void {
  const submitSend = useSubmitSend();

  return useCallback(
    ({ text, workdir, launchOptionIds }: NewSessionSend) => {
      const trimmed = text.trim();
      if (!trimmed) {
        return;
      }
      const body: SendRequest = {
        new_session: true,
        text: trimmed,
        ...(workdir ? { workdir } : {}),
        ...(launchOptionIds.length > 0
          ? { launch_option_ids: launchOptionIds }
          : {}),
      };
      void submitSend({
        target: { kind: 'new-session', workdir, launchOptionIds },
        text: trimmed,
        body,
      }).catch(() => {
        // Already surfaced as a failed chip by the submission path.
      });
    },
    [submitSend],
  );
}
