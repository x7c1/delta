import { useState } from 'react';
import { useComposerStore } from '../../../store/composerStore';
import { WorkdirPickerBody } from '../../composer/WorkdirPickerBody';

/**
 * The Directory tab: the same Recent + Browse picker the old modal showed,
 * now lifted inline so the user does not have to dismiss a dialog to see the
 * other tabs. The picker's candidate is committed to
 * `composerStore.newSessionWorkdir` straight away — there is no Cancel /
 * Select footer here because the tab itself is the picker's surface; a
 * confirm gesture (click a row) is the same gesture that commits.
 */
export function DirectoryTab() {
  const setSelected = useComposerStore((state) => state.setNewSessionWorkdir);
  const selectedPath = useComposerStore((state) => state.newSessionWorkdir);
  // Local candidate state mirrors the dialog's: the picker body lifts it
  // out so the Recent and Browse rows can highlight together. The tab
  // commits immediately rather than waiting on a Select button.
  const [candidate, setCandidate] = useState<string | null>(selectedPath);

  // Committing here stamps `directory` provenance (see `setNewSessionWorkdir`),
  // which drops any PR pick: at most one row stays highlighted across the three
  // tabs, and the worktree controls unlock.
  const commit = (path: string | null) => {
    setCandidate(path);
    if (path !== null) {
      setSelected(path);
    }
  };

  return (
    <div className="space-y-2" data-testid="new-session-directory-tab">
      <WorkdirPickerBody
        active={true}
        candidate={candidate}
        setCandidate={commit}
        // Enter on the highlighted row: the same commit as a click, routed
        // through `commit` so the two gestures cannot drift apart.
        onConfirm={() => commit(candidate)}
      />
    </div>
  );
}
