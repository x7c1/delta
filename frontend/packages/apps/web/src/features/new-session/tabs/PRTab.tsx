import { Fragment, useEffect, useMemo, useState } from 'react';
import {
  ApiError,
  useAddCloneRootMutation,
  useCloneRepositoryMutation,
  useCloneRootsQuery,
  usePullRequestsQuery,
  useRepositoriesQuery,
} from '@delta/api-client';
import { displayBranch } from '@delta/model';
import type { PullRequest, PullRequestsResponse } from '@delta/wire-gen';
import { Button, Spinner, cn } from '@delta/ui-kit';
import { useApiClient } from '../../../data/apiContext';
import { useComposerStore } from '../../../store/composerStore';
import { useLiveStore } from '../../../store/liveStore';

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
 *   non-default branch, which is why a PR pick always runs in a
 *   worktree rather than offering the choice.)
 * - `pr` provenance on the workdir, which both highlights the picked
 *   row and locks the composer's worktree controls to that branch —
 *   the session is for the PR, so there is nothing left to choose.
 *
 * A row whose repo has no registered local clone is not a dead end:
 * clicking it expands an inline clone panel under the row, and the
 * dialog stays exactly where it is. The panel resolves the one thing
 * Delta still needs — which clone root to put it in — and nothing
 * else: with no registered root it offers a path input that registers
 * one first, with exactly one it just names the destination, and with
 * several it offers a selector defaulting to the most recently
 * registered. `gh` is already known to be authenticated (this tab is
 * gated on it), so nothing else has to be asked.
 *
 * The clone runs server-side; the row spins while it does, and every
 * other row, tab, and the composer stay interactive. When the
 * completion event lands, the tab auto-continues into the normal PR
 * pick — but only while that clone's intent is still the active one.
 * If the user picked something else, started a session, or left the
 * tab in the meantime, the refetch simply enables the row and the
 * composer is left alone; a finished clone never hijacks a choice the
 * user has since made.
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

  const setNewSessionWorkdirFromPr = useComposerStore(
    (state) => state.setNewSessionWorkdirFromPr,
  );
  // The picked row's highlight follows the workdir's provenance, so it can only
  // ever agree with what the composer will actually launch: any later directory
  // pick resets the provenance and the highlight goes with it.
  const workdirSource = useComposerStore(
    (state) => state.newSessionWorkdirSource,
  );
  const selectedPrUrl = workdirSource.kind === 'pr' ? workdirSource.url : null;

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

  // The clone roots are only consulted by the clone panel, so the query stays
  // off until a row without a clone is actually on screen.
  const hasRowWithoutClone =
    (reviewerQuery.data?.pull_requests ?? []).some((pr) => !pr.has_local_clone) ||
    (authorQuery.data?.pull_requests ?? []).some((pr) => !pr.has_local_clone);
  const cloneRootsQuery = useCloneRootsQuery(client, hasRowWithoutClone);
  const cloneRoots = useMemo(
    () => (cloneRootsQuery.data?.clone_roots ?? []).map((root) => root.path),
    [cloneRootsQuery.data?.clone_roots],
  );

  const addCloneRoot = useAddCloneRootMutation(client);
  const cloneRepository = useCloneRepositoryMutation(client);

  // Which row's clone panel is open. One at a time: the panel is a decision the
  // user is making right now, and two open panels would offer two.
  const [openClonePanelUrl, setOpenClonePanelUrl] = useState<string | null>(null);
  // The request-time refusal for the open panel (an unregistered root, an
  // occupied destination). Distinct from the job failure below, which arrives on
  // the event stream long after the request was accepted.
  const [requestError, setRequestError] = useState<string | null>(null);

  const cloneIntent = useLiveStore((state) => state.cloneIntent);
  const cloneCompletion = useLiveStore((state) => state.cloneCompletion);
  const cloneFailure = useLiveStore((state) => state.cloneFailure);
  const startCloneIntent = useLiveStore((state) => state.startCloneIntent);
  const clearCloneIntent = useLiveStore((state) => state.clearCloneIntent);
  const clearCloneCompletion = useLiveStore(
    (state) => state.clearCloneCompletion,
  );

  // Leaving this tab — closing the new-session screen, switching tabs, starting
  // a session — retires the intent. The clone keeps running server-side and the
  // row still flips on the refetch; what stops is the auto-continue, because the
  // user is no longer where they asked for it.
  useEffect(() => () => clearCloneIntent(), [clearCloneIntent]);

  // The auto-continue. Reached only while the intent is still active (the store
  // drops any completion whose intent was superseded), and only while this tab
  // is mounted — so a clone that lands after the user moved on is inert. The
  // destination comes from the event itself, so the pick does not have to wait
  // for the repository list to refetch.
  useEffect(() => {
    if (cloneCompletion === null) {
      return;
    }
    setNewSessionWorkdirFromPr(cloneCompletion.destination, cloneCompletion.pr);
    setOpenClonePanelUrl(null);
    clearCloneCompletion();
  }, [cloneCompletion, setNewSessionWorkdirFromPr, clearCloneCompletion]);

  const onPickPr = (pr: PullRequest) => {
    if (!pr.has_local_clone) {
      // No clone yet: open (or close) this row's inline clone panel. The dialog
      // stays put — cloning is a step of this flow, not a trip to Settings.
      setRequestError(null);
      setOpenClonePanelUrl((current) => (current === pr.url ? null : pr.url));
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
    // Picking a different PR supersedes any clone the user was waiting on: they
    // have chosen where this compose is going, and a clone landing afterwards
    // must not overwrite that.
    clearCloneIntent();
    setOpenClonePanelUrl(null);
    // A cross-fork PR (head_repo_owner != repo_owner) currently still resolves
    // the branch from the local clone's `origin` — letting `git worktree add`
    // handle the actual fetch failure if the branch is not reachable, rather
    // than blocking at the click.
    setNewSessionWorkdirFromPr(clonePath, pr);
  };

  /**
   * Ask the server to clone `pr`'s repository into `cloneRoot`, registering that
   * root first when the user typed a fresh one.
   *
   * The intent is recorded only once the server has accepted (`202`): recording
   * it earlier would leave a row spinning forever behind a request that was
   * refused.
   */
  const requestClone = async (
    pr: PullRequest,
    cloneRoot: string,
    registerFirst: boolean,
  ) => {
    setRequestError(null);
    // The clone must name the root exactly as `GET /api/clone-roots` spells it,
    // because the server matches the registered path verbatim. A root the user
    // typed is not that spelling yet — registration canonicalises it (trailing
    // slashes stripped) — so the clone follows the row the registration
    // returned, not the text in the input. Otherwise `/home/dev/projects/`
    // would register fine and then be refused as "not a registered clone root".
    let registeredRoot = cloneRoot;
    try {
      if (registerFirst) {
        registeredRoot = (await addCloneRoot.mutateAsync({ path: cloneRoot }))
          .path;
      }
      await cloneRepository.mutateAsync({
        repo_owner: pr.repo_owner,
        repo_name: pr.repo_name,
        clone_root: registeredRoot,
      });
    } catch (error) {
      setRequestError(
        error instanceof ApiError || error instanceof Error
          ? error.message
          : 'Could not start the clone.',
      );
      return;
    }
    // The destination is the server's rule, not a guess: `<clone_root>/<name>`,
    // with no fallback naming. It is what the completion event will carry, and
    // matching on it is how the store knows the event is this request's.
    startCloneIntent({
      pr,
      destination: `${trimTrailingSlash(registeredRoot)}/${pr.repo_name}`,
    });
  };

  if (isAnyLoading) {
    return <Spinner label="Loading pull requests…" />;
  }

  if (isAnyError) {
    return (
      <div
        className="rounded border border-danger/30 bg-danger/10 px-3 py-2 text-caption text-danger"
        data-testid="pr-tab-error"
        role="alert"
      >
        Could not load pull requests. The Repository tab still works.
      </div>
    );
  }

  const clone: CloneControls = {
    roots: cloneRoots,
    rootsLoading: cloneRootsQuery.isLoading,
    openPanelUrl: openClonePanelUrl,
    // A row is busy from the moment the server accepts until its event lands.
    // The request itself is also covered, so a slow POST does not leave a dead
    // Clone button that invites a second click.
    busyPrUrl:
      cloneIntent?.pr.url ??
      (addCloneRoot.isPending || cloneRepository.isPending
        ? openClonePanelUrl
        : null),
    // Whichever failure is current: a refusal from the request, or the job's own
    // message from the event stream.
    error: requestError ?? cloneFailure?.message ?? null,
    errorPrUrl: requestError !== null ? openClonePanelUrl : cloneFailure?.pr.url ?? null,
    onClone: (pr, cloneRoot, registerFirst) => {
      void requestClone(pr, cloneRoot, registerFirst);
    },
  };

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
        clone={clone}
      />
      <PrSection
        testId="pr-tab-author"
        heading="Yours"
        emptyMessage="No open PRs you authored."
        data={authorQuery.data}
        onPick={onPickPr}
        selectedPrUrl={selectedPrUrl}
        clone={clone}
      />
    </div>
  );
}

