/**
 * The clone roots category.
 *
 * Shared fixtures live in SettingsView.testkit.tsx.
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
import { createHandlers } from '@delta/api-mocks';
import {
  renderSettings,
  resetSettingsState,
  server,
  switchToCloneRoots,
  unstubGlobals,
} from './SettingsView.testkit';

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
// A fresh mock registry per test, not just a reset of the per-test overrides:
// `createHandlers()` closes over an in-memory store, so one shared instance
// would let a template (or launch option) created by one test leak into the
// next one's list and make the file order-dependent.
beforeEach(() => server.resetHandlers(...createHandlers()));
afterAll(() => server.close());

describe('SettingsView', () => {
  beforeEach(resetSettingsState);
  afterEach(unstubGlobals);

  describe('Clone roots section', () => {
    it('renders the empty-state when no roots are registered', async () => {
      renderSettings();
      switchToCloneRoots();
      const section = await screen.findByTestId('clone-roots-section');
      expect(
        within(section).getByText('No clone roots registered yet.'),
      ).toBeInTheDocument();
      expect(within(section).queryByTestId('clone-roots-list')).toBeNull();
    });

    it('adds a clone root through the picker dialog', async () => {
      renderSettings();
      switchToCloneRoots();
      const section = await screen.findByTestId('clone-roots-section');
      fireEvent.click(within(section).getByTestId('add-clone-root'));

      // The picker dialog opens; the WorkdirPickerBody pre-selects the most
      // recent directory as its candidate, so Add becomes enabled without
      // needing to click a tree node first.
      const confirm = await screen.findByTestId('clone-root-confirm');
      await waitFor(() => expect(confirm).toBeEnabled());
      fireEvent.click(confirm);

      // After success the picker closes and the list shows the new entry.
      await waitFor(() =>
        expect(within(section).getByTestId('clone-roots-list')).toBeInTheDocument(),
      );
    });

    it('shows an inline duplicate hint when adding the same root twice', async () => {
      renderSettings();
      switchToCloneRoots();
      const section = await screen.findByTestId('clone-roots-section');

      // First registration: the picker opens, the candidate is auto-pre-
      // selected (the WorkdirPickerBody seeds it from the recent list), and
      // submitting succeeds.
      fireEvent.click(within(section).getByTestId('add-clone-root'));
      const firstConfirm = await screen.findByTestId('clone-root-confirm');
      await waitFor(() => expect(firstConfirm).toBeEnabled());
      fireEvent.click(firstConfirm);
      await waitFor(() =>
        expect(within(section).getByTestId('clone-roots-list')).toBeInTheDocument(),
      );

      // Second attempt at the same path: the server replies 409 with the
      // stable code, and the picker shows an inline "Already registered" hint
      // instead of a global toast.
      fireEvent.click(within(section).getByTestId('add-clone-root'));
      const secondConfirm = await screen.findByTestId('clone-root-confirm');
      await waitFor(() => expect(secondConfirm).toBeEnabled());
      fireEvent.click(secondConfirm);
      await waitFor(() =>
        expect(screen.getByTestId('clone-root-duplicate')).toBeInTheDocument(),
      );
    });

    it('removes a registered clone root', async () => {
      renderSettings();
      switchToCloneRoots();
      const section = await screen.findByTestId('clone-roots-section');

      // Register one first so there is a row to remove.
      fireEvent.click(within(section).getByTestId('add-clone-root'));
      const confirm = await screen.findByTestId('clone-root-confirm');
      await waitFor(() => expect(confirm).toBeEnabled());
      fireEvent.click(confirm);
      const list = await within(section).findByTestId('clone-roots-list');

      // Click the row's Remove button (the only button inside the list row),
      // and the row disappears as the list refetches.
      const removeButton = within(list).getByRole('button', {
        name: /Remove clone root /,
      });
      fireEvent.click(removeButton);
      await waitFor(() =>
        expect(within(section).queryByTestId('clone-roots-list')).toBeNull(),
      );
      expect(
        within(section).getByText('No clone roots registered yet.'),
      ).toBeInTheDocument();
    });
  });
});
