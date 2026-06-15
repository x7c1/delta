import { useState } from 'react';
import { useGitBranchesQuery, useGitRepoInfoQuery } from '@delta/api-client';
import type { WorktreeStartPoint } from '@delta/wire-gen';
import { Spinner, cn } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import {
  DEFAULT_WORKTREE_START_POINT,
  useComposerStore,
} from '../../store/composerStore';

/**
 * The opt-in worktree controls shown in the new-session composer, directly below
 * the selected-directory chip. They appear only when the selected directory is a
 * git repository (`GET /api/workdir/git` reports a `repo_root`); for a non-git
 * directory — or while the check is in flight or errors — nothing renders, so a
 * plain non-git session is unaffected.
 *
 * When present, a toggle opts the new session into starting in an isolated git
 * worktree. With the toggle on, a start-point selector chooses where the
 * worktree starts from:
 *
 * - **Current HEAD** — `{ kind: "head" }`, the safe default (no fetch); always
 *   a fresh branch (HEAD's branch is checked out in the main tree).
 * - **Latest `<default_branch>`** — a branch start-point, shown only when the
 *   repo's default branch is known.
 * - **Other remote branch…** — expands a list fetched lazily from
 *   `GET /api/workdir/git/branches` (which performs a `git fetch`), plus a
 *   free-text entry for a brand-new branch not yet in the fetched list.
 *
 * For any branch start-point (the default-branch preset or an explicit branch),
 * a use-vs-new choice picks the worktree's branch mode:
 *
 * - **New branch from it** — `{ kind: "remote_branch", name }`, the default
 *   (safest isolation): cut a fresh per-session branch from it.
 * - **Use this branch** — `{ kind: "use_remote_branch", name }`: work on the
 *   branch directly (the backend reuses the worktree that already has it
 *   checked out, including the main tree, or creates one that checks it out).
 *
 * The chosen toggle/start-point live in `composerStore`; the composer reads them
 * and attaches `worktree` to the new-session send. The start-point value itself
 * encodes the use-vs-new mode via its `kind`, so no separate mode field needed.
 */
export function WorktreeOptions() {
  const client = useApiClient();
  const workdir = useComposerStore((state) => state.newSessionWorkdir);
  const enabled = useComposerStore((state) => state.newSessionWorktreeEnabled);
  const setEnabled = useComposerStore(
    (state) => state.setNewSessionWorktreeEnabled,
  );
  const startPoint = useComposerStore(
    (state) => state.newSessionWorktreeStartPoint,
  );
  const setStartPoint = useComposerStore(
    (state) => state.setNewSessionWorktreeStartPoint,
  );

  // Cheap, no-network probe: runs as soon as a directory is selected.
  const repoQuery = useGitRepoInfoQuery(client, workdir, workdir !== null);
  const repoRoot = repoQuery.data?.repo_root ?? null;
  const defaultBranch = repoQuery.data?.default_branch ?? null;

  if (workdir === null || repoRoot === null) {
    // Not a git repository (or the check is still loading / errored): no
    // worktree option to offer, so the plain non-git flow is untouched.
    return null;
  }

  return (
    <section
      className="space-y-1.5 rounded border border-slate-200 bg-slate-50 px-2 py-1.5 text-xs"
      data-testid="worktree-options"
    >
      <label className="flex cursor-pointer items-center gap-2">
        <input
          type="checkbox"
          checked={enabled}
          onChange={(event) => setEnabled(event.target.checked)}
          data-testid="worktree-toggle"
        />
        <span className="font-medium text-slate-700">
          Start in an isolated git worktree
        </span>
      </label>

      {enabled && (
        <WorktreeStartPointSelector
          workdir={workdir}
          defaultBranch={defaultBranch}
          startPoint={startPoint}
          onChange={setStartPoint}
        />
      )}
    </section>
  );
}

/**
 * Discriminates which of the three start-point choices the current
 * {@link WorktreeStartPoint} represents, so the right radio reads as selected.
 * `head` and the default-branch preset are exact matches; any other branch
 * (including a free-text entry) falls under "other". The use-vs-new *mode*
 * (`remote_branch` vs `use_remote_branch`) is orthogonal to this and read
 * separately, so both branch kinds classify the same way.
 */
type StartPointChoice = 'head' | 'default-branch' | 'other';

function classifyStartPoint(
  startPoint: WorktreeStartPoint,
  defaultBranch: string | null,
): StartPointChoice {
  if (startPoint.kind === 'head') {
    return 'head';
  }
  if (defaultBranch !== null && startPoint.name === defaultBranch) {
    return 'default-branch';
  }
  return 'other';
}

