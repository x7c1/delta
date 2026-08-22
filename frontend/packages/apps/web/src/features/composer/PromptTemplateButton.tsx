import { useCallback, useEffect, useRef, useState } from 'react';
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

  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement | null>(null);
  // The caret as it stood when the popover opened. Captured there and not at
  // selection time, because opening moves focus INTO the popover — by the time
  // a template is chosen the textarea no longer reports a live caret.
  const selectionRef = useRef<CaretRange | null>(null);
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

  const close = useCallback(
    (restoreFocus: boolean) => {
      setOpen(false);
      // Hand the caret back exactly where it was picked up, so dismissing the
      // popover leaves the draft as untouched as it looks.
      const selection = selectionRef.current;
      if (restoreFocus && selection) {
        focusTextarea(selection);
      }
    },
    [focusTextarea],
  );

  const openPopover = useCallback(() => {
    const textarea = target.textareaRef.current;
    // A textarea that has never held the caret reports offset 0, which would
    // silently prepend to a draft the user restored or pasted; end-of-draft is
    // the honest reading of "wherever they left off".
    selectionRef.current =
      textarea && target.everFocusedRef.current
        ? { start: textarea.selectionStart, end: textarea.selectionEnd }
        : { start: draft.length, end: draft.length };
    setOpen(true);
  }, [draft.length, target]);

  // Escape and click-outside dismiss, mirroring the ui-kit Menu — this panel
  // cannot BE that Menu (its items are single-line labels and its trigger owns
  // the glyph), but it must feel like it.
  useEffect(() => {
    if (!open) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        close(true);
      }
    };
    const onPointerDown = (event: PointerEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) {
        close(true);
      }
    };
    document.addEventListener('keydown', onKeyDown);
    document.addEventListener('pointerdown', onPointerDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      document.removeEventListener('pointerdown', onPointerDown);
    };
  }, [open, close]);

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
      const selection = selectionRef.current ?? {
        start: draft.length,
        end: draft.length,
      };
      const { next, caret } = insertAtSelection(
        draft,
        selection.start,
        selection.end,
        template.text,
      );
      setDraft(draftKey, next);
      setPendingCaret(caret);
      // Focus is restored by the pending-caret effect, at the new offset.
      setOpen(false);
    },
    [draft, draftKey, setDraft],
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
    <div ref={containerRef} className={COMPOSER_RAIL_ITEM_CLASS}>
      <button
        type="button"
        aria-label="Prompt templates"
        aria-haspopup="menu"
        aria-expanded={open}
        data-testid="prompt-templates-button"
        onClick={() => (open ? close(true) : openPopover())}
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
        <PromptTemplatePopover onSelect={selectTemplate} onManage={manage} />
      )}
    </div>
  );
}
