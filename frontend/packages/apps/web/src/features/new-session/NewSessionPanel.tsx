import { cn } from '@delta/ui-kit';
import {
  type NewSessionTab,
  useComposerStore,
} from '../../store/composerStore';
import { DirectoryTab } from './tabs/DirectoryTab';
import { PRTab } from './tabs/PRTab';
import { RepositoryTab } from './tabs/RepositoryTab';

/**
 * The new-session screen's three-tab container. Rendered above the composer
 * card in the new-session state. The active tab's body is the only one
 * mounted, so its data queries are gated on visibility for free.
 *
 * - `PR` (placeholder in Phase B) — listing reviewer/author PRs is wired up
 *   in Phase C; for now the tab shows an empty state describing what is
 *   coming and pointing at `gh auth login`.
 * - `Repository` — registered repos (one per upstream, multiple clones
 *   bundled), recency-ordered. Selecting a repo reveals its clone list with
 *   the most-recent clone pre-selected; selecting a clone fills
 *   `composerStore.newSessionWorkdir` so the composer card below knows where
 *   to spawn. The detailed worktree-options and launch-options UI stays
 *   visible below as overrides.
 * - `Directory` — the old Recent + Browse picker, moved inline. The same
 *   `Recent` list as the now-retired auto-opened modal.
 */
export function NewSessionPanel() {
  const activeTab = useComposerStore((state) => state.newSessionTab);
  const setActiveTab = useComposerStore((state) => state.setNewSessionTab);

  return (
    <section
      className="space-y-3"
      data-testid="new-session-panel"
    >
      <TabBar activeTab={activeTab} onSelect={setActiveTab} />
      <div data-testid="new-session-tab-content">
        {activeTab === 'pr' && <PRTab />}
        {activeTab === 'repository' && <RepositoryTab />}
        {activeTab === 'directory' && <DirectoryTab />}
      </div>
    </section>
  );
}

interface TabBarProps {
  activeTab: NewSessionTab;
  onSelect: (tab: NewSessionTab) => void;
}

/**
 * Three pill-shaped tab buttons. The choice is recorded in the persisted
 * composer store, so the active tab survives a reload.
 */
function TabBar({ activeTab, onSelect }: TabBarProps) {
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
      className="flex items-center gap-1 rounded-md border border-slate-200 bg-white p-1"
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
            'flex-1 rounded px-3 py-1.5 text-xs font-medium transition',
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
