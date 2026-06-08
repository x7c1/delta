import { useCallback, type FormEvent } from 'react';
import type { SendRequest, Thread, ThreadId } from '@delta/model';
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
 * - `thread`: targets the focused session's active thread. A branch origin turns
 *   the send into a branch (a new child thread); otherwise it is a plain send to
 *   the active thread. Either way, a closed (`readOnly`) session is re-opened by
 *   the backend before the send is dispatched — continuing whatever thread is in
 *   view, main or sub, rather than jumping to the session's main thread.
 */
export type ComposerMode =
  | { kind: 'new-session' }
  | {
      kind: 'thread';
      activeThread: Thread;
      readOnly: boolean;
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
  const retargetSend = useLiveStore((state) => state.retargetSend);
  const failSend = useLiveStore((state) => state.failSend);
  const setActiveThread = useNavStore((state) => state.setActiveThread);

  // Branching applies to the active thread whether the session is open or
  // closed: a branch send to a closed session resumes it (the backend
  // ensure-opens) and then creates the child thread, so a quote from an old
  // conversation can be picked up and drilled into.
  const branching =
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
      // renders under. A branch send keys to the active (parent) thread and is
      // retargeted to the child once the backend creates it; a plain send keys
      // to the active thread directly. Only the new-session send, which has no
      // thread yet, uses the sentinel key.
      const optimisticThread: ThreadId = activeThread
        ? activeThread.id
        : NEW_SESSION_DRAFT_KEY;

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

      // Build the send target. A branch carries the semantic parent; otherwise
      // it is a plain send to the active thread. Both target the active thread
      // regardless of open/closed — for a closed session the backend
      // ensure-opens (resumes) it before dispatching, so continuing a sub-thread
      // stays on that sub-thread instead of jumping to the session's main thread.
      let body: SendRequest;
      if (isNew) {
        body = { new_session: true, text };
      } else if (branching) {
        body = {
          thread_id: activeThread!.id,
          text,
          semantic_parent_uuid: branchOrigin.semanticParentUuid,
          locator_quote: branchOrigin.locatorQuote,
        };
      } else {
        body = { thread_id: activeThread!.id, text };
      }

      try {
        const { send } = await mutation.mutateAsync(body);
        attachSendId(localId, send.id);
        if (branching) {
          // The backend created a fresh child thread for this branch send and
          // returns its id. The pending entry was enqueued under the parent
          // thread (the child did not exist yet), so move it onto the child and
          // drill into it — otherwise the "waiting" indicator would stay on the
          // parent while the user is looking at the new sub-thread.
          retargetSend(localId, send.thread_id);
          setActiveThread(send.thread_id);
          setBranchOrigin(null);
        }
      } catch {
        failSend(localId);
      }
    },
    [
      draft,
      draftKey,
      isNew,
      activeThread,
      branching,
      branchOrigin,
      enqueueSend,
      clearDraft,
      mutation,
      attachSendId,
      retargetSend,
      setBranchOrigin,
      setActiveThread,
      failSend,
    ],
  );

  const placeholder = isNew
    ? 'Message to start a new session…'
    : branching
      ? 'Ask a follow-up on the selected text…'
      : readOnly
        ? 'Send to resume this closed session…'
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