/**
 * Everything the rows need to render — and drive — the inline clone panel,
 * bundled so the two sections and every row do not each grow six props.
 */
interface CloneControls {
  /** Registered clone roots, most recently registered first. */
  roots: string[];
  rootsLoading: boolean;
  /** The row whose clone panel is expanded, by PR url. */
  openPanelUrl: string | null;
  /** The row whose clone is in flight, by PR url. */
  busyPrUrl: string | null;
  /** The current inline failure message, if any. */
  error: string | null;
  /** The row that failure belongs to, by PR url. */
  errorPrUrl: string | null;
  /**
   * Start a clone. `registerFirst` means the user typed a path that is not a
   * clone root yet, so it is registered before the clone is asked for.
   */
  onClone: (pr: PullRequest, cloneRoot: string, registerFirst: boolean) => void;
}

/**
 * Drop trailing slashes so a root the user typed as `/home/dev/projects/`
 * predicts the same destination the server derives from `/home/dev/projects`
 * (which is how it canonicalises a registration).
 *
 * A bare `/` trims to the empty string on purpose: the destination is this plus
 * `/<repo_name>`, and the server joins a `/` root the same way, so `/` must
 * predict `/<name>` rather than `//<name>` — a prediction that missed would
 * leave the completion event unmatched and the row spinning forever.
 */
