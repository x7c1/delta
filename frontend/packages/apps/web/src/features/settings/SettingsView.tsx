import { useRef, type KeyboardEvent, type ReactNode } from 'react';
import { Button, cn, Dialog } from '@delta/ui-kit';
import { useNavStore } from '../../store/navStore';
import {
  type SettingsCategoryId,
  useSettingsStore,
} from '../../store/settingsStore';
import { StorageSection } from './storage/StorageSection';
import { LaunchOptionsSection } from './LaunchOptionsSection';
import { PromptTemplatesSection } from './PromptTemplatesSection';
import { CloneRootsSection } from './CloneRootsSection';
import { AppearanceSection } from './AppearanceSection';
import { DefaultProviderSection } from './DefaultProviderSection';

/**
 * The settings modal: hosts the registry of per-provider CLI launch options,
 * the registry of prompt templates, and the registry of clone roots, each a
 * top-level category in a VS Code-style 2-pane layout. The left rail lists
 * categories; the right pane renders the active category's content. The
 * categories are conceptually unrelated (one targets session startup flags,
 * another reusable prompt text, another where to look for git repos to start
 * sessions in), so they live in separate panes rather than stacked sections —
 * keeping each category's UI undivided by the others.
 *
 * Rendered as a {@link Dialog} overlay layered on top of the workspace rather
 * than replacing the center pane, so the conversation stays in place beneath
 * it. Opened from the navigator's lower-left settings entry (`openSettings`)
 * and closed via the dialog's Close button, Esc, or a backdrop click
 * (`closeSettings`). The active category is persisted to localStorage so a
 * reload (or a dialog close/reopen) restores the last view.
 */
export function SettingsView() {
  const settingsOpen = useNavStore((state) => state.settingsOpen);
  const closeSettings = useNavStore((state) => state.closeSettings);
  const activeCategory = useSettingsStore((state) => state.activeCategory);
  const setActiveCategory = useSettingsStore((state) => state.setActiveCategory);

  // Single source of truth for category id + label + content. Adding a new
  // top-level category is one entry here plus a new id in `settingsStore.ts`.
  // The renderer receives `active` so each category can gate its data queries
  // on the dialog being open AND the category being the visible one — an
  // inactive category does not fire its initial fetch.
  const categories: {
    id: SettingsCategoryId;
    label: string;
    render: (active: boolean) => ReactNode;
  }[] = [
    {
      id: 'launch-options',
      label: 'Launch options',
      render: (active) => <LaunchOptionsSection active={active} />,
    },
    {
      id: 'prompt-templates',
      label: 'Prompt templates',
      render: (active) => <PromptTemplatesSection active={active} />,
    },
    {
      id: 'clone-roots',
      label: 'Clone roots',
      render: (active) => <CloneRootsSection active={active} />,
    },
    {
      id: 'appearance',
      label: 'Appearance',
      // The Appearance section has no data fetch of its own; the `active`
      // prop is ignored.
      render: () => <AppearanceSection />,
    },
    {
      id: 'default-provider',
      label: 'Default provider',
      // The Default provider section reads a persisted preference only; no data
      // fetch, so the `active` prop is ignored.
      render: () => <DefaultProviderSection />,
    },
    {
      id: 'storage',
      label: 'Storage',
      render: (active) => <StorageSection active={active} />,
    },
  ];

  const activeIndex = categories.findIndex((c) => c.id === activeCategory);
  const railRef = useRef<HTMLDivElement>(null);

  // Arrow Up / Arrow Down on the left rail moves focus between categories
  // (standard ARIA vertical-tablist behavior). Home / End jump to first /
  // last. We focus the destination button and switch the active category in
  // the same step, so the right pane follows the keyboard focus.
  const onRailKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (categories.length === 0) {
      return;
    }
    const current = activeIndex >= 0 ? activeIndex : 0;
    let next: number | null = null;
    if (event.key === 'ArrowDown') {
      next = (current + 1) % categories.length;
    } else if (event.key === 'ArrowUp') {
      next = (current - 1 + categories.length) % categories.length;
    } else if (event.key === 'Home') {
      next = 0;
    } else if (event.key === 'End') {
      next = categories.length - 1;
    }
    if (next === null) {
      return;
    }
    event.preventDefault();
    const target = categories[next];
    setActiveCategory(target.id);
    const button = railRef.current?.querySelector<HTMLButtonElement>(
      `[data-testid="settings-category-${target.id}"]`,
    );
    button?.focus();
  };

  const active = categories[activeIndex >= 0 ? activeIndex : 0];

  return (
    <Dialog
      open={settingsOpen}
      onClose={closeSettings}
      title="Settings"
      // Wider than the single-pane prompt: the 2-pane layout needs room for a
      // ~180px left rail plus the right pane's option rows (label + monospace
      // flag/value) without truncating either column.
      //
      // The height is a fixed frame rather than content-derived, because the
      // categories differ wildly in natural height (a two-option radio group
      // vs. a form plus an unbounded list). Sizing to the content made the
      // panel resize on every category switch, and since the backdrop centers
      // it, both edges moved — including the rail button the user had just
      // clicked. The frame belongs to the dialog, not to whichever category
      // happens to be showing; overflow is the right pane's business. `min()`
      // keeps it viewport-bound on short screens, where it degrades to the
      // `max-h-full` behavior it would have had anyway.
      className="h-[min(42rem,100%)] max-w-4xl"
      footer={
        <Button variant="ghost" onClick={closeSettings} data-testid="settings-close">
          Close
        </Button>
      }
    >
      <div className="flex h-full w-full gap-4">
        <div
          ref={railRef}
          role="tablist"
          aria-label="Settings categories"
          aria-orientation="vertical"
          className="flex w-44 shrink-0 flex-col gap-1 border-r border-border-default pr-3"
          data-testid="settings-categories"
          onKeyDown={onRailKeyDown}
        >
          {categories.map((category) => {
            const selected = category.id === active.id;
            return (
              <button
                key={category.id}
                type="button"
                role="tab"
                aria-selected={selected}
                aria-controls={`settings-panel-${category.id}`}
                id={`settings-tab-${category.id}`}
                // The active tab is in the focus order; the others are skipped
                // by Tab and reached via the arrow-key handler above (standard
                // ARIA "roving tabindex" pattern for a tablist).
                tabIndex={selected ? 0 : -1}
                onClick={() => setActiveCategory(category.id)}
                className={cn(
                  'rounded px-3 py-1.5 text-left text-caption font-medium transition',
                  selected
                    ? 'bg-accent/10 text-accent ring-1 ring-accent/30'
                    : 'text-fg-muted hover:bg-surface-elevated hover:text-fg',
                )}
                data-testid={`settings-category-${category.id}`}
              >
                {category.label}
              </button>
            );
          })}
        </div>
        <div
          role="tabpanel"
          id={`settings-panel-${active.id}`}
          aria-labelledby={`settings-tab-${active.id}`}
          // The right pane scrolls independently of the rail so a long
          // launch-options list never pushes the rail out of view.
          // `scrollbar-hover` (not `scrollbar-none`): the fixed-frame dialog
          // clips overflowing categories with no other cue that more content
          // exists, so keep a hover-revealed thumb as a position indicator.
          className="min-w-0 flex-1 overflow-y-auto scrollbar-hover"
          data-testid={`settings-panel-${active.id}`}
        >
          {active.render(settingsOpen)}
        </div>
      </div>
    </Dialog>
  );
}
