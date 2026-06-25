import { cn } from '@delta/ui-kit';
import {
  type NewSessionTab,
  useComposerStore,
} from '../../store/composerStore';

/**
 * The PR / Repository / Directory tab strip for the new-session screen.
 *
 * Lives in its own file so {@link TranscriptPane} can mount it in the Panel's
 * sticky header slot (above the scrolling body), keeping the tabs pinned while
 * the active tab's list area scrolls underneath. {@link NewSessionPanel}
 * renders only the active tab's body — no inline tabs — so the strip never
 * shows twice. The active tab is read from / written to the persisted composer
 * store, so the choice survives reloads.
 *
 * The container styling stays flush in the Panel header (no extra outer
 * border / shadow) and uses internal padding to match the body content's
 * horizontal inset.
 */
export function NewSessionTabBar() {
  const activeTab = useComposerStore((state) => state.newSessionTab);
  const onSelect = useComposerStore((state) => state.setNewSessionTab);

  // Single source of truth for label + ordering so the buttons render in the
  // intended PR / Repository / Directory left-to-right order regardless of
  // how the union type is declared in the store.
  const tabs: { id: NewSessionTab; label: string }[] = [
    { id: 'pr', label: 'PR' },
    { id: 'repository', label: 'Repository' },
    { id: 'directory', label: 'Directory' },
  ];
  return (
    <div
      role="tablist"
      aria-label="Start a session from"
      className="inline-flex items-center gap-1"
      data-testid="new-session-tabs"
    >
      {tabs.map((tab) => (
        <button
          key={tab.id}
          type="button"
          role="tab"
          aria-selected={activeTab === tab.id}
          onClick={() => onSelect(tab.id)}
          className={cn(
            'rounded px-3 py-1.5 text-xs font-medium transition',
            activeTab === tab.id
              ? 'bg-indigo-50 text-indigo-700 ring-1 ring-indigo-200'
              : 'text-slate-600 hover:bg-slate-50 hover:text-slate-800',
          )}
          data-testid={`new-session-tab-${tab.id}`}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
}
