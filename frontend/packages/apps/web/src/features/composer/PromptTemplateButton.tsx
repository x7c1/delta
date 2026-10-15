import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from 'react';
import type { PromptTemplate } from '@delta/wire-gen';
import { cn } from '@delta/ui-kit';
import { useComposerStore } from '../../store/composerStore';
import { useNavStore } from '../../store/navStore';
import { useSettingsStore } from '../../store/settingsStore';
import { COMPOSER_RAIL_ITEM_CLASS } from './ComposerRail';
import { useComposerDraftTarget } from './composerDraftTarget';
import { insertAtSelection } from './insertAtSelection';
import { PromptTemplatePopover } from './PromptTemplatePopover';

/**
 * "Document with text lines" glyph for the prompt-template button — a block of
 * written text, which is what a template is. Decorative: the button carries the
 * accessible name. This file is the only user.
 */
function FileTextIcon({ className }: { className?: string }) {
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
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <polyline points="14 2 14 8 20 8" />
      <line x1="16" y1="13" x2="8" y2="13" />
      <line x1="16" y1="17" x2="8" y2="17" />
      <polyline points="10 9 9 9 8 9" />
    </svg>
  );
}

/**
 * A captured caret: where in the draft an insertion will land. Named for the
 * caret rather than "selection" so it is not read as the DOM's own `Selection`,
 * which this file never touches.
 */
interface CaretRange {
  start: number;
  end: number;
}

/**
 * How the popover is open:
 *
 * - `hover` — the pointer rested on the button. Focus stays where it was (the
 *   user may be typing in the composer), and the popover closes once the
 *   pointer has left both the button and the popover.
 * - `pinned` — a click or keyboard activation opened it (or a click pinned a
 *   hover-opened one). Focus moves into the list for ↑/↓/Enter, and only
 *   Escape, click-outside, a second click or a choice closes it.
 */
type OpenMode = 'hover' | 'pinned';

/**
 * How long the pointer must rest on the button before the popover opens, so
 * passing over the rail does not flash it.
 */
export const HOVER_OPEN_DELAY_MS = 150;

/**
 * How long a hover-opened popover survives the pointer leaving it, so the
 * pointer can cross the gap between the button and the popover above it.
 */
export const HOVER_CLOSE_DELAY_MS = 200;

/**
 * The composer rail's leftmost item: insert a registered prompt template into
 * the draft at the cursor.
 *
 * It is the ONLY entry point to the templates, so it renders in both composer
 * modes and stays visible even with an empty registry — a control that appeared
 * only once templates existed would leave a user with none no way to discover
 * them (the popover's footer is how they reach the settings editor).
 *
 * Inserting touches nothing but the local draft — no request, no session state —
 * so it behaves identically whether the session is new, idle, mid-turn, closed
 * or resuming.
 */
