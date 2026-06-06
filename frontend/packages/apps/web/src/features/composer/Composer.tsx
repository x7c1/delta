import { useCallback, type FormEvent } from 'react';
import type { Thread, ThreadId } from '@delta/model';
import { Badge, Button } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useCreateSendMutation } from '@delta/api-client';
import { useComposerStore } from '../../store/composerStore';
import { useLiveStore } from '../../store/liveStore';
import { useNavStore } from '../../store/navStore';

export interface ComposerProps {
  activeThread: Thread;
}

/**
 * The bottom composer: text input bound to a per-thread draft, an optional
 * branch-origin notice, and submit wired to `POST /api/sends`. A branch origin
 * turns the send into a branch send (semantic parent + locator quote).
 */
export function Composer({ activeThread }: ComposerProps) {
  const client = useApiClient();
  const mutation = useCreateSendMutation(client);

  const draft = useComposerStore(
    (state) => state.drafts[activeThread.id] ?? '',
  );
  const setDraft = useComposerStore((state) => state.setDraft);
  const clearDraft = useComposerStore((state) => state.clearDraft);
  const branchOrigin = useComposerStore((state) => state.branchOrigin);
  const setBranchOrigin = useComposerStore((state) => state.setBranchOrigin);

  const enqueueSend = useLiveStore((state) => state.enqueueSend);
  const attachSendId = useLiveStore((state) => state.attachSendId);
  const failSend = useLiveStore((state) => state.failSend);
  const setActiveThread = useNavStore((state) => state.setActiveThread);

  const branching =
    branchOrigin !== null && branchOrigin.parentThreadId === activeThread.id;

  const submit = useCallback(
    async (event: FormEvent) => {
      event.preventDefault();
      const text = draft.trim();
      if (!text) {
        return;
      }
      const localId = `local-${Date.now()}-${Math.random().toString(36).slice(2)}`;
      const targetThread: ThreadId = activeThread.id;

      // Optimistic FIFO entry shown immediately.
      enqueueSend({
        localId,
        sendId: null,
        threadId: targetThread,
        text,
        semanticParentUuid: branching ? branchOrigin.semanticParentUuid : null,
        status: 'queued',
        createdAt: Date.now(),
      });
      clearDraft(activeThread.id);

      try {
        const { send } = await mutation.mutateAsync({
          thread_id: targetThread,
          text,
          ...(branching
            ? {
                semantic_parent_uuid: branchOrigin.semanticParentUuid,
                locator_quote: branchOrigin.locatorQuote,
              }
            : {}),
        });
        attachSendId(localId, send.id);
        if (branching) {
          // The backend created a fresh child thread for this branch send and
          // returns its id on the send. Drill into it so the user lands in the
          // new branch (the threads query is invalidated by the mutation, so
          // the navigator will render it), and clear the branch origin.
          setActiveThread(send.thread_id);
          setBranchOrigin(null);
        }
      } catch {
        failSend(localId);
      }
    },
    [
      draft,
      activeThread.id,
      branching,
      branchOrigin,
      enqueueSend,
      clearDraft,
      mutation,
      attachSendId,
      setBranchOrigin,
      setActiveThread,
      failSend,
    ],
  );

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
              “{branchOrigin.locatorQuote}”
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
          onChange={(event) => setDraft(activeThread.id, event.target.value)}
          placeholder={
            branching
              ? 'Ask a follow-up on the selected text…'
              : `Message ${activeThread.title}…`
          }
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
