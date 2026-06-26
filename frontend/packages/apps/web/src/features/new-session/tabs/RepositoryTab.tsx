import { useCallback, useEffect, useMemo, useState } from 'react';
import { useHomeDirQuery, useRepositoriesQuery } from '@delta/api-client';
import { displayBranch } from '@delta/model';
import type { RepositoryEntry } from '@delta/wire-gen';
import { Spinner, cn } from '@delta/ui-kit';
import { useApiClient } from '../../../data/apiContext';
import { useComposerStore } from '../../../store/composerStore';
import { displayPath } from '../../../utils/displayPath';

/**
 * Identity-key prefix the picker uses to flag a "this is a path, not an
 * origin URL" repository entry. Backend returns the absolute clone path as
 * the identity key whenever `origin` is unset; we surface that with a
 * subtle pill in the row so it is clear why this entry is not bundled.
 */
const PATH_KEY_PREFIX = '/';

/**
 * The Repository tab: registered repositories (origin-deduplicated),
 * recency-ordered. Clicking a Repository row expands the per-clone picker
 * and auto-picks the repo's `recently_used_clone_path` (falling back to
 * the first clone) into `composerStore.newSessionWorkdir`, so a user who
 * picks a repo and presses Send can spawn from a sensible default without
 * having to click a clone first. Picking a different clone from the same
 * repo overrides that default; clicking a different Repository row
 * replaces the picked clone with the new repo's default.
 *
 * The initial highlight of the first (most-recent) repo on mount is local
 * UI state only — it does NOT write to the composer store, so just opening
 * the New session screen never leaks a workdir into the other tabs.
 *
 * Per-clone state — branch, launch options, worktree opt-in — will pre-fill
 * once per-session persistence lands. Today the existing override UI under
 * the composer card remains the authoritative knobs.
 */
