import { useEffect, useState } from 'react';
import {
  ApiError,
  useHomeDirQuery,
  useRecentWorkdirsQuery,
  useWorkdirListQuery,
} from '@delta/api-client';
import { Button, Dialog, Spinner, cn } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useComposerStore } from '../../store/composerStore';
import { displayPath } from '../../utils/displayPath';

/**
 * The folder glyph used to mark directory rows (and the chip's change button) so
 * the picker reads as a directory chooser. Decorative — always `aria-hidden`, so
 * a row's accessible name stays its path text. This file is the only user.
 */
function FolderIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 20 20"
      fill="currentColor"
      className={className}
      aria-hidden="true"
    >
      <path d="M3.75 3A1.75 1.75 0 0 0 2 4.75v3.26a3.235 3.235 0 0 1 1.75-.51h12.5c.644 0 1.245.188 1.75.51V6.75A1.75 1.75 0 0 0 16.25 5h-4.836a.25.25 0 0 1-.177-.073L9.823 3.513A1.75 1.75 0 0 0 8.586 3H3.75ZM3.75 9A1.75 1.75 0 0 0 2 10.75v4.5c0 .966.784 1.75 1.75 1.75h12.5A1.75 1.75 0 0 0 18 15.25v-4.5A1.75 1.75 0 0 0 16.25 9H3.75Z" />
    </svg>
  );
}

/**
 * The corner / level-up arrow glyph marking the ".." row so it reads as "go up
 * one level to the parent directory" — the shaft rises then turns to point left,
 * visually distinct from the folder rows. Decorative — always
 * `aria-hidden`, so the row's accessible name stays "..". This file is the only
 * user.
 */
function ParentDirIcon({ className }: { className?: string }) {
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
      <polyline points="9 14 4 9 9 4" />
      <path d="M20 20v-7a4 4 0 0 0-4-4H4" />
    </svg>
  );
}

export interface WorkdirDialogProps {
  /** Whether the modal is shown. */
  open: boolean;
  /** Close without committing (Cancel / Esc / backdrop). */
  onClose: () => void;
  /**
   * Whether the picker can be dismissed without choosing a directory. When
   * `false` (the first-run / zero-session case) there is nowhere to fall back
   * to, so the only way out is to Select a directory: the Cancel button is
   * hidden and Esc/backdrop no longer close. Defaults to `true`.
   */
  dismissable?: boolean;
}

/**
 * Modal working-directory picker for a new session. Choosing a directory is
 * mandatory (the composer keeps Send disabled until one is committed), so this
 * is presented as a focused dialog rather than an always-visible panel.
 *
 * Inside, a "Recent" list of previously-used cwds and a "Browse" view that walks
 * the filesystem one directory at a time. A clicked Recent row, or the
 * "Use this directory" affordance in Browse, marks a *candidate* — the
 * highlighted answer-in-waiting. On open the most-recent directory (the first
 * Recent row) is pre-selected as the candidate so the user can confirm at once;
 * with no Recent list nothing is pre-selected and Select stays disabled until a
 * browse pick.
 *
 * Select commits the candidate to `composerStore.newSessionWorkdir` and closes;
 * the composer reads that and attaches it as `workdir` on the new-session send.
 * Cancel closes without committing. Enter confirms (when a candidate exists) and
 * Esc cancels (handled by the Dialog).
 */
