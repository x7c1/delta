import { useEffect, useState } from 'react';
import { useHomeDirQuery } from '@delta/api-client';
import { Button, Dialog } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useComposerStore } from '../../store/composerStore';
import { displayPath } from '../../utils/displayPath';
import { WorkdirPickerBody } from './WorkdirPickerBody';

/**
 * The folder glyph used to mark the chip's change button. Decorative —
 * always `aria-hidden`, so the button's accessible name stays its label.
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
  const setSelected = useComposerStore((state) => state.setNewSessionWorkdir);

  // The highlighted answer-in-waiting. Committed to the store only on Select.
  const [candidate, setCandidate] = useState<string | null>(null);

  // Reset the candidate whenever the dialog closes, so a reopen starts fresh
  // rather than reusing a previous in-flight pick.
  useEffect(() => {
    if (!open) {
      setCandidate(null);
    }
  }, [open]);

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
      <WorkdirPickerBody
        active={open}
        candidate={candidate}
        setCandidate={setCandidate}
        onConfirm={confirm}
      />
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
      className="flex items-center gap-2 rounded border border-accent/30 bg-accent/10 px-2 py-1 text-xs"
      data-testid="workdir-chip"
    >
      <span className="shrink-0 font-medium text-accent">Start in:</span>
      <span
        className="min-w-0 flex-1 truncate font-mono text-fg"
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
