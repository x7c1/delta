import { useCallback, useLayoutEffect, useRef, type FormEvent } from 'react';
import type { ThreadId } from '@delta/model';
import type { SendRequest, Thread } from '@delta/wire-gen';
import { Button } from '@delta/ui-kit';
import {
  NEW_SESSION_DRAFT_KEY,
  useComposerStore,
} from '../../store/composerStore';
import { COMPOSER_MAX_HEIGHT, autoGrowGeometry } from './autoGrow';
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
 * Paper-plane glyph for the round submit button overlaid on the textarea.
 * Decorative — always `aria-hidden`, so the button's accessible name stays its
 * "Send" label. This file is the only user.
 */
function SendIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <line x1="22" y1="2" x2="11" y2="13" />
      <polygon points="22 2 15 22 11 13 2 9 22 2" />
    </svg>
  );
}

/**
 * The bottom composer: text input bound to a per-thread draft and submit wired
 * to `POST /api/sends` (via the shared submission path, which keeps the
 * pending strip honest). The send target depends on {@link ComposerMode}.
 */
export function Composer({ mode }: ComposerProps) {
  const submitSend = useSubmitSend();
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

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
  const newSessionLaunchOptionIds = useComposerStore(
    (state) => state.newSessionLaunchOptionIds,
  );
  const newSessionWorktreeEnabled = useComposerStore(
    (state) => state.newSessionWorktreeEnabled,
  );
  const newSessionWorktreeStartPoint = useComposerStore(
    (state) => state.newSessionWorktreeStartPoint,
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
        // per-spawn directory (today's behavior). Likewise, attach the selected
        // launch options only when at least one is picked, so an unselected
        // session starts with no extra launch flags.
        // Attach the worktree request only when the opt-in toggle is on AND a
        // directory is selected: the picker only surfaces the toggle once a
        // git-repo directory is chosen, so this guard mirrors that and keeps the
        // backend-rejected "worktree without workdir" state unreachable. Omit it
        // entirely otherwise (the unchanged non-worktree behavior).
        body = {
          new_session: true,
          text,
          ...(newSessionWorkdir ? { workdir: newSessionWorkdir } : {}),
          ...(newSessionLaunchOptionIds.length > 0
            ? { launch_option_ids: newSessionLaunchOptionIds }
            : {}),
          ...(newSessionWorktreeEnabled && newSessionWorkdir
            ? { worktree: { start_point: newSessionWorktreeStartPoint } }
            : {}),
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
            : {
                kind: 'new-session',
                workdir: newSessionWorkdir,
                launchOptionIds: newSessionLaunchOptionIds,
              },
          text,
          body,
        });
        // Picker selections are reset by TranscriptPane's leave-effect when the
        // new-session state is left; doing it here would briefly uncheck the
        // boxes in place before the screen unmounts.
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
      newSessionLaunchOptionIds,
      newSessionWorktreeEnabled,
      newSessionWorktreeStartPoint,
      clearDraft,
      submitSend,
      setBranchOrigin,
      setActiveThread,
      clearResumeUnavailable,
    ],
  );

  // Auto-grow the textarea with its content up to a cap, then scroll
  // internally. Keyed on the controlled `draft` so it grows as you type, shrinks
  // back when text is deleted, and resets to the min height after a submit
  // clears the draft. Reset the inline height to `auto` first so `scrollHeight`
  // reflects the content's natural height (not a previously-applied larger one),
  // then clamp it and toggle the internal scrollbar past the cap.
  useLayoutEffect(() => {
    const el = textareaRef.current;
    if (!el) {
      return;
    }
    el.style.height = 'auto';
    const { height, overflow } = autoGrowGeometry(el.scrollHeight);
    el.style.height = `${height}px`;
    el.style.overflowY = overflow ? 'auto' : 'hidden';
  }, [draft]);

  const placeholder = isNew
    ? 'Message to start a new session…'
    : branching
      ? 'Ask a follow-up on the selected text…'
      : readOnly
        ? 'Send to resume this closed session…'
        : // Prefix the thread title with `#` (Slack/Discord style) so it reads as
          // an addressable target — "Message #main" / "Message #foobar" — rather
          // than the verb running straight into a bare word.
          `Message ${activeThread?.title ? `#${activeThread.title}` : ''}…`;

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
      {/* The textarea spans the full width; the round submit button is overlaid
          in the bottom-right corner. The textarea reserves right padding (`pr-10`)
          so typed text never slides under the button — just past its 32px (`w-8`)
          footprint, leaving a small gap rather than a wide empty margin. The
          button sticks to the bottom so it stays anchored as the textarea
          auto-grows. */}
      <div className="relative">
        <textarea
          ref={textareaRef}
          value={draft}
          onChange={(event) => setDraft(draftKey, event.target.value)}
          placeholder={placeholder}
          rows={2}
          // Auto-grow replaces the manual `resize-y` handle: the height is driven
          // by the effect above (content height clamped to [min, cap]). The cap
          // is also set inline as a hard ceiling so the textarea can never exceed
          // it before the effect runs. `overflow-y` is toggled by the effect, so
          // it scrolls internally only once the content passes the cap.
          style={{ maxHeight: `${COMPOSER_MAX_HEIGHT}px` }}
          // The textarea is borderless and transparent: the enclosing composer
          // card (see TranscriptPane's bottom layer) supplies the visual
          // boundary, so a border here would double up with it. Its own left
          // padding is kept small (`pl-0.5`) because the card already pads
          // `px-3`: together they give the text a 14px left inset that matches
          // its 14px top inset (card `py-2` + the textarea's `py-1.5`), rather
          // than the lopsided 20px the card-default `pl-2` would stack up to.
          className="min-h-[2.5rem] w-full resize-none bg-transparent py-1.5 pl-0.5 pr-10 text-sm focus:outline-none"
          onKeyDown={(event) => {
            if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
              void submit(event);
            }
          }}
        />
        <button
          type="submit"
          aria-label="Send"
          disabled={
            draft.trim().length === 0 ||
            sendInFlight ||
            // A new session must start in a chosen directory: selection is
            // mandatory, so Send stays disabled until the picker commits one.
            (isNew && !newSessionWorkdir)
          }
          // Anchored to the card's bottom-right corner with an equal visual gap
          // to the card's right and bottom borders. The enclosing composer card
          // pads `px-3` (12px) but only `py-2` (8px), so the button sits `right-0`
          // (12px right gap = the card's horizontal padding) and `bottom-1` (4px
          // above the textarea + the card's 8px bottom padding = a matching 12px
          // bottom gap). Staying pinned to the bottom keeps it in the corner as
          // the textarea auto-grows.
          className="absolute bottom-1 right-0 inline-flex h-8 w-8 items-center justify-center rounded-full border border-slate-400 bg-transparent text-indigo-500 shadow-md transition-all hover:bg-slate-200 hover:text-indigo-600 hover:shadow-lg disabled:cursor-not-allowed disabled:text-slate-400 disabled:shadow-md"
        >
          {/* The paper-plane's visual weight sits top-right, so nudge the glyph a
              hair toward the bottom-left to center it within the round button. */}
          <SendIcon className="h-4 w-4 -translate-x-px translate-y-px" />
        </button>
      </div>
    </form>
  );
}