export function WorkdirDialog({
  open,
  onClose,
  dismissable = true,
}: WorkdirDialogProps) {
  const client = useApiClient();
  const setSelected = useComposerStore((state) => state.setNewSessionWorkdir);

  // `null` browses the server default ($HOME). Once the user navigates we track
  // the concrete path so the ".." entry and descends address real directories.
  const [browsePath, setBrowsePath] = useState<string | null>(null);
  // The highlighted answer-in-waiting. Committed to the store only on Select.
  const [candidate, setCandidate] = useState<string | null>(null);

  const recentQuery = useRecentWorkdirsQuery(client, open);
  const listQuery = useWorkdirListQuery(client, browsePath, open);

  // The home directory, used only to abbreviate displayed paths to `~`. The
  // values committed to the store and sent to the backend stay absolute.
  const home = useHomeDirQuery(client, open).data?.path ?? null;

  // A 400/403 from an invalid or forbidden directory: show it inline with a way
  // back to the parent (or $HOME), never crash the dialog.
  const listError =
    listQuery.error instanceof ApiError ? listQuery.error : null;
  const listing = listQuery.data ?? null;

  // The "Recent" section is best-effort: if it fails, hide it entirely rather
  // than surfacing an error for a non-essential convenience list.
  const recent =
    recentQuery.isSuccess && recentQuery.data.workdirs.length > 0
      ? recentQuery.data.workdirs
      : null;

  // Reset the candidate and browse position whenever the dialog closes, so a
  // reopen starts fresh from the recent pre-selection rather than reusing a
  // previous in-flight pick.
  useEffect(() => {
    if (!open) {
      setCandidate(null);
      setBrowsePath(null);
    }
  }, [open]);

  // On open, pre-select the most-recent directory (the first Recent row) so the
  // user can confirm immediately. The recent list arrives async after open, so
  // this keys on it too. The `candidate === null` guard seeds only once and
  // never clobbers a user pick.
  useEffect(() => {
    if (open && recent && candidate === null) {
      setCandidate(recent[0].path);
    }
  }, [open, recent, candidate]);

  // Navigating to a directory also makes it the candidate, so the highlighted
  // selection follows where you are in Browse — dropping the recent
  // pre-selection once the user starts browsing.
  const navigateTo = (path: string | null) => {
    setBrowsePath(path);
    setCandidate(path);
  };

  const confirm = () => {
    if (candidate === null) {
      return;
    }
    setSelected(candidate);
    onClose();
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      dismissable={dismissable}
      title="Where should this session run?"
      footer={
        <>
          {dismissable && (
            <Button
              variant="ghost"
              onClick={onClose}
              data-testid="workdir-cancel"
            >
              Cancel
            </Button>
          )}
          <Button
            variant="primary"
            onClick={confirm}
            disabled={candidate === null}
            data-testid="workdir-confirm"
          >
            Select
          </Button>
        </>
      }
    >
      <div
        className="space-y-4"
        data-testid="workdir-picker"
        onKeyDown={(event) => {
          // Enter confirms when a candidate exists; Esc is handled by the Dialog.
          if (event.key === 'Enter') {
            event.preventDefault();
            confirm();
          }
        }}
      >
        <p className="text-xs text-slate-500" data-testid="workdir-help">
          Claude Code starts in this folder. Pick the project to work in.
        </p>

        {recent && (
          <section className="space-y-1" data-testid="workdir-recent">
            <h3 className="text-xs font-semibold uppercase tracking-wide text-slate-500">
              Recent
            </h3>
            <ul className="space-y-0.5">
              {recent.map((item) => (
                <li key={item.path}>
                  <button
                    type="button"
                    onClick={() => setCandidate(item.path)}
                    aria-pressed={candidate === item.path}
                    className={cn(
                      'flex w-full min-w-0 items-center gap-2 rounded px-2 py-1 text-left font-mono text-xs hover:bg-slate-100',
                      candidate === item.path
                        ? 'bg-indigo-50 text-indigo-700 ring-1 ring-indigo-200'
                        : 'text-slate-700',
                    )}
                    title={item.path}
                  >
                    <FolderIcon className="h-4 w-4 shrink-0" />
                    <span className="truncate">
                      {displayPath(item.path, home)}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        )}

        <section className="space-y-1" data-testid="workdir-browse">
          <h3 className="text-xs font-semibold uppercase tracking-wide text-slate-500">
            Browse
          </h3>

          {listQuery.isLoading && <Spinner label="Loading directory…" />}

          {listError && (
            <div
              className="space-y-2 rounded border border-rose-200 bg-rose-50 px-2 py-2 text-xs text-rose-700"
              data-testid="workdir-error"
              role="alert"
            >
              <p>
                {listError.status === 403
                  ? 'Permission denied for this directory.'
                  : 'This directory could not be opened.'}
              </p>
              <Button
                size="sm"
                variant="secondary"
                onClick={() => {
                  // Go back to the parent of the directory we tried to open. We
                  // only know the parent when an earlier successful listing
                  // recorded it; otherwise fall back to $HOME (null).
                  setBrowsePath(listing?.parent ?? null);
                }}
              >
                {listing?.parent ? 'Back to parent' : 'Back to home'}
              </Button>
            </div>
          )}

          {!listError && listing && (
            <>
              <button
                type="button"
                onClick={() => setCandidate(listing.path)}
                aria-pressed={candidate === listing.path}
                className={cn(
                  'w-full truncate rounded px-2 py-1 text-left font-mono text-xs hover:bg-slate-100',
                  candidate === listing.path
                    ? 'bg-indigo-50 text-indigo-700 ring-1 ring-indigo-200'
                    : 'text-slate-700',
                )}
                title={listing.path}
                data-testid="workdir-use-current"
              >
                {(() => {
                  // The home directory abbreviates to a bare `~`, which is easy
                  // to overlook here; spell it out so the choice is unambiguous.
                  // Only at this call site — `displayPath` stays pure and the
                  // `title` keeps the full absolute path.
                  const abbr = displayPath(listing.path, home);
                  return `Use this directory: ${abbr === '~' ? '~ (HOME)' : abbr}`;
                })()}
              </button>
              <ul className="space-y-0.5">
                {listing.parent !== null && (
                  <li>
                    <button
                      type="button"
                      onClick={() => navigateTo(listing.parent)}
                      className="flex w-full min-w-0 items-center gap-2 rounded px-2 py-1 text-left font-mono text-xs text-slate-700 hover:bg-slate-100"
                      data-testid="workdir-parent"
                    >
                      <ParentDirIcon className="h-4 w-4 shrink-0" />
                      <span>..</span>
                    </button>
                  </li>
                )}
                {listing.entries.map((entry) => (
                  <li key={entry.path}>
                    <button
                      type="button"
                      onClick={() => navigateTo(entry.path)}
                      className="flex w-full min-w-0 items-center gap-2 rounded px-2 py-1 text-left font-mono text-xs text-slate-700 hover:bg-slate-100"
                      title={entry.path}
                    >
                      <FolderIcon className="h-4 w-4 shrink-0" />
                      <span className="truncate">{entry.name}/</span>
                    </button>
                  </li>
                ))}
                {listing.entries.length === 0 && (
                  <li className="px-2 py-1 text-xs italic text-slate-400">
                    No subdirectories.
                  </li>
                )}
              </ul>
            </>
          )}
        </section>
      </div>
    </Dialog>
  );
}

export interface WorkdirChipProps {
  /** Reopen the dialog to change the selection (the folder-icon affordance). */
  onEdit: () => void;
}

/**
 * The selected-cwd chip shown directly above the composer: "Start in: <path>"
 * followed by a folder-icon button. Selection is mandatory, so there is no
 * "clear to default" — the folder button instead reopens the dialog to pick a
 * different directory. Renders nothing when no directory is selected.
 */
export function WorkdirChip({ onEdit }: WorkdirChipProps) {
  const client = useApiClient();
  const selected = useComposerStore((state) => state.newSessionWorkdir);
  // Read before the early return to keep the hook order stable. Display-only;
  // the stored `newSessionWorkdir` stays the absolute path.
  const home = useHomeDirQuery(client, true).data?.path ?? null;

  if (!selected) {
    return null;
  }

  return (
    <div
      className="flex items-center gap-2 rounded border border-indigo-200 bg-indigo-50 px-2 py-1 text-xs"
      data-testid="workdir-chip"
    >
      <span className="shrink-0 font-medium text-indigo-700">Start in:</span>
      <span
        className="min-w-0 flex-1 truncate font-mono text-slate-700"
        title={selected}
      >
        {displayPath(selected, home)}
      </span>
      <Button
        variant="ghost"
        size="sm"
        onClick={onEdit}
        aria-label="Change working directory"
        title="Change directory"
      >
        <FolderIcon className="h-4 w-4" />
      </Button>
    </div>
  );
}
