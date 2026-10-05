import { useEffect, useState, type ReactNode } from 'react';
import { useStorageQuery } from '@delta/api-client';
import type { StorageFile, StorageResponse } from '@delta/wire-gen';
import { Button, Spinner } from '@delta/ui-kit';
import { useApiClient } from '../../../data/apiContext';
import { exactBytes, formatBytes } from './formatBytes';

/**
 * Storage category content: where this running Delta keeps its files on this
 * machine, and how large the database is — read from `GET /api/storage`, so it
 * names the directories *this* install actually uses rather than the
 * documented defaults.
 *
 * Read-only: one row per location, each with a copy control. `active` gates
 * the fetch to while the category is shown; the query refetches on a later
 * visit once its answer is stale, so the sizes follow the files on disk.
 */
export function StorageSection({ active }: { active: boolean }) {
  const client = useApiClient();
  const storageQuery = useStorageQuery(client, active);

  return (
    <section className="space-y-3" data-testid="storage-section">
      <div>
        <h3 className="mb-1 text-secondary font-semibold text-fg">Storage</h3>
        <p className="text-caption text-fg-muted">
          Where this Delta keeps its files on this machine. Worktrees and Claude
          Code transcripts live outside the data directory.
        </p>
      </div>

      {storageQuery.isPending ? (
        <div className="flex justify-center py-4">
          <Spinner label="loading storage" />
        </div>
      ) : storageQuery.isError ? (
        <div className="flex flex-col items-center gap-2 py-4 text-secondary text-fg-muted">
          <p>Could not load storage.</p>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => storageQuery.refetch()}
          >
            Retry
          </Button>
        </div>
      ) : (
        <StorageInventory storage={storageQuery.data} />
      )}
    </section>
  );
}

function StorageInventory({ storage }: { storage: StorageResponse }) {
  return (
    <>
      <dl
        className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-caption"
        data-testid="storage-identity"
      >
        <dt className="text-fg-muted">Identifier</dt>
        <dd className="font-mono text-code text-fg">{storage.identifier}</dd>
        <dt className="text-fg-muted">Version</dt>
        <dd className="font-mono text-code text-fg">{storage.version}</dd>
      </dl>
      <ul className="flex flex-col gap-2" data-testid="storage-list">
        <StorageRow label="Data directory" value={storage.data_dir} />
        <StorageRow
          label="Database"
          value={storage.database.path}
          bytes={storage.database.bytes}
          testId="storage-database"
        >
          <Snapshots snapshots={storage.snapshots} />
        </StorageRow>
        <StorageRow label="Hook state" value={storage.hook_state} />
        <StorageRow
          label="Session working directories"
          value={storage.sessions_dir}
        />
        <StorageRow
          label="Session settings"
          value={storage.session_settings}
        />
        <StorageRow label="tmux configuration" value={storage.tmux_conf} />
        <StorageRow label="tmux socket name" value={storage.tmux_socket} />
        <StorageRow label="Worktrees" value={storage.worktree_base} />
        <StorageRow
          label="Claude Code transcripts"
          value={storage.transcript_root}
        />
      </ul>
    </>
  );
}

/**
 * The database's migration snapshots, under its row. Said in one line when
 * there are none, so the absence reads as a fact rather than a missing row.
 */
function Snapshots({ snapshots }: { snapshots: StorageFile[] }) {
  if (snapshots.length === 0) {
    return (
      <p
        className="text-caption text-fg-subtle"
        data-testid="storage-no-snapshots"
      >
        No migration snapshots.
      </p>
    );
  }
  return (
    <div>
      <p className="text-caption text-fg-muted">Migration snapshots</p>
      <ul className="mt-1 flex flex-col gap-1" data-testid="storage-snapshots">
        {snapshots.map((snapshot) => (
          <li key={snapshot.path} className="flex items-center gap-2">
            <PathText value={snapshot.path} />
            <Size bytes={snapshot.bytes} />
            <CopyButton value={snapshot.path} label="snapshot path" />
          </li>
        ))}
      </ul>
    </div>
  );
}

interface StorageRowProps {
  label: string;
  value: string;
  bytes?: number;
  testId?: string;
  children?: ReactNode;
}

function StorageRow({ label, value, bytes, testId, children }: StorageRowProps) {
  return (
    <li
      className="flex flex-col gap-1 rounded-lg border border-border-default px-3 py-2"
      data-testid={testId}
    >
      <div className="flex items-center gap-2">
        <span className="text-caption font-medium text-fg-muted">{label}</span>
        {bytes !== undefined && <Size bytes={bytes} />}
      </div>
      <div className="flex items-center gap-2">
        <PathText value={value} />
        <CopyButton value={value} label={label} />
      </div>
      {children}
    </li>
  );
}

/**
 * A path in monospace, truncated from the left on overflow so the file name —
 * the part that tells two paths apart — stays visible. The `rtl` container
 * puts the ellipsis at the start; the `<bdi>` keeps the path itself reading
 * left to right, leading `/` included.
 */
function PathText({ value }: { value: string }) {
  return (
    <span
      className="min-w-0 flex-1 truncate text-left font-mono text-code text-fg [direction:rtl]"
      title={value}
    >
      <bdi>{value}</bdi>
    </span>
  );
}

function Size({ bytes }: { bytes: number }) {
  return (
    <span
      className="shrink-0 text-caption tabular-nums text-fg-subtle"
      title={exactBytes(bytes)}
    >
      {formatBytes(bytes)}
    </span>
  );
}

/** How long the copy control says "Copied" (or "Copy failed") after a click. */
const COPY_FEEDBACK_MS = 1500;

type CopyState = 'idle' | 'copied' | 'failed';

function CopyButton({ value, label }: { value: string; label: string }) {
  const [state, setState] = useState<CopyState>('idle');

  useEffect(() => {
    if (state === 'idle') {
      return;
    }
    const timer = window.setTimeout(() => setState('idle'), COPY_FEEDBACK_MS);
    return () => window.clearTimeout(timer);
  }, [state]);

  // Started inside a promise so a webview without `navigator.clipboard`, where
  // the call throws synchronously, also ends in "Copy failed" rather than no change.
  const copy = () => {
    Promise.resolve()
      .then(() => navigator.clipboard.writeText(value))
      .then(
        () => setState('copied'),
        () => setState('failed'),
      );
  };

  return (
    <Button
      size="sm"
      variant="ghost"
      className="shrink-0"
      onClick={copy}
      aria-label={`Copy ${label}`}
    >
      {state === 'copied' ? 'Copied' : state === 'failed' ? 'Copy failed' : 'Copy'}
    </Button>
  );
}