function trimTrailingSlash(path: string): string {
  return path.replace(/\/+$/, '');
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
  clone: CloneControls;
}

function PrSection({
  testId,
  heading,
  emptyMessage,
  data,
  onPick,
  selectedPrUrl,
  clone,
}: PrSectionProps) {
  const prs = data?.pull_requests ?? [];
  // Adjacency-group the section's rows by repo so a multi-PR repo
  // visually clusters. Sort by repo first, then by recency within
  // each repo, so the "newer first" overall reading is preserved
  // inside each cluster.
  const grouped = useMemo(() => groupPrsByRepo(prs), [prs]);

  return (
    <section className="space-y-1" data-testid={testId}>
      <h3 className="text-caption font-semibold uppercase tracking-wide text-fg-subtle">
        {heading}
      </h3>
      {grouped.length === 0 ? (
        <p className="text-caption text-fg-subtle">{emptyMessage}</p>
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
                  className="my-1 border-t border-border-default"
                />
              )}
              <li>
                <PrRow
                  pr={row.pr}
                  onPick={onPick}
                  isSelected={row.pr.url === selectedPrUrl}
                  clone={clone}
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
  clone: CloneControls;
}

function PrRow({ pr, onPick, isSelected, clone }: PrRowProps) {
  const needsClone = !pr.has_local_clone;
  const repoLabel = `${pr.repo_owner}/${pr.repo_name}#${pr.number}`;
  const panelOpen = clone.openPanelUrl === pr.url;
  const busy = clone.busyPrUrl === pr.url;
  // A no-clone row can never be the "selected" row: the indigo pill means "this
  // is what the composer will launch", and a repository with no clone on disk
  // cannot be that until its clone lands (at which point the row is no longer a
  // no-clone row).
  const showSelected = isSelected && !needsClone;
  return (
    <>
      <button
        type="button"
        onClick={() => onPick(pr)}
        aria-pressed={showSelected}
        aria-expanded={needsClone ? panelOpen : undefined}
        data-testid="pr-tab-row"
        data-has-local-clone={pr.has_local_clone ? 'true' : 'false'}
        data-selected={showSelected ? 'true' : 'false'}
        title={
          needsClone
            ? `No local clone — clone ${pr.repo_owner}/${pr.repo_name} to open this PR.`
            : pr.url
        }
        className={cn(
          'flex w-full min-w-0 flex-col items-start gap-0.5 rounded px-2 py-1.5 text-left text-caption',
          showSelected
            ? 'bg-accent/10 text-accent ring-1 ring-accent-disabled'
            : 'text-fg hover:bg-surface-elevated-hover',
          // Still de-emphasised: this row cannot start a session yet. It is no
          // longer un-clickable, though — the click is now the way out.
          needsClone && !panelOpen && 'opacity-60',
        )}
      >
        <div className="flex w-full min-w-0 items-center gap-2">
          <span className="shrink-0 font-mono text-code font-medium text-fg">
            {repoLabel}
          </span>
          <span className="min-w-0 flex-1 truncate">{pr.title}</span>
          {busy && <Spinner label="Cloning…" />}
          {pr.draft && (
            <span className="shrink-0 rounded bg-warning/10 px-1.5 py-0.5 text-caption uppercase tracking-wide text-warning">
              draft
            </span>
          )}
        </div>
        <div className="flex w-full min-w-0 items-center gap-2 text-caption text-fg-subtle">
          <span className="shrink-0 font-mono text-code" title={pr.head_ref}>
            {displayBranch(pr.head_ref)}
          </span>
          <span aria-hidden>·</span>
          <span className="shrink-0">{formatRelative(pr.updated_at)}</span>
          <span aria-hidden>·</span>
          <span className="shrink-0">{pr.author_login}</span>
        </div>
        {needsClone && (
          <p
            className="mt-0.5 text-caption text-fg-subtle"
            data-testid="pr-tab-row-no-clone-hint"
          >
            No local clone — click to clone{' '}
            <code className="rounded bg-surface-sunken px-1 font-mono text-code">
              {pr.repo_owner}/{pr.repo_name}
            </code>
            .
          </p>
        )}
      </button>
      {needsClone && panelOpen && (
        // A sibling of the row's button, never a child: the panel holds its own
        // controls, and nesting interactive elements inside a button breaks both
        // the markup and the keyboard.
        <ClonePanel pr={pr} clone={clone} busy={busy} />
      )}
    </>
  );
}

/**
 * The inline clone panel: pick where the clone goes, then start it.
 *
 * Which clone root to use is the only open question — `gh` is authenticated and
 * the destination name is fixed — so the panel is shaped by how many roots are
 * registered:
 *
 * - **none** — a path input. Registration is a step of this flow, not a detour
 *   into Settings: sending the user elsewhere mid-decision is how a two-click
 *   task becomes a five-click one.
 * - **one** — no choice to make, so the destination is simply shown.
 * - **several** — a selector, defaulting to the most recently registered (the
 *   list arrives newest-first), which is the one the user most likely means.
 */
function ClonePanel({
  pr,
  clone,
  busy,
}: {
  pr: PullRequest;
  clone: CloneControls;
  busy: boolean;
}) {
  const needsRegistration = clone.roots.length === 0;
  // `null` means "nothing chosen yet", which resolves to the default below.
  // Deriving the effective root rather than seeding state from a prop keeps the
  // panel correct when the root list arrives after it mounted (the first render
  // shows a spinner) and when a registration made here adds one: a stale seeded
  // value would leave the selector showing a root that is not the one selected.
  const [pickedRoot, setPickedRoot] = useState<string | null>(null);
  const [typedRoot, setTypedRoot] = useState('');
  const selectedRoot =
    pickedRoot !== null && clone.roots.includes(pickedRoot)
      ? pickedRoot
      : clone.roots[0] ?? '';
  const chosenRoot = needsRegistration ? typedRoot.trim() : selectedRoot;
  const destination =
    chosenRoot === ''
      ? null
      : `${trimTrailingSlash(chosenRoot)}/${pr.repo_name}`;
  const error = clone.errorPrUrl === pr.url ? clone.error : null;

  if (clone.rootsLoading) {
    return (
      <div className="px-2 py-1.5" data-testid="pr-tab-clone-panel">
        <Spinner label="Loading clone roots…" />
      </div>
    );
  }

  return (
    <div
      className="mt-1 space-y-2 rounded border border-border-default bg-surface-sunken px-2 py-2"
      data-testid="pr-tab-clone-panel"
    >
      {needsRegistration ? (
        <label className="block space-y-1">
          <span className="block text-caption text-fg-subtle">
            No clone root registered yet. Where do your clones live?
          </span>
          <input
            type="text"
            value={typedRoot}
            onChange={(event) => setTypedRoot(event.target.value)}
            placeholder="/home/dev/projects"
            spellCheck={false}
            data-testid="pr-tab-clone-root-input"
            className="w-full rounded border border-border-default bg-surface px-2 py-1 font-mono text-code text-fg"
          />
        </label>
      ) : clone.roots.length === 1 ? (
        <p className="text-caption text-fg-subtle" data-testid="pr-tab-clone-root-single">
          Clone into{' '}
          <code className="rounded bg-surface px-1 font-mono text-code text-fg">
            {clone.roots[0]}
          </code>
        </p>
      ) : (
        <label className="block space-y-1">
          <span className="block text-caption text-fg-subtle">Clone root</span>
          <select
            value={selectedRoot}
            onChange={(event) => setPickedRoot(event.target.value)}
            data-testid="pr-tab-clone-root-select"
            className="w-full rounded border border-border-default bg-surface px-2 py-1 font-mono text-code text-fg"
          >
            {clone.roots.map((root) => (
              <option key={root} value={root}>
                {root}
              </option>
            ))}
          </select>
        </label>
      )}

      <div className="flex items-center gap-2">
        <Button
          variant="primary"
          size="sm"
          onClick={() => clone.onClone(pr, chosenRoot, needsRegistration)}
          disabled={busy || destination === null}
          data-testid="pr-tab-clone-start"
        >
          {busy ? 'Cloning…' : 'Clone'}
        </Button>
        {destination !== null && (
          <span
            className="min-w-0 truncate font-mono text-code text-fg-subtle"
            data-testid="pr-tab-clone-destination"
            title={destination}
          >
            → {destination}
          </span>
        )}
      </div>

      {error !== null && (
        <p
          className="text-caption text-danger"
          data-testid="pr-tab-clone-error"
          role="alert"
        >
          {error}
        </p>
      )}
    </div>
  );
}

function GhUnavailableHint() {
  return (
    <p
      className="text-caption text-fg-subtle"
      data-testid="pr-tab-gh-unavailable"
    >
      Run{' '}
      <code className="rounded bg-surface-sunken px-1 py-0.5 font-mono text-code text-fg">
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
