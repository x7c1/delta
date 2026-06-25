import { useEffect, useState } from 'react';
import {
  ApiError,
  useHomeDirQuery,
  useRecentWorkdirsQuery,
  useWorkdirListQuery,
} from '@delta/api-client';
import { Button, Spinner, cn } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { displayPath } from '../../utils/displayPath';

/**
 * The folder glyph used to mark directory rows. Decorative — always
 * `aria-hidden`, so a row's accessible name stays its path text.
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
 * The corner / level-up arrow glyph marking the ".." row.
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

export interface WorkdirPickerBodyProps {
  /** Whether the picker should fetch its lists (passed straight to the
   *  `enabled` gate on each query). */
  active: boolean;
  /** The current candidate selection, lifted to the caller so it can drive a
   *  confirm button alongside this body. */
  candidate: string | null;
  /** Update the candidate. The picker calls this from a Recent-row click and
   *  from the "Use this directory" button. */
  setCandidate: (path: string | null) => void;
  /** Confirm the candidate (Enter inside the picker dispatches this when a
   *  candidate exists). */
  onConfirm: () => void;
  /**
   * Whether to include the leading help text. The modal dialog already
   * shows the picker's intent in its title, so it omits the line; the
   * inline tab leans on it as the panel's only context.
   */
  showHelpText?: boolean;
}

/**
 * The Recent + Browse picker UI body, lifted out of `WorkdirDialog` so the
 * Directory tab can render the same lists inline (no modal). Owns the browse
 * cursor (the path currently being browsed and the error state); the
 * candidate selection is lifted to the caller because the confirm button
 * lives alongside this body in both the dialog and the tab.
 */
export function WorkdirPickerBody({
  active,
  candidate,
  setCandidate,
  onConfirm,
  showHelpText = true,
}: WorkdirPickerBodyProps) {
  const client = useApiClient();

  // `null` browses the server default ($HOME). Once the user navigates we
  // track the concrete path so the ".." entry and descends address real
  // directories.
  const [browsePath, setBrowsePath] = useState<string | null>(null);

  const recentQuery = useRecentWorkdirsQuery(client, active);
  const listQuery = useWorkdirListQuery(client, browsePath, active);

  // The home directory, used only to abbreviate displayed paths to `~`.
  const home = useHomeDirQuery(client, active).data?.path ?? null;

  // A 400/403 from an invalid or forbidden directory: show it inline with a
  // way back to the parent (or $HOME).
  const listError =
    listQuery.error instanceof ApiError ? listQuery.error : null;
  const listing = listQuery.data ?? null;

  // The "Recent" section is best-effort: if it fails, hide it entirely.
  const recent =
    recentQuery.isSuccess && recentQuery.data.workdirs.length > 0
      ? recentQuery.data.workdirs
      : null;

  // Reset the browse position whenever the picker goes inactive, so a
  // re-mount starts fresh rather than carrying an in-flight pick.
  useEffect(() => {
    if (!active) {
      setBrowsePath(null);
    }
  }, [active]);

  // On activation, pre-select the most-recent directory so the user can
  // confirm immediately. The recent list arrives async after activation, so
  // this keys on it too. The `candidate === null` guard seeds only once and
  // never clobbers a user pick.
  useEffect(() => {
    if (active && recent && candidate === null) {
      setCandidate(recent[0].path);
    }
  }, [active, recent, candidate, setCandidate]);

  // Navigating to a directory also makes it the candidate, so the highlighted
  // selection follows where you are in Browse.
  const navigateTo = (path: string | null) => {
    setBrowsePath(path);
    setCandidate(path);
  };

  return (
    <div
      className="space-y-4"
      data-testid="workdir-picker"
      onKeyDown={(event) => {
        // Enter confirms when a candidate exists; Esc is handled by the
        // caller (the Dialog or, in the tab, by the surrounding UI).
        if (event.key === 'Enter') {
          event.preventDefault();
          onConfirm();
        }
      }}
    >
      {showHelpText && (
        <p className="text-xs text-slate-500" data-testid="workdir-help">
          Claude Code starts in this folder. Pick the project to work in.
        </p>
      )}

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
                // The home directory abbreviates to a bare `~`, which is
                // easy to overlook here; spell it out so the choice is
                // unambiguous. Only at this call site — `displayPath`
                // stays pure and the `title` keeps the full absolute path.
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
  );
}
