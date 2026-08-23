import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { usePromptTemplatesQuery } from '@delta/api-client';
import type { PromptTemplate } from '@delta/wire-gen';
import { Spinner, cn } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';

export interface PromptTemplatePopoverProps {
  /** Insert this template into the draft. The caller then closes the popover. */
  onSelect: (template: PromptTemplate) => void;
  /** Open the settings registry (the footer item). */
  onManage: () => void;
}

/**
 * The panel behind the composer's prompt-template button: pick one of the
 * registered templates to splice into the draft.
 *
 * Two columns, because a template is a BLOCK of prose rather than a label with
 * a one-line body. The left column lists labels only — the same choice the
 * settings list makes, for the same reason: any inline preview would either
 * truncate a multi-paragraph body into nonsense or swamp the list. The right
 * column then shows the focused (or hovered) template's text IN FULL, scrolling
 * within a capped height, so the user reads what they are about to insert
 * before committing to it.
 *
 * It opens UPWARD (`bottom-full`): the composer sits at the bottom of the
 * screen, so a downward panel would fall off it. Escape, click-outside and
 * focus restoration are owned by the button that mounts this (it holds the open
 * state and the anchor); what lives here is the list's own roving focus — ↑/↓
 * move between labels and drive the preview, Enter/click select.
 *
 * The list is fetched on open rather than kept warm: the button is on screen
 * for the entire session, and a registry the user is not looking at is not
 * worth a request.
 */
export function PromptTemplatePopover({
  onSelect,
  onManage,
}: PromptTemplatePopoverProps) {
  const client = useApiClient();
  const query = usePromptTemplatesQuery(client, true);
  const templates = query.data?.prompt_templates ?? [];

  // The row the preview mirrors: moved by ↑/↓ (which also move DOM focus) and
  // by hover, so pointer and keyboard drive the same single highlight.
  const [activeIndex, setActiveIndex] = useState(0);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const manageRef = useRef<HTMLButtonElement | null>(null);

  // Take focus once, as soon as there is something to take it — the first
  // template, or the footer when the registry is empty or unreadable. Gated on
  // a ref rather than on `open`, because the first open lands while the list is
  // still in flight: the effect must wait for the rows to exist, then fire
  // exactly once, and never yank focus back from a subsequent ↑/↓.
  const claimedFocus = useRef(false);
  useEffect(() => {
    if (claimedFocus.current || query.isPending) {
      return;
    }
    claimedFocus.current = true;
    if (templates.length > 0) {
      setActiveIndex(0);
      itemRefs.current[0]?.focus();
    } else {
      manageRef.current?.focus();
    }
  }, [query.isPending, templates.length]);

  // A refetch (a template deleted in settings while this is open) can shorten
  // the list under the highlight; keep it on a row that exists.
  useEffect(() => {
    if (activeIndex > 0 && activeIndex >= templates.length) {
      setActiveIndex(Math.max(0, templates.length - 1));
    }
  }, [activeIndex, templates.length]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') {
      return;
    }
    if (templates.length === 0) {
      return;
    }
    // Otherwise the arrow would also scroll the panel (and, with focus still in
    // the composer's orbit, look like it moved the caret).
    event.preventDefault();
    const delta = event.key === 'ArrowDown' ? 1 : -1;
    const next =
      (activeIndex + delta + templates.length) % templates.length;
    setActiveIndex(next);
    itemRefs.current[next]?.focus();
  };

  const preview = templates[activeIndex]?.text ?? '';

  return (
    <div
      role="menu"
      aria-label="Prompt templates"
      data-testid="prompt-templates-popover"
      onKeyDown={onKeyDown}
      // Opening upward from the rail, layered over the cards stacked above the
      // composer. Its offset parent is the RAIL, not the button's box (see
      // {@link ComposerRail}), and the rail starts at the button — so `left-0`
      // still left-aligns to the button, while `max-w-full` is measured against
      // the composer card. Wide enough for prose, never wider than that card:
      // this pane is one column of a multi-pane workspace and can be a good deal
      // narrower than the window.
      className="absolute bottom-full left-0 z-20 mb-1 flex w-[38rem] max-w-full flex-col overflow-hidden rounded-md border border-border-default bg-surface-elevated text-left shadow-lg"
    >
      {query.isPending ? (
        <div className="flex justify-center py-6">
          <Spinner label="loading prompt templates" />
        </div>
      ) : query.isError ? (
        // The footer below stays reachable, so a failed load is still a way
        // into the registry rather than a dead end.
        <p
          role="alert"
          data-testid="prompt-templates-popover-error"
          className="px-3 py-4 text-caption text-danger"
        >
          Could not load prompt templates.
        </p>
      ) : templates.length === 0 ? (
        <p
          data-testid="prompt-templates-popover-empty"
          className="px-3 py-6 text-center text-secondary text-fg-subtle"
        >
          No prompt templates yet.
        </p>
      ) : (
        <div className="flex min-h-0">
          <div
            data-testid="prompt-templates-popover-list"
            className="max-h-[50vh] w-52 shrink-0 overflow-y-auto border-r border-border-default py-1 scrollbar-hover"
          >
            {templates.map((template, index) => (
              <button
                key={template.id}
                ref={(node) => {
                  itemRefs.current[index] = node;
                }}
                type="button"
                role="menuitem"
                data-testid={`prompt-template-option-${template.id}`}
                title={template.label}
                onMouseEnter={() => setActiveIndex(index)}
                onFocus={() => setActiveIndex(index)}
                onClick={() => onSelect(template)}
                className={cn(
                  'block w-full truncate px-3 py-1.5 text-left text-secondary transition-colors',
                  index === activeIndex
                    ? 'bg-surface-elevated-hover text-fg'
                    : 'text-fg-muted hover:bg-surface-elevated-hover hover:text-fg',
                )}
              >
                {template.label}
              </button>
            ))}
          </div>
          {/* The body, whole. `whitespace-pre-wrap` keeps the blank lines and
              indentation the user wrote; the cap turns a long template into a
              scroll rather than a popover taller than the window. */}
          <div
            role="note"
            aria-label="Prompt template preview"
            data-testid="prompt-templates-popover-preview"
            className="max-h-[50vh] min-w-0 flex-1 overflow-y-auto whitespace-pre-wrap break-words px-3 py-2 text-body text-fg-muted scrollbar-hover"
          >
            {preview}
          </div>
        </div>
      )}
      <button
        ref={manageRef}
        type="button"
        role="menuitem"
        data-testid="prompt-templates-manage"
        onClick={onManage}
        className="block w-full border-t border-border-default px-3 py-1.5 text-left text-caption text-fg-muted transition-colors hover:bg-surface-elevated-hover hover:text-fg"
      >
        Manage templates…
      </button>
    </div>
  );
}
