import { useState } from 'react';
import { useDeleteSnapshotMutation } from '@delta/api-client';
import type { StorageFile } from '@delta/wire-gen';
import { Button } from '@delta/ui-kit';
import { useApiClient } from '../../../data/apiContext';
import { ConfirmPanel } from './ConfirmPanel';
import { formatBytes } from './formatBytes';
import { baseName, CopyButton, errorMessage, PathText, Size } from './StorageParts';

/**
 * The database's migration snapshots, under its row, each with a Delete
 * control. Said in one line when there are none, so the absence reads as a
 * fact rather than a missing row.
 *
 * The migration runner writes a snapshot before a step that rewrites data and
 * never removes it, so deleting one is the user's call: the confirmation
 * names the file and its size, since a deleted snapshot cannot be restored.
 */
export function SnapshotList({ snapshots }: { snapshots: StorageFile[] }) {
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
          <SnapshotRow key={snapshot.path} snapshot={snapshot} />
        ))}
      </ul>
    </div>
  );
}

function SnapshotRow({ snapshot }: { snapshot: StorageFile }) {
  const client = useApiClient();
  const deleteSnapshot = useDeleteSnapshotMutation(client);
  const [confirming, setConfirming] = useState(false);
  const name = baseName(snapshot.path);

  const cancel = () => {
    setConfirming(false);
    deleteSnapshot.reset();
  };

  return (
    <li className="flex flex-col gap-1" data-testid="storage-snapshot">
      <div className="flex items-center gap-2">
        <PathText value={snapshot.path} />
        <Size bytes={snapshot.bytes} />
        <CopyButton value={snapshot.path} label="snapshot path" />
        {!confirming && (
          <Button
            size="sm"
            variant="ghost"
            className="shrink-0"
            onClick={() => setConfirming(true)}
            aria-label={`Delete snapshot ${name}`}
          >
            Delete
          </Button>
        )}
      </div>
      {confirming && (
        <ConfirmPanel
          confirmLabel="Delete snapshot"
          onConfirm={() => deleteSnapshot.mutate({ path: snapshot.path })}
          onCancel={cancel}
          pending={deleteSnapshot.isPending}
          error={deleteSnapshot.isError ? errorMessage(deleteSnapshot.error) : null}
          testId="storage-snapshot-confirm"
        >
          <p>
            Delete <span className="font-mono">{name}</span> (
            {formatBytes(snapshot.bytes)})? It is a copy of the database from
            before an upgrade, and cannot be restored once deleted.
          </p>
        </ConfirmPanel>
      )}
    </li>
  );
}