export function PromptTemplateButton() {
  const target = useComposerDraftTarget();
  const draftKey = target.draftKey;
  const draft = useComposerStore((state) => state.drafts[draftKey] ?? '');
  const setDraft = useComposerStore((state) => state.setDraft);
  const openSettings = useNavStore((state) => state.openSettings);
  const setActiveCategory = useSettingsStore((state) => state.setActiveCategory);

  const [mode, setMode] = useState<OpenMode | null>(null);
  const open = mode !== null;
  const containerRef = useRef<HTMLDivElement | null>(null);
  // The caret as it stood when the popover was pinned. Captured there and not
  // at selection time, because pinning moves focus INTO the popover — by the
  // time a template is chosen the textarea no longer reports a live caret. A
  // hover-opened popover leaves focus in the textarea, so its choice reads the
  // caret afresh instead (see `selectTemplate`).
  const selectionRef = useRef<CaretRange | null>(null);
  // The pending hover open (intent delay) or hover close (grace delay); at
  // most one is ever outstanding.
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Where the caret must be put once React has re-rendered the textarea with
  // the spliced draft. `null` when nothing is pending.
  const [pendingCaret, setPendingCaret] = useState<number | null>(null);

  /** Put the caret back in the textarea, at `at`, and focus it. */
  const focusTextarea = useCallback(
    (at: CaretRange) => {
      const textarea = target.textareaRef.current;
      if (!textarea) {
        return;
      }
      textarea.focus();
      textarea.setSelectionRange(at.start, at.end);
    },
    [target],
  );

  const clearHoverTimer = useCallback(() => {
    if (hoverTimerRef.current !== null) {
      clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
    }
  }, []);

  useEffect(() => clearHoverTimer, [clearHoverTimer]);

  const close = useCallback(
    (restoreFocus: boolean) => {
      clearHoverTimer();
      setMode(null);
      // Hand the caret back exactly where it was picked up, so dismissing the
      // popover leaves the draft as untouched as it looks.
      const selection = selectionRef.current;
      if (restoreFocus && selection) {
        focusTextarea(selection);
      }
    },
    [focusTextarea, clearHoverTimer],
  );

  /** Read the textarea's caret into {@link selectionRef}, and return it. */
  const captureCaret = useCallback((): CaretRange => {
    const textarea = target.textareaRef.current;
    // A textarea that has never held the caret reports offset 0, which would
    // silently prepend to a draft the user restored or pasted; end-of-draft is
    // the honest reading of "wherever they left off".
    const caret =
      textarea && target.everFocusedRef.current
        ? { start: textarea.selectionStart, end: textarea.selectionEnd }
        : { start: draft.length, end: draft.length };
    selectionRef.current = caret;
    return caret;
  }, [draft.length, target]);

  /**
   * Open with focus in the list — a click, Enter/Space on the button, or a
   * click on the button of a hover-opened popover. The caret is captured here,
   * at pin time: while the popover was open by hover the textarea kept focus,
   * so the user may have typed since it opened.
   */
  const openPinned = useCallback(() => {
    clearHoverTimer();
    captureCaret();
    setMode('pinned');
  }, [captureCaret, clearHoverTimer]);

  // Hover opens and closes the popover through the CONTAINER, which holds both
  // the button and (as a DOM child) the popover, so moving from one into the
  // other is not a leave. Touch has no hover: its pointer events are ignored
  // here, so a tap stays the plain click it always was.
  const onPointerEnter = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (event.pointerType === 'touch') {
        return;
      }
      clearHoverTimer();
      if (open) {
        return;
      }
      hoverTimerRef.current = setTimeout(() => {
        hoverTimerRef.current = null;
        setMode((current) => current ?? 'hover');
      }, HOVER_OPEN_DELAY_MS);
    },
    [clearHoverTimer, open],
  );

  const onPointerLeave = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (event.pointerType === 'touch') {
        return;
      }
      clearHoverTimer();
      if (mode !== 'hover') {
        return;
      }
      hoverTimerRef.current = setTimeout(() => {
        hoverTimerRef.current = null;
        // A click may have pinned it in the meantime; only a still-hovering
        // popover goes. Focus never left, so there is none to restore.
        setMode((current) => (current === 'hover' ? null : current));
      }, HOVER_CLOSE_DELAY_MS);
    },
    [clearHoverTimer, mode],
  );

  // Escape and click-outside dismiss, mirroring the ui-kit Menu — this panel
  // cannot BE that Menu (its items are single-line labels and its trigger owns
  // the glyph), but it must feel like it. Only a pinned popover took focus, so
  // only a pinned one hands it back; a hover-opened one leaves it where it is.
  useEffect(() => {
    if (mode === null) {
      return;
    }
    const restoreFocus = mode === 'pinned';
    const onKeyDown = (event: KeyboardEvent) => {
      // A hover-opened popover leaves the caret in the textarea, where Escape
      // may be cancelling an IME composition rather than addressing this.
      if (event.key === 'Escape' && !event.isComposing) {
        event.stopPropagation();
        close(restoreFocus);
      }
    };
    const onPointerDown = (event: PointerEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) {
        close(restoreFocus);
      }
    };
    document.addEventListener('keydown', onKeyDown);
    document.addEventListener('pointerdown', onPointerDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      document.removeEventListener('pointerdown', onPointerDown);
    };
  }, [mode, close]);

  // The caret can only be set once the textarea is showing the new value, so it
  // waits for the render that the draft update triggers rather than running in
  // the click handler.
  useEffect(() => {
    if (pendingCaret === null) {
      return;
    }
    focusTextarea({ start: pendingCaret, end: pendingCaret });
    setPendingCaret(null);
  }, [pendingCaret, draft, focusTextarea]);

  const selectTemplate = useCallback(
    (template: PromptTemplate) => {
      // A hover-opened popover never took focus, so the user may have typed
      // since it opened; the textarea still holds that live caret (a blurred
      // textarea keeps its selection), so read it afresh. A pinned popover
      // took focus at open, and the caret captured then is the one to use.
      const selection =
        mode === 'hover'
          ? captureCaret()
          : (selectionRef.current ?? {
              start: draft.length,
              end: draft.length,
            });
      const { next, caret } = insertAtSelection(
        draft,
        selection.start,
        selection.end,
        template.text,
      );
      setDraft(draftKey, next);
      setPendingCaret(caret);
      // Focus is restored by the pending-caret effect, at the new offset.
      clearHoverTimer();
      setMode(null);
    },
    [captureCaret, clearHoverTimer, draft, draftKey, mode, setDraft],
  );

  const manage = useCallback(() => {
    // Focus belongs to the dialog that is about to open, not back in the
    // textarea, so this close deliberately does not restore it.
    close(false);
    setActiveCategory('prompt-templates');
    openSettings();
  }, [close, openSettings, setActiveCategory]);

  return (
    // Deliberately NOT `relative`: the popover below anchors to the enclosing
    // rail (which is), so its width can be capped at the composer card's rather
    // than at this box's — see {@link ComposerRail}. This div stays the
    // click-outside boundary either way; the popover is its DOM child.
    <div
      ref={containerRef}
      className={COMPOSER_RAIL_ITEM_CLASS}
      onPointerEnter={onPointerEnter}
      onPointerLeave={onPointerLeave}
    >
      <button
        type="button"
        aria-label="Prompt templates"
        aria-haspopup="menu"
        aria-expanded={open}
        data-testid="prompt-templates-button"
        // A click on a hover-opened popover pins it rather than closing it:
        // the pointer is already over the button, so the click is read as
        // "I want this" — and it is how the keyboard path is reached by mouse.
        onClick={() => (mode === 'pinned' ? close(true) : openPinned())}
        // `px-2.5`/`py-1` with a 14px glyph gives the same 22px item height as
        // the provider tabs beside it (`py-1` + `text-secondary leading-none`),
        // so the rail's items sit on one line.
        className="group flex items-center px-2.5 py-1 leading-none"
      >
        <span
          className={cn(
            'inline-flex transition-colors',
            open ? 'text-fg' : 'text-fg-subtle group-hover:text-fg',
          )}
        >
          <FileTextIcon className="h-3.5 w-3.5" />
        </span>
      </button>
      {open && (
        <PromptTemplatePopover
          onSelect={selectTemplate}
          onManage={manage}
          takeFocus={mode === 'pinned'}
        />
      )}
    </div>
  );
}
