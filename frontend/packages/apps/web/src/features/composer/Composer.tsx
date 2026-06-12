import { useCallback, type FormEvent } from 'react';
import type { ThreadId } from '@delta/model';
import type { SendRequest, Thread } from '@delta/wire-gen';
import { Button } from '@delta/ui-kit';
import {
  NEW_SESSION_DRAFT_KEY,
  useComposerStore,
} from '../../store/composerStore';
import { useLiveStore } from '../../store/liveStore';
import { useNavStore } from '../../store/navStore';
import { useSubmitSend } from './useSubmitSend';

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
 * to `POST /api/sends` (via the shared submission path, which keeps the
 * pending strip honest). The send target depends on {@link ComposerMode}.
 */
export function Composer({ mode }: ComposerProps) {
  const submitSend = useSubmitSend();

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
  const newSessionWorkdir = useComposerStore(
    (state) => state.newSessionWorkdir,
  );
  const setNewSessionWorkdir = useComposerStore(
    (state) => state.setNewSessionWorkdir,
  );

  const sendInFlight = useLiveStore((state) =>
    state.sending.some((item) => item.status === 'sending'),
  );
  const clearResumeUnavailable = useLiveStore(
    (state) => state.clearResumeUnavailable,
  );
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

      // A fresh attempt against this session clears any stale "cannot be
      // resumed" notice up front, so a retry (e.g. after the transcript is
      // restored) does not leave the old notice showing alongside the new chip.
      if (activeThread) {
        clearResumeUnavailable(activeThread.session_id);
      }
      clearDraft(draftKey);

      // Build the send target. A branch carries the semantic parent; otherwise
      // it is a plain send to the active thread. Both target the active thread
      // regardless of open/closed — for a closed session the backend
      // ensure-opens (resumes) it before dispatching, so continuing a sub-thread
      // stays on that sub-thread instead of jumping to the session's main thread.
      let body: SendRequest;
      if (isNew) {
        // Honor the picker's chosen working directory when one is selected;
        // omit `workdir` entirely otherwise so the server uses its default
        // per-spawn directory (today's behavior).
        body = {
          new_session: true,
          text,
          ...(newSessionWorkdir ? { workdir: newSessionWorkdir } : {}),
        };
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
        const send = await submitSend({
          target: activeThread
            ? {
                kind: 'thread',
                sessionId: activeThread.session_id,
                threadId: activeThread.id,
              }
            : { kind: 'new-session', workdir: newSessionWorkdir },
          text,
          body,
        });
        if (isNew) {
          // The spawn was accepted; reset the picker selection so the next new
          // session starts from the default again. (The accepted spawn itself
          // is tracked by the submission path; the workspace focuses it by its
          // real id once it registers.)
          setNewSessionWorkdir(null);
        }
        if (branching) {
          // The backend created a fresh child thread for this branch send and
          // returns its id; drill into it. The pending chip needs no re-keying:
          // the accepted send already carries the child thread id.
          setActiveThread(send.thread_id);
          setBranchOrigin(null);
        }
      } catch {
        // The submission path already surfaced the failure (a recoverable
        // failed chip, or the resume-unavailable notice); nothing more to do.
      }
    },
    [
      draft,
      draftKey,
      isNew,
      activeThread,
      branching,
      branchOrigin,
      newSessionWorkdir,
      clearDraft,
      submitSend,
      setBranchOrigin,
      setNewSessionWorkdir,
      setActiveThread,
      clearResumeUnavailable,
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
            <span className="font-medium text-indigo-700">
              Branch from selected text
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
          disabled={
            draft.trim().length === 0 ||
            sendInFlight ||
            // A new session must start in a chosen directory: selection is
            // mandatory, so Send stays disabled until the picker commits one.
            (isNew && !newSessionWorkdir)
          }
        >
          Send
        </Button>
      </div>
    </form>
  );
}