export function RepositoryTab() {
  const client = useApiClient();
  const repositoriesQuery = useRepositoriesQuery(client, true);
  const setSelected = useComposerStore((state) => state.setNewSessionWorkdir);
  const selectedPath = useComposerStore((state) => state.newSessionWorkdir);
  const setNewSessionSelectedPrUrl = useComposerStore(
    (state) => state.setNewSessionSelectedPrUrl,
  );
  // Display-only home abbreviation. The stored `newSessionWorkdir` stays
  // the absolute path.
  const home = useHomeDirQuery(client, true).data?.path ?? null;

  const repositories = repositoriesQuery.data?.repositories ?? [];

  // Selected repo, by identity_key — drives the expanded clone list.
  // Defaults to the first (most-recent) repo so the panel reads as "pick a
  // clone" out of the gate rather than "first pick a repo, then pick a
  // clone".
  const [selectedRepoKey, setSelectedRepoKey] = useState<string | null>(null);
  useEffect(() => {
    if (
      selectedRepoKey === null &&
      repositories.length > 0
    ) {
      setSelectedRepoKey(repositories[0].identity_key);
    }
  }, [selectedRepoKey, repositories]);

  const selectedRepo = useMemo(
    () =>
      repositories.find((repo) => repo.identity_key === selectedRepoKey) ?? null,
    [repositories, selectedRepoKey],
  );

  // Clicking a Repository row both highlights it and auto-picks a clone path
  // into the composer store, so the user can press Send (or Cmd/Ctrl+Enter)
  // without first clicking a clone. Preference order:
  //   1. `recently_used_clone_path` (when it is actually in the clones list)
  //   2. the first clone
  // The auto-pick is skipped when the current selection already belongs to
  // this repo — that means the user explicitly picked a different clone of
  // the same repo and we must not stomp it (e.g. re-clicking the same Repo
  // row, or arriving via a clone click that ran the existing repo-auto-
  // select effect).
  //
  // Crucially the auto-pick lives on the click, not on the mount-time
  // `selectedRepoKey` defaulting effect: just opening the New session
  // screen must not write to the store, otherwise the workdir leaks into
  // the PR / Directory tabs the user later switches to.
  const handleRepoClick = useCallback(
    (repo: RepositoryEntry) => {
      setSelectedRepoKey(repo.identity_key);
      if (repo.clones.length === 0) {
        return;
      }
      const alreadyPicked = repo.clones.some(
        (clone) => clone.path === selectedPath,
      );
      if (alreadyPicked) {
        return;
      }
      const recent = repo.recently_used_clone_path;
      const recentMatches = repo.clones.some((clone) => clone.path === recent);
      const next = recentMatches ? recent : repo.clones[0].path;
      setSelected(next);
    },
    [selectedPath, setSelected],
  );

  if (repositoriesQuery.isLoading) {
    return <Spinner label="Loading repositories…" />;
  }

  if (repositoriesQuery.isError) {
    return (
      <div
        className="rounded border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700"
        data-testid="repository-tab-error"
        role="alert"
      >
        Could not load repositories. The Directory tab still works.
      </div>
    );
  }

  if (repositories.length === 0) {
    return (
      <div
        className="rounded-md border border-dashed border-slate-300 bg-slate-50 px-4 py-6 text-sm text-slate-600"
        data-testid="repository-tab-empty"
      >
        <p className="font-medium text-slate-700">No repositories yet.</p>
        <p className="mt-1 text-xs text-slate-500">
          Start a session via the Directory tab to register your first repo.
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-3" data-testid="repository-tab">
      <section className="space-y-1" data-testid="repository-tab-repos">
        <h3 className="text-xs font-semibold uppercase tracking-wide text-slate-500">
          Repositories
        </h3>
        <ul className="space-y-0.5">
          {repositories.map((repo) => (
            <li key={repo.identity_key}>
              <RepoRow
                repo={repo}
                isSelected={selectedRepoKey === repo.identity_key}
                onSelect={() => handleRepoClick(repo)}
              />
            </li>
          ))}
        </ul>
      </section>

      {selectedRepo && (
        <section className="space-y-1" data-testid="repository-tab-clones">
          <h3 className="text-xs font-semibold uppercase tracking-wide text-slate-500">
            Clones
          </h3>
          <ul className="space-y-0.5">
            {selectedRepo.clones.map((clone) => {
              const isPicked = selectedPath === clone.path;
              return (
                <li key={clone.path}>
                  <button
                    type="button"
                    onClick={() => {
                      // Picking a clone here is mutually exclusive with the PR
                      // tab's "selected row" highlight — clear it so at most
                      // one row reads as the active pick across the three tabs.
                      setSelected(clone.path);
                      setNewSessionSelectedPrUrl(null);
                    }}
                    aria-pressed={isPicked}
                    className={cn(
                      'flex w-full min-w-0 items-center justify-between gap-3 rounded px-2 py-1.5 text-left text-xs hover:bg-slate-100',
                      isPicked
                        ? 'bg-indigo-50 text-indigo-700 ring-1 ring-indigo-200'
                        : 'text-slate-700',
                    )}
                    title={clone.path}
                    data-testid="repository-tab-clone-row"
                    data-default={
                      clone.path === selectedRepo.recently_used_clone_path
                        ? 'true'
                        : 'false'
                    }
                  >
                    <span className="min-w-0 flex-1 truncate font-mono">
                      {displayPath(clone.path, home)}
                    </span>
                    {clone.last_branch && (
                      <span
                        className="shrink-0 rounded bg-slate-100 px-1.5 py-0.5 font-mono text-[0.7rem] text-slate-500"
                        title={clone.last_branch}
                      >
                        {displayBranch(clone.last_branch)}
                      </span>
                    )}
                  </button>
                </li>
              );
            })}
          </ul>
        </section>
      )}
    </div>
  );
}

interface RepoRowProps {
  repo: RepositoryEntry;
  isSelected: boolean;
  onSelect: () => void;
}

function RepoRow({ repo, isSelected, onSelect }: RepoRowProps) {
  // The identity_key falls back to the path when `origin` was unset — surface
  // that with a subtle pill so the entry is not mistaken for one of the
  // origin-deduplicated repos. A path-shaped key always starts with `/`.
  const isPathKey = repo.identity_key.startsWith(PATH_KEY_PREFIX);
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={isSelected}
      className={cn(
        'flex w-full min-w-0 items-center justify-between gap-3 rounded px-2 py-1.5 text-left hover:bg-slate-100',
        isSelected
          ? 'bg-indigo-50 text-indigo-700 ring-1 ring-indigo-200'
          : 'text-slate-700',
      )}
      data-testid="repository-tab-repo-row"
    >
      <div className="min-w-0 flex-1">
        <div className="truncate text-xs font-medium">{repo.display_name}</div>
        <div className="truncate font-mono text-[0.7rem] text-slate-500">
          {repo.identity_key}
        </div>
      </div>
      {isPathKey && (
        <span className="shrink-0 rounded bg-slate-100 px-1.5 py-0.5 text-[0.65rem] uppercase tracking-wide text-slate-500">
          local
        </span>
      )}
      {repo.clones.length > 1 && (
        <span className="shrink-0 rounded bg-slate-100 px-1.5 py-0.5 text-[0.65rem] text-slate-600">
          {repo.clones.length} clones
        </span>
      )}
    </button>
  );
}
