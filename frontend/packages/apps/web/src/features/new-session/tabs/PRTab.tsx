import { Fragment, useMemo } from 'react';
import { usePullRequestsQuery, useRepositoriesQuery } from '@delta/api-client';
import { displayBranch } from '@delta/model';
import type { PullRequest, PullRequestsResponse } from '@delta/wire-gen';
import { Spinner, cn } from '@delta/ui-kit';
import { useApiClient } from '../../../data/apiContext';
import { useComposerStore } from '../../../store/composerStore';

/**
 * The Pull Request tab: two side-by-side lenses backed by `gh search`.
 *
 * - "Requested for your review" — reviewer lens, drafts excluded.
 *   These are the inbox-style "your turn" PRs.
 * - "Yours" — author lens, drafts included so an in-flight draft
 *   branch can be resumed in one click.
 *
 * Clicking a row pre-fills the composer:
 * - workdir = the registered clone's `recently_used_clone_path`;
 * - worktree on, with `start_point.kind = 'use_remote_branch'` keyed
 *   to the PR's head ref. (The head ref is by definition a
 *   non-default branch, which is the worktree-default-ON rule.)
 *
 * A row whose repo has no registered local clone is visibly
 * de-emphasised and silently un-clickable, with an inline hint
 * pointing at `gh repo clone <owner>/<repo>` — that is the unblock,
 * not a Delta-side action.
 *
 * When `gh` is missing or unauthenticated the use case reports
 * `gh_available: false`, the list is empty, and a small slate-tone
 * banner points at `gh auth login`. The tab keeps rendering — it does
 * not display a full-screen error — so the rest of the picker stays
 * usable.
 */
export function PRTab() {
  const client = useApiClient();
  // Two independent queries so flipping between sections does not
  // invalidate the other (or worse, refetch them both on every render).
  const reviewerQuery = usePullRequestsQuery(client, 'reviewer', true);
  const authorQuery = usePullRequestsQuery(client, 'author', true);
  // The repositories query backs the "where do I send the worktree to?"
  // lookup — `usePullRequestsQuery` only tells us whether a clone
  // exists; the actual default path comes from
  // `recently_used_clone_path` on the matching repository entry.
  const repositoriesQuery = useRepositoriesQuery(client, true);

  const setNewSessionWorkdir = useComposerStore(
    (state) => state.setNewSessionWorkdir,
  );
  const setNewSessionWorktreeEnabled = useComposerStore(
    (state) => state.setNewSessionWorktreeEnabled,
  );
  const setNewSessionWorktreeStartPoint = useComposerStore(
    (state) => state.setNewSessionWorktreeStartPoint,
  );
  const selectedPrUrl = useComposerStore(
    (state) => state.newSessionSelectedPrUrl,
  );
  const setNewSessionSelectedPrUrl = useComposerStore(
    (state) => state.setNewSessionSelectedPrUrl,
  );

  // The PR endpoint's `gh_available` is the same value for both
  // lenses (it reflects whether `gh auth status` worked, not the
  // lens choice), so a single read is enough. Treat a missing
  // response as "still loading" — the banner only fires after at
  // least one query has resolved.
  const ghAvailable = !!(
    reviewerQuery.data?.gh_available ?? authorQuery.data?.gh_available
  );
  const isAnyLoading = reviewerQuery.isLoading || authorQuery.isLoading;
  const isAnyError = reviewerQuery.isError || authorQuery.isError;

  // Build the `<owner>/<repo>` → recently-used clone-path index so a
  // row click resolves the registered clone in O(1).
  const cloneIndex = useMemo(
    () => buildCloneIndex(repositoriesQuery.data?.repositories ?? []),
    [repositoriesQuery.data?.repositories],
  );

  const onPickPr = (pr: PullRequest) => {
    if (!pr.has_local_clone) {
      // Silently blocked. The inline hint on the row tells the user
      // how to unblock (clone the repo locally). No state change.
      return;
    }
    const clonePath = cloneIndex.get(repoKey(pr.repo_owner, pr.repo_name));
    if (!clonePath) {
      // `has_local_clone` claimed a clone exists but the index
      // resolved nothing — this is a torn read between the gh result
      // and the repositories list. Treat it like the no-clone path
      // rather than committing to a nonsense workdir.
      return;
    }
    setNewSessionWorkdir(clonePath);
    setNewSessionWorktreeEnabled(true);
    // PR head refs are non-default branches; cut the worktree to
    // check the branch out itself (the `use_remote_branch` mode), so
    // resuming a PR's work simply attaches to its branch. A
    // cross-fork PR (head_repo_owner != repo_owner) currently still
    // resolves the branch from the local clone's `origin` — letting
    // `git worktree add` handle the actual fetch failure if the
    // branch is not reachable, rather than blocking at the click.
    setNewSessionWorktreeStartPoint({
      kind: 'use_remote_branch',
      name: pr.head_ref,
    });
    // Mark the row as the active pick so it gets the indigo "you picked
    // this" highlight. Set last: the earlier writes synchronously trigger
    // `setNewSessionWorkdir`'s reset side-effects (see composerStore) —
    // those don't touch the PR url, but the order keeps it obvious that
    // the highlight reflects the final committed pick.
    setNewSessionSelectedPrUrl(pr.url);
  };

  if (isAnyLoading) {
    return <Spinner label="Loading pull requests…" />;
  }

  if (isAnyError) {
    return (
      <div
        className="rounded border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700"
        data-testid="pr-tab-error"
        role="alert"
      >
        Could not load pull requests. The Repository tab still works.
      </div>
    );
  }

  return (
    <div className="space-y-3" data-testid="new-session-pr-tab">
      {!ghAvailable && <GhUnavailableHint />}
      <PrSection
        testId="pr-tab-reviewer"
        heading="Requested for your review"
        emptyMessage="No reviewer-requested PRs."
        data={reviewerQuery.data}
        onPick={onPickPr}
        selectedPrUrl={selectedPrUrl}
      />
      <PrSection
        testId="pr-tab-author"
        heading="Yours"
        emptyMessage="No open PRs you authored."
        data={authorQuery.data}
        onPick={onPickPr}
        selectedPrUrl={selectedPrUrl}
      />
    </div>
  );
}

