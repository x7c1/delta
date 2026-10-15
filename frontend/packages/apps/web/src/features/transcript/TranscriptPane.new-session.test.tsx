/**
 * The new-session state: its tabs, the directory chip and the picker.
 *
 * Shared fixtures live in TranscriptPane.testkit.tsx.
 */
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { SESSION_ID, mockThreads } from '@delta/api-mocks';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
import {
  renderNewSessionPane,
  resetTranscriptPaneState,
  server,
} from './TranscriptPane.testkit';

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('TranscriptPane', () => {
  beforeEach(resetTranscriptPaneState);

  it('lands on the Repository tab in the new-session state', async () => {
    // Phase B retired the auto-opened modal: the new-session screen shows
    // the 3-tab picker (PR / Repository / Directory) inline and defaults to
    // Repository, which is the recency-ordered registered-repo list.
    renderNewSessionPane();

    expect(
      await screen.findByTestId('new-session-tab-repository'),
    ).toHaveAttribute('aria-selected', 'true');
    // The modal is NOT auto-opened anymore.
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('hoists the tab strip into the Panel header instead of a "New session" label', async () => {
    // The tabs are pinned at the top of the pane (the Panel header lives
    // outside the scrolling body), and the plain "New session" label is
    // gone — the tabs convey the screen's identity.
    renderNewSessionPane();
    const tablist = await screen.findByRole('tablist', {
      name: 'Start a session from',
    });
    // The Panel renders its header inside a <header> element; the tablist
    // sits inside it, above the scroll viewport.
    expect(tablist.closest('header')).not.toBeNull();
    // The old plain "New session" label is removed.
    expect(screen.queryByText('New session', { exact: true })).toBeNull();
  });

  it('does not pop a modal even when workdirMandatory is set', async () => {
    // First run (no sessions to fall back to): in Phase B the tabbed picker
    // replaces the auto-opened modal entirely, so `workdirMandatory` has no
    // modal to gate. The new-session intent is preserved, and the Directory
    // tab is reachable for the inline picker. The modal's own
    // non-dismissable behaviour is exercised by WorkdirDialog.test.tsx.
    renderNewSessionPane(mockThreads, { workdirMandatory: true });

    expect(
      await screen.findByTestId('new-session-tab-directory'),
    ).toBeInTheDocument();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
  });

  it('shows a chip with an edit affordance once a directory is selected', async () => {
    useComposerStore.setState({ newSessionWorkdir: '/home/dev/projects/delta' });
    renderNewSessionPane();

    const chip = screen.getByTestId('workdir-chip');
    // The path label collapses home to `~` once $HOME is known, while the
    // full path is preserved in the title for hover.
    await waitFor(() =>
      expect(chip).toHaveTextContent('Start in:~/projects/delta'),
    );
    expect(
      within(chip).getByTitle('/home/dev/projects/delta'),
    ).toBeInTheDocument();
    // The ✎ reopens the dialog rather than clearing the (mandatory) selection.
    expect(
      within(chip).getByRole('button', { name: 'Change working directory' }),
    ).toBeInTheDocument();
  });

  it('reopens the picker from the chip ✎ without resetting the selection', async () => {
    useComposerStore.setState({ newSessionWorkdir: '/home/dev/projects/delta' });
    renderNewSessionPane();

    // The picker starts closed (a directory is already selected, so the
    // auto-open effect does not fire).
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();

    fireEvent.click(
      within(screen.getByTestId('workdir-chip')).getByRole('button', {
        name: 'Change working directory',
      }),
    );

    // The ✎ opens the picker via openWorkdirDialog (no reset), so the chosen
    // directory is still in the store while editing.
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    expect(useComposerStore.getState().newSessionWorkdir).toBe(
      '/home/dev/projects/delta',
    );
  });

  it('shows no chip and disables Send when no workdir is selected (no session to return to)', async () => {
    // Phase B: no auto-opened modal. The new-session screen waits for the
    // user to pick a starting point from the inline tabs. With nothing
    // picked, no chip shows and Send stays disabled; the new-session intent
    // is preserved because the empty initial screen has nowhere to return
    // to.
    useNavStore.setState({
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: null,
    });
    renderNewSessionPane();

    expect(await screen.findByTestId('new-session-tabs')).toBeInTheDocument();
    expect(screen.queryByTestId('workdir-chip')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
  });

  it('returns to the previously-focused session when the chip-opened modal is dismissed without a selection', async () => {
    // Phase B: there is no auto-opened modal. The cancel path here is
    // chip ✎ → modal → Cancel. To exercise it the test seeds a workdir
    // (so the chip is present), clicks the chip's pencil button to open
    // the picker, then cancels. With nothing in `newSessionWorkdir` after
    // the (no-op) cancel, the empty-back-out path restores the previously-
    // focused session. Strictly: the cancel clears the candidate via the
    // dialog's onClose, but does NOT clear the already-committed selection,
    // so the contract here is "previously-focused session restoration
    // only fires when the workdir is unset at dismiss time". To mirror
    // that, the test clears the workdir right before clicking Cancel.
    useNavStore.setState({
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: SESSION_ID,
    });
    useComposerStore.setState({ newSessionWorkdir: '/home/dev/projects/delta' });
    renderNewSessionPane();

    fireEvent.click(
      within(screen.getByTestId('workdir-chip')).getByRole('button', {
        name: 'Change working directory',
      }),
    );

    // Clear the seeded workdir so cancellation takes the
    // "no selection at dismiss time" path that restores the prior focus.
    useComposerStore.setState({ newSessionWorkdir: null });
    fireEvent.click(await screen.findByTestId('workdir-cancel'));

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID),
    );
    expect(useNavStore.getState().preNewSessionFocus).toBeNull();
  });

  it('stays in new-session when a directory has been selected (dismiss does not cancel)', async () => {
    // A previous session is recorded, but a directory is already chosen, so
    // dismissing the picker (e.g. the ✎-reopen-then-close path) must NOT cancel
    // the new-session intent.
    useNavStore.setState({
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: SESSION_ID,
    });
    useComposerStore.setState({ newSessionWorkdir: '/home/dev/projects/delta' });
    renderNewSessionPane();

    // Reopen via the chip's edit affordance, then dismiss.
    fireEvent.click(
      within(screen.getByTestId('workdir-chip')).getByRole('button', {
        name: 'Change working directory',
      }),
    );
    fireEvent.click(await screen.findByTestId('workdir-cancel'));

    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    // Still in new-session: a directory is selected, so the cancel was skipped.
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
  });
});
