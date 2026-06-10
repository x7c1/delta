import { useState } from 'react';
import {
  ApiError,
  useRecentWorkdirsQuery,
  useWorkdirListQuery,
} from '@delta/api-client';
import { Button, Spinner } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useComposerStore } from '../../store/composerStore';

/**
 * Inline working-directory picker for the new-session center area. Always
 * visible (no modal/popover/focus-trap): a "Recent" list of previously-used
 * cwds and a "Browse" view that walks the filesystem one directory at a time.
 *
 * Selecting a directory writes it to `composerStore.newSessionWorkdir`; the
 * composer reads that and attaches it as `workdir` on the new-session send. The
 * default is no selection, in which case the send omits `workdir` and the server
 * falls back to its per-spawn default. The chosen cwd is surfaced separately as
 * a chip directly above the composer (see {@link WorkdirChip}).
 */
export function WorkdirPicker() {
  const client = useApiClient();
  const selected = useComposerStore((state) => state.newSessionWorkdir);
  const setSelected = useComposerStore((state) => state.setNewSessionWorkdir);

  // `null` browses the server default ($HOME). Once the user navigates we track
  // the concrete path so the ".." entry and descends address real directories.
  const [browsePath, setBrowsePath] = useState<string | null>(null);

  const recentQuery = useRecentWorkdirsQuery(client, true);
  const listQuery = useWorkdirListQuery(client, browsePath, true);

  // A 400/403 from an invalid or forbidden directory: show it inline with a way
  // back to the parent (or $HOME), never crash the picker.
  const listError =
    listQuery.error instanceof ApiError ? listQuery.error : null;
  const listing = listQuery.data ?? null;

  // The "Recent" section is best-effort: if it fails, hide it entirely rather
  // than surfacing an error for a non-essential convenience list.
  const recent =
    recentQuery.isSuccess && recentQuery.data.workdirs.length > 0
      ? recentQuery.data.workdirs
      : null;

  return (
    <div
      className="space-y-4 px-3 py-4"
      data-testid="workdir-picker"
    >
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
                  onClick={() => setSelected(item.path)}
                  className="w-full truncate rounded px-2 py-1 text-left font-mono text-xs text-slate-700 hover:bg-slate-100"
                  title={item.path}
                >
                  {item.path}
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
            <p
              className="truncate font-mono text-xs text-slate-500"
              title={listing.path}
              data-testid="workdir-current-path"
            >
              {listing.path}
            </p>
            <ul className="space-y-0.5">
              {listing.parent !== null && (
                <li>
                  <button
                    type="button"
                    onClick={() => setBrowsePath(listing.parent)}
                    className="w-full rounded px-2 py-1 text-left font-mono text-xs text-slate-700 hover:bg-slate-100"
                    data-testid="workdir-parent"
                  >
                    ..
                  </button>
                </li>
              )}
              {listing.entries.map((entry) => (
                <li key={entry.path}>
                  <button
                    type="button"
                    onClick={() => setBrowsePath(entry.path)}
                    className="w-full truncate rounded px-2 py-1 text-left font-mono text-xs text-slate-700 hover:bg-slate-100"
                    title={entry.path}
                  >
                    {entry.name}/
                  </button>
                </li>
              ))}
              {listing.entries.length === 0 && (
                <li className="px-2 py-1 text-xs italic text-slate-400">
                  No subdirectories.
                </li>
              )}
            </ul>
            <Button
              size="sm"
              variant="primary"
              onClick={() => setSelected(listing.path)}
              disabled={selected === listing.path}
              data-testid="workdir-select-current"
            >
              Start here
            </Button>
          </>
        )}
      </section>
    </div>
  );
}

/**
 * The selected-cwd chip shown directly above the composer: "Start in: <path> ✕".
 * The ✕ clears the selection, returning to the default (the send omits
 * `workdir`). Renders nothing when no directory is selected.
 */
export function WorkdirChip() {
  const selected = useComposerStore((state) => state.newSessionWorkdir);
  const setSelected = useComposerStore((state) => state.setNewSessionWorkdir);

  if (!selected) {
    return null;
  }

  return (
    <div
      className="flex items-center gap-2 rounded border border-indigo-200 bg-indigo-50 px-2 py-1 text-xs"
      data-testid="workdir-chip"
    >
      <span className="shrink-0 font-medium text-indigo-700">Start in:</span>
      <span className="min-w-0 flex-1 truncate font-mono text-slate-700" title={selected}>
        {selected}
      </span>
      <Button
        variant="ghost"
        size="sm"
        onClick={() => setSelected(null)}
        aria-label="Clear working directory"
      >
        ✕
      </Button>
    </div>
  );
}