/**
 * Index from `<owner>/<repo>` to the registered clone path the picker
 * should pre-fill when a row of that repo is clicked. Only matches the
 * `github.com`-shaped identity keys; path-keyed entries (origin unset)
 * can never collide with a PR by construction.
 */
function buildCloneIndex(
  repositories: import('@delta/wire-gen').RepositoryEntry[],
): Map<string, string> {
  const index = new Map<string, string>();
  for (const repo of repositories) {
    // The backend normaliser produces `github.com/<owner>/<repo>` for
    // GitHub origins. A leading `/` flags a path-keyed entry.
    if (!repo.identity_key.startsWith('github.com/')) {
      continue;
    }
    const segments = repo.identity_key.split('/');
    // host/owner/repo or host/owner/.../repo — take the last two
    // segments as `owner/repo`, matching the wire `display_name` rule.
    if (segments.length < 3) {
      continue;
    }
    const key = `${segments[segments.length - 2]}/${segments[segments.length - 1]}`;
    index.set(key, repo.recently_used_clone_path);
  }
  return index;
}

function repoKey(owner: string, name: string): string {
  return `${owner}/${name}`;
}

interface PrSectionProps {
  testId: string;
  heading: string;
  emptyMessage: string;
  data: PullRequestsResponse | undefined;
  onPick: (pr: PullRequest) => void;
  selectedPrUrl: string | null;
}

function PrSection({
  testId,
  heading,
  emptyMessage,
  data,
  onPick,
  selectedPrUrl,
}: PrSectionProps) {
  const prs = data?.pull_requests ?? [];
  // Adjacency-group the section's rows by repo so a multi-PR repo
  // visually clusters. Sort by repo first, then by recency within
  // each repo, so the "newer first" overall reading is preserved
  // inside each cluster.
  const grouped = useMemo(() => groupPrsByRepo(prs), [prs]);

  return (
    <section className="space-y-1" data-testid={testId}>
      <h3 className="text-xs font-semibold uppercase tracking-wide text-slate-500">
        {heading}
      </h3>
      {grouped.length === 0 ? (
        <p className="text-xs text-slate-500">{emptyMessage}</p>
      ) : (
        <ul className="space-y-0.5">
          {grouped.map((row, index) => (
            <Fragment
              key={`${row.pr.repo_owner}/${row.pr.repo_name}#${row.pr.number}`}
            >
              {row.isRepoFirstRow && index > 0 && (
                <li
                  role="separator"
                  data-testid="pr-tab-repo-divider"
                  className="my-1 border-t border-slate-200"
                />
              )}
              <li>
                <PrRow
                  pr={row.pr}
                  onPick={onPick}
                  isSelected={row.pr.url === selectedPrUrl}
                />
              </li>
            </Fragment>
          ))}
        </ul>
      )}
    </section>
  );
}

interface GroupedPrRow {
  pr: PullRequest;
  /** True for the first PR row inside each repo's cluster — drives
   *  the horizontal divider rendered above the row so adjacent repo
   *  clusters are visually separated. */
  isRepoFirstRow: boolean;
}