/**
 * Whether a start-point names a branch (so the use-vs-new choice applies), and
 * which mode it currently is. `head` is new-branch-only — HEAD's branch is
 * checked out in the main tree, so it cannot be "used" in another worktree.
 */
type BranchMode = 'remote_branch' | 'use_remote_branch';

/** The branch name a non-`head` start-point carries, or `null` for `head`. */
function branchName(startPoint: WorktreeStartPoint): string | null {
  return startPoint.kind === 'head' ? null : startPoint.name;
}

interface WorktreeStartPointSelectorProps {
  workdir: string;
  defaultBranch: string | null;
  startPoint: WorktreeStartPoint;
  onChange: (startPoint: WorktreeStartPoint) => void;
}

function WorktreeStartPointSelector({
  workdir,
  defaultBranch,
  startPoint,
  onChange,
}: WorktreeStartPointSelectorProps) {
  const choice = classifyStartPoint(startPoint, defaultBranch);
  // Whether the "other remote branch" list has been opened. Gates the lazy
  // (fetching) branches query — it stays closed until the user picks "other".
  const [otherOpen, setOtherOpen] = useState(choice === 'other');

  // The current use-vs-new mode for a branch start-point, carried so switching
  // which branch is selected preserves the user's choice. `head` is always a
  // new branch, so when it is selected the mode reads as `remote_branch`.
  const mode: BranchMode =
    startPoint.kind === 'use_remote_branch'
      ? 'use_remote_branch'
      : 'remote_branch';

  // Re-emit the currently-selected branch under `nextMode`. Only meaningful for
  // a branch start-point (not `head`), so a `null` name is a no-op.
  const selectMode = (nextMode: BranchMode) => {
    const name = branchName(startPoint);
    if (name !== null) {
      onChange({ kind: nextMode, name });
    }
  };

  const selectChoice = (next: StartPointChoice) => {
    if (next === 'head') {
      // HEAD is new-branch-only: its branch is checked out in the main tree, so
      // it cannot be "used" in another worktree. Reset to the safe default.
      setOtherOpen(false);
      onChange(DEFAULT_WORKTREE_START_POINT);
    } else if (next === 'default-branch' && defaultBranch !== null) {
      setOtherOpen(false);
      onChange({ kind: mode, name: defaultBranch });
    } else {
      // "Other": open the lazy branch picker; keep whatever branch is already
      // chosen (else leave the name blank until one is picked/typed). Preserve
      // the current use-vs-new mode across the switch.
      setOtherOpen(true);
      if (startPoint.kind === 'head') {
        onChange({ kind: mode, name: '' });
      }
    }
  };

  return (
    <fieldset className="space-y-1" data-testid="worktree-start-point">
      <legend className="font-semibold uppercase tracking-wide text-slate-500">
        Branch from
      </legend>

      <label className="flex cursor-pointer items-start gap-2 px-1 py-0.5">
        <input
          type="radio"
          name="worktree-start-point"
          checked={choice === 'head'}
          onChange={() => selectChoice('head')}
          data-testid="start-point-head"
          className="mt-0.5"
        />
        <span className="flex flex-col">
          <span className="font-medium text-slate-700">Current HEAD</span>
          <span className="text-slate-500">
            Branch from the repository&rsquo;s current state.
          </span>
        </span>
      </label>

      {defaultBranch !== null && (
        <label className="flex cursor-pointer items-start gap-2 px-1 py-0.5">
          <input
            type="radio"
            name="worktree-start-point"
            checked={choice === 'default-branch'}
            onChange={() => selectChoice('default-branch')}
            data-testid="start-point-default-branch"
            className="mt-0.5"
          />
          <span className="flex flex-col">
            <span className="font-medium text-slate-700">
              Latest{' '}
              <span className="font-mono text-slate-600">{defaultBranch}</span>
            </span>
            <span className="text-slate-500">
              Fetch and branch from the default branch.
            </span>
          </span>
        </label>
      )}

      <label className="flex cursor-pointer items-start gap-2 px-1 py-0.5">
        <input
          type="radio"
          name="worktree-start-point"
          checked={choice === 'other'}
          onChange={() => selectChoice('other')}
          data-testid="start-point-other"
          className="mt-0.5"
        />
        <span className="flex flex-col">
          <span className="font-medium text-slate-700">
            Other remote branch&hellip;
          </span>
          <span className="text-slate-500">
            Fetch and branch from a specific remote branch.
          </span>
        </span>
      </label>

      {otherOpen && (
        <RemoteBranchPicker
          workdir={workdir}
          selectedName={branchName(startPoint) ?? ''}
          onSelect={(name) => onChange({ kind: mode, name })}
        />
      )}

      {/* The use-vs-new choice applies to any branch start-point (the
          default-branch preset or an explicit branch), but never to HEAD. */}
      {choice !== 'head' && (
        <BranchModeChoice mode={mode} onChange={selectMode} />
      )}
    </fieldset>
  );
}

