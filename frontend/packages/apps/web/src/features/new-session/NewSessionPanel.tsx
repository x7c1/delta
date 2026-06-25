import { useComposerStore } from '../../store/composerStore';
import { DirectoryTab } from './tabs/DirectoryTab';
import { PRTab } from './tabs/PRTab';
import { RepositoryTab } from './tabs/RepositoryTab';

/**
 * The new-session screen's body: renders the active tab's content.
 * The tab strip itself lives in {@link NewSessionTabBar} so
 * {@link TranscriptPane} can mount it in the Panel header slot (above the
 * scrolling body), keeping it pinned while the list area scrolls underneath.
 * Only the active tab's body is mounted, so its data queries are gated on
 * visibility for free.
 *
 * - `PR` — registered repos' open PRs (reviewer / author lenses), each row
 *   pre-filling `composerStore.newSessionWorkdir` plus the worktree branch
 *   for that PR head when clicked.
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

  return (
    <section
      className="space-y-3"
      data-testid="new-session-panel"
    >
      <div data-testid="new-session-tab-content">
        {activeTab === 'pr' && <PRTab />}
        {activeTab === 'repository' && <RepositoryTab />}
        {activeTab === 'directory' && <DirectoryTab />}
      </div>
    </section>
  );
}
