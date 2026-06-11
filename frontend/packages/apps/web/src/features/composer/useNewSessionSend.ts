import { useCallback } from 'react';
import type { SendRequest } from '@delta/model';
import { useCreateSendMutation } from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { NEW_SESSION_DRAFT_KEY } from '../../store/composerStore';
import { useLiveStore } from '../../store/liveStore';

/**
 * The text + working directory of a new-session send. A failed-spawn pending chip
 * retains both so Retry can re-attempt the identical launch.
 */
export interface NewSessionSend {
  text: string;
  workdir: string | null;
}

/**
 * Re-attempt a new-session send (used by the failed-spawn Retry action). It
 * mirrors the new-session branch of the composer's submit: enqueue a fresh
 * optimistic pending chip under the new-session sentinel thread, then
 * `POST /api/sends` with `{ new_session: true, text, workdir? }`, attaching the
 * server send id on success or marking the chip `failed` on error. The original
 * failed chip is removed by the caller before retrying, so the FIFO never shows
 * the stale failure alongside the fresh attempt.
 */
export function useNewSessionSend(): (send: NewSessionSend) => void {
  const client = useApiClient();
  const mutation = useCreateSendMutation(client);
  const enqueueSend = useLiveStore((state) => state.enqueueSend);
  const attachSendId = useLiveStore((state) => state.attachSendId);
  const failSend = useLiveStore((state) => state.failSend);

  return useCallback(
    ({ text, workdir }: NewSessionSend) => {
      const trimmed = text.trim();
      if (!trimmed) {
        return;
      }
      const localId = `local-${Date.now()}-${Math.random().toString(36).slice(2)}`;

      enqueueSend({
        localId,
        sendId: null,
        // A new-session send has no bound session yet; its spawn reconciles this
        // once it registers (or fails again into another `failed` chip).
        sessionId: null,
        threadId: NEW_SESSION_DRAFT_KEY,
        text: trimmed,
        semanticParentUuid: null,
        workdir,
        status: 'queued',
        createdAt: Date.now(),
      });

      const body: SendRequest = {
        new_session: true,
        text: trimmed,
        ...(workdir ? { workdir } : {}),
      };

      void mutation
        .mutateAsync(body)
        .then(({ send }) => attachSendId(localId, send.id))
        .catch(() => failSend(localId));
    },
    [enqueueSend, attachSendId, failSend, mutation],
  );
}