interface BranchModeChoiceProps {
  mode: BranchMode;
  onChange: (mode: BranchMode) => void;
}

/**
 * The "use this branch" vs "new branch from it" choice shown for a branch
 * start-point. Defaults to "new branch from it" (the safest isolation,
 * preserving the original behavior). "Use this branch" works on the branch
 * directly in the worktree — the backend reuses the worktree that already has
 * it checked out (including the main working tree), or creates one that checks
 * it out.
 */
function BranchModeChoice({ mode, onChange }: BranchModeChoiceProps) {
  return (
    <div className="ml-6 space-y-1" data-testid="branch-mode">
      <label className="flex cursor-pointer items-start gap-2 px-1 py-0.5">
        <input
          type="radio"
          name="worktree-branch-mode"
          checked={mode === 'remote_branch'}
          onChange={() => onChange('remote_branch')}
          data-testid="branch-mode-new"
          className="mt-0.5"
        />
        <span className="flex flex-col">
          <span className="font-medium text-slate-700">New branch from it</span>
          <span className="text-slate-500">
            Cut a fresh per-session branch starting from it.
          </span>
        </span>
      </label>

      <label className="flex cursor-pointer items-start gap-2 px-1 py-0.5">
        <input
          type="radio"
          name="worktree-branch-mode"
          checked={mode === 'use_remote_branch'}
          onChange={() => onChange('use_remote_branch')}
          data-testid="branch-mode-use"
          className="mt-0.5"
        />
        <span className="flex flex-col">
          <span className="font-medium text-slate-700">Use this branch</span>
          <span className="text-slate-500">
            Work on the branch directly in the worktree.
          </span>
        </span>
      </label>
    </div>
  );
}

interface RemoteBranchPickerProps {
  workdir: string;
  /** The currently chosen remote branch name (empty when none yet). */
  selectedName: string;
  onSelect: (name: string) => void;
}

/**
 * The lazily-fetched remote-branch list for the "Other remote branch" choice.
 * Mounting it enables `useGitBranchesQuery` (which performs a `git fetch`), so
 * it only runs once the user opens this picker. A brand-new remote branch may
 * not be in the fetched list yet, so a free-text field lets the user name any
 * ref; the backend fetches the chosen ref at spawn.
 */
function RemoteBranchPicker({
  workdir,
  selectedName,
  onSelect,
}: RemoteBranchPickerProps) {
  const client = useApiClient();
  const query = useGitBranchesQuery(client, workdir, true);
  const branches = query.data?.remote_branches ?? [];

  return (
    <div className="ml-6 space-y-1.5" data-testid="remote-branch-picker">
      <label className="flex flex-col gap-0.5">
        <span className="text-slate-500">Branch name</span>
        <input
          type="text"
          value={selectedName}
          onChange={(event) => onSelect(event.target.value)}
          placeholder="origin branch (e.g. feature/x)"
          className="rounded border border-slate-300 px-2 py-1 font-mono text-xs focus:border-indigo-400 focus:outline-none"
          data-testid="remote-branch-input"
        />
      </label>

      {query.isLoading && <Spinner label="Fetching remote branches…" />}

      {!query.isLoading && query.isError && (
        <p
          className="rounded border border-rose-200 bg-rose-50 px-2 py-1 text-rose-700"
          role="alert"
          data-testid="remote-branch-error"
        >
          Could not fetch remote branches.
        </p>
      )}

      {!query.isLoading && !query.isError && branches.length > 0 && (
        <ul className="max-h-32 space-y-0.5 overflow-y-auto">
          {branches.map((name) => (
            <li key={name}>
              <button
                type="button"
                onClick={() => onSelect(name)}
                aria-pressed={selectedName === name}
                className={cn(
                  'w-full truncate rounded px-2 py-1 text-left font-mono text-xs hover:bg-slate-100',
                  selectedName === name
                    ? 'bg-indigo-50 text-indigo-700 ring-1 ring-indigo-200'
                    : 'text-slate-700',
                )}
                title={name}
                data-testid={`remote-branch-${name}`}
              >
                {name}
              </button>
            </li>
          ))}
        </ul>
      )}

      {!query.isLoading && !query.isError && branches.length === 0 && (
        <p className="px-2 py-1 italic text-slate-400">
          No remote branches found. Type a branch name above.
        </p>
      )}
    </div>
  );
}
