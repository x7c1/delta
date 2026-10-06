import type { ReactNode } from 'react';
import { useStorageQuery } from '@delta/api-client';
import type { StorageResponse } from '@delta/wire-gen';
import { Button, Spinner } from '@delta/ui-kit';
import { useApiClient } from '../../../data/apiContext';
import { PruneSessionsBlock } from './PruneSessionsBlock';
import { SnapshotList } from './SnapshotList';
import { CopyButton, PathText, Size } from './StorageParts';
import { WorktreesBlock } from './WorktreesBlock';

/**
 * Storage category content: where this running Delta keeps its files on this
 * machine, and how large the database is — read from `GET /api/storage`, so it
 * names the directories *this* install actually uses rather than the
 * documented defaults.
 *
 * One row per location, each with a copy control; the database's migration
 * snapshots can be deleted from under its row. Below the inventory, the
 * cleanup blocks: removing old sessions in bulk, and the worktrees left under
 * the worktree base. `active` gates every fetch to while the category is
 * shown; the queries refetch on a later visit once their answers are stale,
 * so the sizes follow the files on disk.
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

      <PruneSessionsBlock active={active} />
      <WorktreesBlock active={active} />
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
          <SnapshotList snapshots={storage.snapshots} />
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
