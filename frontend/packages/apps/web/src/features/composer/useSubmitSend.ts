import { useCallback } from 'react';
import type { Send, SendRequest } from '@delta/wire-gen';
import { ApiError, useCreateSendMutation } from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { useLiveStore, type SendingItem } from '../../store/liveStore';

/**
 * The one path every composer submit takes, shared by the composer's Send and
 * the failed-spawn Retry. It owns the client-side bookkeeping around
 * `POST /api/sends`:
 *
 * 1. record a `sending` chip for the surface the user submitted on, so the
 *    pending strip reacts instantly (the server knows nothing yet);
 * 2. fire the POST (the mutation's onSuccess patches the accepted send into
 *    the session's open-send cache, so the chip is server-backed without a
 *    fetch gap);
 * 3. on success, swap the `sending` chip for a tracked local send keyed by the
 *    REAL ids the server returned — it keeps the chip alive after the send
 *    matches its transcript line, until the turn-end event lands — and, for a
 *    new-session target, track the spawn so the workspace can focus the new
 *    session by id and a failed launch can surface Retry / Dismiss;
 * 4. on failure, keep the chip as a recoverable `failed` row — except a
 *    `resume_unavailable` rejection, where the turn can never start (the
 *    transcript is gone): the chip is dropped outright and the session is
 *    flagged so the inline notice shows instead.
 *
 * Resolves with the accepted send; rejects with the original error after the
 * bookkeeping above, so callers only add their own post-success steps.
 */
export function useSubmitSend(): (args: {
  target: SendingItem['target'];
  text: string;
  body: SendRequest;
}) => Promise<Send> {
  const client = useApiClient();
  const mutation = useCreateSendMutation(client);
  const beginSending = useLiveStore((state) => state.beginSending);
  const failSending = useLiveStore((state) => state.failSending);
  const removeSending = useLiveStore((state) => state.removeSending);
  const recordLocalSend = useLiveStore((state) => state.recordLocalSend);
  const trackSpawn = useLiveStore((state) => state.trackSpawn);
  const markResumeUnavailable = useLiveStore(
    (state) => state.markResumeUnavailable,
  );

  return useCallback(
    async ({ target, text, body }) => {
      const id = `local-${Date.now()}-${Math.random().toString(36).slice(2)}`;
      beginSending({
        id,
        target,
        text,
        status: 'sending',
        createdAt: Date.now(),
      });
      try {
        const { send } = await mutation.mutateAsync(body);
        removeSending(id);
        recordLocalSend({
          sendId: send.id,
          sessionId: send.session_id,
          threadId: send.thread_id,
          text: send.text,
          createdAt: Date.now(),
        });
        if (target.kind === 'new-session') {
          trackSpawn({
            sessionId: send.session_id,
            threadId: send.thread_id,
            text: send.text,
            workdir: target.workdir,
          });
        }
        return send;
      } catch (error) {
        if (
          error instanceof ApiError &&
          error.code === 'resume_unavailable' &&
          target.kind === 'thread'
        ) {
          removeSending(id);
          markResumeUnavailable(target.sessionId);
        } else {
          failSending(id);
        }
        throw error;
      }
    },
    [
      beginSending,
      mutation,
      removeSending,
      recordLocalSend,
      trackSpawn,
      markResumeUnavailable,
      failSending,
    ],
  );
}
