import { useEffect, useState } from 'react';
import {
  ApiError,
  useAddCloneRootMutation,
  useHomeDirQuery,
  useRemoveCloneRootMutation,
  useCloneRootsQuery,
} from '@delta/api-client';
import type { CloneRoot } from '@delta/wire-gen';
import { Button, Dialog, Spinner } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { displayPath } from '../../utils/displayPath';
import { WorkdirPickerBody } from '../composer/WorkdirPickerBody';

/**
 * Clone roots category content: list of the registered directories where the
 * user's git clones live — every Repository tab refetch probes their direct
 * children for clones — plus a one-shot picker to register a new one. Drives
 * the same backend the New session screen consults, so adding a clone root
 * here surfaces previously-hidden clones on the very next refetch.
 *
 * `active` mirrors the dialog's `settingsOpen` AND the category being the
 * visible one, so the query only runs while the section is mounted.
 */
export function CloneRootsSection({ active }: { active: boolean }) {
  const client = useApiClient();
  const cloneRootsQuery = useCloneRootsQuery(client, active);
  const addCloneRoot = useAddCloneRootMutation(client);
  const removeCloneRoot = useRemoveCloneRootMutation(client);
  const home = useHomeDirQuery(client, active).data?.path ?? null;

  const [pickerOpen, setPickerOpen] = useState(false);
  // The picker's candidate selection, lifted to this component so the dialog's
  // Add button can drive submission and the Cancel button can dismiss without
  // committing.
  const [candidate, setCandidate] = useState<string | null>(null);

  // The mutation's error is surfaced inline on the picker dialog so a 409
  // duplicate shows a tiny "Already registered." hint instead of a global
  // toast. Cleared whenever the picker reopens.
  const duplicate =
    addCloneRoot.error instanceof ApiError &&
    addCloneRoot.error.code === 'clone_root_duplicate';

  // React Query's mutation handle is a fresh object every render, so it
  // cannot sit in the effect dependencies (it would re-fire forever). Pull
  // `reset` out and depend on it (a stable function reference within a given
  // QueryClient lifetime), which keeps the effect well-behaved.
  const resetCloneRootMutation = addCloneRoot.reset;
  useEffect(() => {
    if (!pickerOpen) {
      setCandidate(null);
      resetCloneRootMutation();
    }
  }, [pickerOpen, resetCloneRootMutation]);

  const submit = () => {
    if (candidate === null) {
      return;
    }
    addCloneRoot.mutate(
      { path: candidate },
      {
        onSuccess: () => setPickerOpen(false),
      },
    );
  };

  const cloneRoots = cloneRootsQuery.data?.clone_roots ?? [];

  return (
    <section className="space-y-3" data-testid="clone-roots-section">
      <div>
        <h3 className="mb-1 text-secondary font-semibold text-fg">
          Clone roots
        </h3>
        <p className="text-caption text-fg-muted">
          Delta scans the direct children of each path below for git
          repositories so you can pick them from the Repository tab without
          having to start a session there first.
        </p>
      </div>

      {cloneRootsQuery.isPending ? (
        <div className="flex justify-center py-4">
          <Spinner label="loading clone roots" />
        </div>
      ) : cloneRootsQuery.isError ? (
        <div className="flex flex-col items-center gap-2 py-4 text-secondary text-fg-muted">
          <p>Could not load clone roots.</p>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => cloneRootsQuery.refetch()}
          >
            Retry
          </Button>
        </div>
      ) : cloneRoots.length === 0 ? (
        <p className="py-3 text-center text-secondary text-fg-subtle">
          No clone roots registered yet.
        </p>
      ) : (
        <ul className="flex flex-col gap-2" data-testid="clone-roots-list">
          {cloneRoots.map((root) => (
            <CloneRootRow
              key={root.path}
              root={root}
              home={home}
              onRemove={() => removeCloneRoot.mutate(root.path)}
              removing={
                removeCloneRoot.isPending && removeCloneRoot.variables === root.path
              }
            />
          ))}
        </ul>
      )}

      <div className="flex justify-end">
        <Button
          size="sm"
          variant="secondary"
          onClick={() => setPickerOpen(true)}
          data-testid="add-clone-root"
        >
          Add clone root…
        </Button>
      </div>

      <Dialog
        open={pickerOpen}
        onClose={() => setPickerOpen(false)}
        title="Pick a parent directory"
        footer={
          <>
            <Button variant="ghost" onClick={() => setPickerOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={submit}
              disabled={candidate === null || addCloneRoot.isPending}
              data-testid="clone-root-confirm"
            >
              Add
            </Button>
          </>
        }
      >
        <div className="space-y-3">
          <p className="text-caption text-fg-muted">
            Pick the parent of one or more git clones. The Repository tab will
            then probe its direct children for <code>.git</code> on every
            refetch.
          </p>
          <WorkdirPickerBody
            active={pickerOpen}
            candidate={candidate}
            setCandidate={setCandidate}
            onConfirm={submit}
            showHelpText={false}
          />
          {duplicate && (
            <p
              className="text-caption text-warning"
              role="alert"
              data-testid="clone-root-duplicate"
            >
              Already registered.
            </p>
          )}
          {addCloneRoot.isError && !duplicate && (
            <p className="text-caption text-danger" role="alert">
              Could not add the clone root. Please try again.
            </p>
          )}
        </div>
      </Dialog>
    </section>
  );
}

interface CloneRootRowProps {
  root: CloneRoot;
  home: string | null;
  onRemove: () => void;
  removing: boolean;
}

function CloneRootRow({ root, home, onRemove, removing }: CloneRootRowProps) {
  return (
    <li className="flex items-center justify-between gap-3 rounded-lg border border-border-default px-3 py-2">
      <span
        className="truncate font-mono text-code text-fg"
        title={root.path}
      >
        {displayPath(root.path, home)}
      </span>
      <Button
        size="sm"
        variant="ghost"
        onClick={onRemove}
        disabled={removing}
        aria-label={`Remove clone root ${root.path}`}
      >
        Remove
      </Button>
    </li>
  );
}
