import { useCallback, type FormEvent } from 'react';
import type { SendRequest, SessionId, Thread, ThreadId } from '@delta/model';
import { Badge, Button } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useCreateSendMutation } from '@delta/api-client';
import {
  NEW_SESSION_DRAFT_KEY,
  useComposerStore,
} from '../../store/composerStore';
import { useLiveStore } from '../../store/liveStore';
import { useNavStore } from '../../store/navStore';

/**
 * Send target for the composer:
 *
 * - `new-session`: spawns a brand-new session (`{ new_session: true, text }`).
 *   There is no thread or session id yet; drafts key off a stable sentinel.
 * - `thread`: targets the focused session's active thread. When the session is
 *   closed (`readOnly`), the first Send resumes it (targeting its main thread);
 *   when open, a branch origin turns it into a branch send.
 */
export type ComposerMode =
  | { kind: 'new-session' }
  | {
      kind: 'thread';
      activeThread: Thread;
      readOnly: boolean;
      sessionMainThreadId?: ThreadId;
    };

export interface ComposerProps {
  mode: ComposerMode;
}

/**
 * The bottom composer: text input bound to a per-thread draft and submit wired
 * to `POST /api/sends`. The send target depends on {@link ComposerMode}.
 */
export function Composer({ mode }: ComposerProps) {
  const client = useApiClient();
  const mutation = useCreateSendMutation(client);

  const isNew = mode.kind === 'new-session';
  const activeThread = mode.kind === 'thread' ? mode.activeThread : null;
  const readOnly = mode.kind === 'thread' ? mode.readOnly : false;
  const draftKey: ThreadId = activeThread
    ? activeThread.id
    : NEW_SESSION_DRAFT_KEY;

  const draft = useComposerStore((state) => state.drafts[draftKey] ?? '');
  const setDraft = useComposerStore((state) => state.setDraft);
  const clearDraft = useComposerStore((state) => state.clearDraft);
  const branchOrigin = useComposerStore((state) => state.branchOrigin);
  const setBranchOrigin = useComposerStore((state) => state.setBranchOrigin);

  const enqueueSend = useLiveStore((state) => state.enqueueSend);
  const attachSendId = useLiveStore((state) => state.attachSendId);
  const failSend = useLiveStore((state) => state.failSend);
  const markResuming = useLiveStore((state) => state.markResuming);
  const clearResuming = useLiveStore((state) => state.clearResuming);
  const setActiveThread = useNavStore((state) => state.setActiveThread);

  // A closed session resumes by sending to its main thread (falling back to the
  // active thread if the main id was not supplied).
  const resumeThreadId: ThreadId | null =
    mode.kind === 'thread'
      ? mode.sessionMainThreadId ?? mode.activeThread.id
      : null;

  // Branching only applies to an open session's active thread.
  const branching =
    !readOnly &&
    activeThread !== null &&
    branchOrigin !== null &&
    branchOrigin.parentThreadId === activeThread.id;

  const submit = useCallback(
    async (event: FormEvent) => {
      event.preventDefault();
      const text = draft.trim();
      if (!text) {
        return;
      }
      const localId = `local-${Date.now()}-${Math.random().toString(36).slice(2)}`;

      // The optimistic FIFO entry is keyed by the thread the pending queue
      // renders under. A resume targets the session's main thread; a new-session
      // send has no thread yet, so it uses the sentinel key.
      const optimisticThread: ThreadId = !activeThread
        ? NEW_SESSION_DRAFT_KEY
        : readOnly && resumeThreadId !== null
          ? resumeThreadId
          : activeThread.id;

      enqueueSend({
        localId,
        sendId: null,
        // A new-session send has no bound session yet; turn events for the
        // session it spawns reconcile this once it registers.
        sessionId: activeThread ? activeThread.session_id : null,
        threadId: optimisticThread,
        text,
        semanticParentUuid: branching ? branchOrigin.semanticParentUuid : null,
        status: 'queued',
        createdAt: Date.now(),
      });
      clearDraft(draftKey);

      // Build the send target.
      let body: SendRequest;
      // The session marked resuming by this send, so its marker can be cleared
      // if the request fails (otherwise `resuming…` would stick forever).
      let resumingSessionId: SessionId | null = null;
      if (isNew) {
        body = { new_session: true, text };
      } else if (readOnly && activeThread && resumeThreadId !== null) {
        // Resume a closed session: send to its main thread; the backend
        // auto-resumes. Mark it resuming until session_opened lands.
        body = { thread_id: resumeThreadId, text };
        resumingSessionId = activeThread.session_id;
        markResuming(activeThread.session_id);
      } else {
        body = {
          thread_id: activeThread!.id,
          text,
          ...(branching
            ? {
                semantic_parent_uuid: branchOrigin.semanticParentUuid,
                locator_quote: branchOrigin.locatorQuote,
              }
            : {}),
        };
      }

      try {
        const { send } = await mutation.mutateAsync(body);
        attachSendId(localId, send.id);
        if (branching) {
          // The backend created a fresh child thread for this branch send and
          // returns its id; drill into it and clear the branch origin.
          setActiveThread(send.thread_id);
          setBranchOrigin(null);
        }
      } catch {
        failSend(localId);
        // The resume never started: drop the marker so it does not stick.
        if (resumingSessionId !== null) {
          clearResuming(resumingSessionId);
        }
      }
    },
    [
      draft,
      draftKey,
      isNew,
      readOnly,
      resumeThreadId,
      activeThread,
      branching,
      branchOrigin,
      enqueueSend,
      clearDraft,
      mutation,
      attachSendId,
      markResuming,
      clearResuming,
      setBranchOrigin,
      setActiveThread,
      failSend,
    ],
  );

  const placeholder = isNew
    ? 'Message to start a new session…'
    : readOnly
      ? 'Send to resume this closed session…'
      : branching
        ? 'Ask a follow-up on the selected text…'
        : `Message ${activeThread?.title ?? ''}…`;

  return (
    <form onSubmit={submit} className="space-y-2">
      {branching && (
        <div className="flex items-start justify-between gap-2 rounded border border-indigo-200 bg-indigo-50 px-2 py-1 text-xs">
          <span className="flex flex-col gap-0.5">
            <span className="flex items-center gap-1 font-medium text-indigo-700">
              <Badge tone="info">branch</Badge>
              from selected text
            </span>
            <span className="line-clamp-2 italic text-slate-600">
              “{branchOrigin?.locatorQuote}”
            </span>
          </span>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setBranchOrigin(null)}
            aria-label="Cancel branch"
          >
            ✕
          </Button>
        </div>
      )}
      <div className="flex items-end gap-2">
        <textarea
          value={draft}
          onChange={(event) => setDraft(draftKey, event.target.value)}
          placeholder={placeholder}
          rows={2}
          className="min-h-[2.5rem] flex-1 resize-y rounded border border-slate-300 px-2 py-1.5 text-sm focus:border-indigo-400 focus:outline-none"
          onKeyDown={(event) => {
            if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
              void submit(event);
            }
          }}
        />
        <Button
          type="submit"
          variant="primary"
          disabled={draft.trim().length === 0 || mutation.isPending}
        >
          Send
        </Button>
      </div>
    </form>
  );
}