function groupPrsByRepo(prs: PullRequest[]): GroupedPrRow[] {
  const sorted = [...prs].sort((a, b) => {
    const aRepo = `${a.repo_owner}/${a.repo_name}`;
    const bRepo = `${b.repo_owner}/${b.repo_name}`;
    if (aRepo !== bRepo) {
      return aRepo < bRepo ? -1 : 1;
    }
    // Newer first inside a repo cluster.
    return a.updated_at < b.updated_at ? 1 : -1;
  });
  let lastRepo: string | null = null;
  return sorted.map((pr) => {
    const repo = `${pr.repo_owner}/${pr.repo_name}`;
    const isRepoFirstRow = repo !== lastRepo;
    lastRepo = repo;
    return { pr, isRepoFirstRow };
  });
}

interface PrRowProps {
  pr: PullRequest;
  onPick: (pr: PullRequest) => void;
  isSelected: boolean;
}

function PrRow({ pr, onPick, isSelected }: PrRowProps) {
  const disabled = !pr.has_local_clone;
  const repoLabel = `${pr.repo_owner}/${pr.repo_name}#${pr.number}`;
  const cloneHint = `gh repo clone ${pr.repo_owner}/${pr.repo_name}`;
  // A disabled row stays a silent no-op on click, so it can never end up
  // being the "selected" row even if state somehow held its url. Gating
  // the highlight on `!disabled` also keeps the styling intent explicit:
  // the indigo pill means "you picked this", which a non-clickable row
  // by definition cannot be.
  const showSelected = isSelected && !disabled;
  return (
    <button
      type="button"
      onClick={() => onPick(pr)}
      // The "click is silently blocked" rule means the button stays
      // mounted (so the inline hint still shows) but its handler is
      // a no-op. `aria-disabled` (not `disabled`) keeps the row
      // discoverable to screen readers while signalling the state.
      aria-disabled={disabled}
      aria-pressed={showSelected}
      data-testid="pr-tab-row"
      data-has-local-clone={pr.has_local_clone ? 'true' : 'false'}
      data-selected={showSelected ? 'true' : 'false'}
      title={disabled ? `No local clone — run \`${cloneHint}\` somewhere first.` : pr.url}
      className={cn(
        'flex w-full min-w-0 flex-col items-start gap-0.5 rounded px-2 py-1.5 text-left text-xs',
        disabled
          ? 'cursor-not-allowed opacity-60'
          : showSelected
            ? 'bg-indigo-50 text-indigo-700 ring-1 ring-indigo-200'
            : 'text-slate-700 hover:bg-slate-100',
      )}
    >
      <div className="flex w-full min-w-0 items-center gap-2">
        <span className="shrink-0 font-mono text-xs font-medium text-slate-700">
          {repoLabel}
        </span>
        <span className="min-w-0 flex-1 truncate">{pr.title}</span>
        {pr.draft && (
          <span className="shrink-0 rounded bg-amber-100 px-1.5 py-0.5 text-[0.65rem] uppercase tracking-wide text-amber-700">
            draft
          </span>
        )}
      </div>
      <div className="flex w-full min-w-0 items-center gap-2 text-[0.7rem] text-slate-500">
        <span className="shrink-0 font-mono" title={pr.head_ref}>
          {displayBranch(pr.head_ref)}
        </span>
        <span aria-hidden>·</span>
        <span className="shrink-0">{formatRelative(pr.updated_at)}</span>
        <span aria-hidden>·</span>
        <span className="shrink-0">{pr.author_login}</span>
      </div>
      {disabled && (
        <p
          className="mt-0.5 text-[0.7rem] text-slate-500"
          data-testid="pr-tab-row-no-clone-hint"
        >
          No local clone — run <code className="rounded bg-slate-200 px-1 font-mono">{cloneHint}</code> somewhere first.
        </p>
      )}
    </button>
  );
}

function GhUnavailableHint() {
  return (
    <p
      className="text-xs text-slate-500"
      data-testid="pr-tab-gh-unavailable"
    >
      Run{' '}
      <code className="rounded bg-slate-200 px-1 py-0.5 font-mono text-[0.7rem] text-slate-700">
        gh auth login
      </code>{' '}
      to enable this tab.
    </p>
  );
}

/**
 * Coarse-grained "x ago" formatter so the rows do not need a heavy
 * date library. Falls back to the ISO timestamp on parse failure so
 * an unexpected gh date format still renders something readable.
 */
function formatRelative(iso: string): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) {
    return iso;
  }
  const ms = Date.now() - then;
  const minutes = Math.round(ms / 60000);
  if (minutes < 1) {
    return 'just now';
  }
  if (minutes < 60) {
    return `${minutes}m ago`;
  }
  const hours = Math.round(minutes / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }
  const days = Math.round(hours / 24);
  if (days < 30) {
    return `${days}d ago`;
  }
  const months = Math.round(days / 30);
  if (months < 12) {
    return `${months}mo ago`;
  }
  const years = Math.round(months / 12);
  return `${years}y ago`;
}
