/**
 * The settings dialog itself: opening and closing it, and the category
 * sidebar.
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
import { fireEvent, screen, within } from '@testing-library/react';
import { createHandlers } from '@delta/api-mocks';
import { useNavStore } from '../../store/navStore';
import type { SettingsCategoryId } from '../../store/settingsStore';
import {
  SETTINGS_STORAGE_KEY,
  useSettingsStore,
} from '../../store/settingsStore';
import {
  findList,
  renderSettings,
  resetSettingsState,
  server,
  switchToCloneRoots,
  unstubGlobals,
} from './SettingsView.testkit';

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
// A fresh mock registry per test, not just a reset of the per-test overrides:
// `createHandlers()` closes over an in-memory store, so one shared instance
// would let a template (or launch option) created by one test leak into the
// next one's list and make the file order-dependent.
beforeEach(() => server.resetHandlers(...createHandlers()));
afterAll(() => server.close());

describe('SettingsView', () => {
  beforeEach(resetSettingsState);
  afterEach(unstubGlobals);

  it('renders nothing while the settings overlay is closed', () => {
    useNavStore.setState({ settingsOpen: false });
    renderSettings();
    expect(screen.queryByTestId('dialog-backdrop')).toBeNull();
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('renders the launch options inside the dialog overlay', async () => {
    renderSettings();
    await findList();
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByTestId('launch-options-list')).toBeInTheDocument();
  });

  it('closes the settings overlay via Close', async () => {
    renderSettings();
    await findList();
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(useNavStore.getState().settingsOpen).toBe(false);
  });

  describe('category sidebar', () => {
    it('opens on the persisted category and reveals its right pane', () => {
      useSettingsStore.setState({ activeCategory: 'clone-roots' });
      renderSettings();
      expect(screen.getByTestId('settings-category-clone-roots')).toHaveAttribute(
        'aria-selected',
        'true',
      );
      expect(screen.getByTestId('clone-roots-section')).toBeInTheDocument();
      expect(screen.queryByTestId('launch-options-section')).toBeNull();
    });

    it('renders the first category when the active id is not in the registry', () => {
      // A category id an earlier build persisted under a name this one has
      // since renamed (or any other value the registry does not know) reaches
      // the view only if it slips past the store's rehydration guard — a
      // `setState` from stale code, a future id, a hand-edited localStorage.
      // The rail-to-pane lookup must still land on a real category rather than
      // leaving the right pane blank with every rail entry unselected.
      useSettingsStore.setState({
        activeCategory: 'retired-category' as SettingsCategoryId,
      });
      renderSettings();
      expect(screen.getByTestId('launch-options-section')).toBeInTheDocument();
      expect(screen.queryByTestId('clone-roots-section')).toBeNull();
      // The rail follows the same fallback, so the rendered pane still has its
      // tab marked selected instead of the rail showing no selection at all.
      expect(
        screen.getByTestId('settings-category-launch-options'),
      ).toHaveAttribute('aria-selected', 'true');
    });

    it('switches the right pane content when a category is clicked', async () => {
      renderSettings();
      // Default landing pane is Launch options; the clone-roots pane is not
      // mounted yet.
      expect(screen.getByTestId('launch-options-section')).toBeInTheDocument();
      expect(screen.queryByTestId('clone-roots-section')).toBeNull();

      switchToCloneRoots();

      expect(screen.queryByTestId('launch-options-section')).toBeNull();
      expect(await screen.findByTestId('clone-roots-section')).toBeInTheDocument();
    });

    it('persists the active category to localStorage', () => {
      renderSettings();
      switchToCloneRoots();
      const raw = localStorage.getItem(SETTINGS_STORAGE_KEY);
      expect(raw).not.toBeNull();
      // The persist middleware wraps state under `{ state, version }`; assert
      // on the parsed shape rather than substring-matching the JSON.
      const parsed = JSON.parse(raw ?? '{}');
      expect(parsed.state.activeCategory).toBe('clone-roots');
    });

    it('exposes a vertical tablist with one tab per category', () => {
      renderSettings();
      const tablist = screen.getByRole('tablist');
      expect(tablist).toHaveAttribute('aria-orientation', 'vertical');
      const tabs = within(tablist).getAllByRole('tab');
      expect(tabs.map((t) => t.textContent)).toEqual([
        'Launch options',
        'Prompt templates',
        'Clone roots',
        'Appearance',
        'Default provider',
        'Storage',
      ]);
      expect(
        screen.getByTestId('settings-category-launch-options'),
      ).toHaveAttribute('aria-selected', 'true');
      expect(
        screen.getByTestId('settings-category-prompt-templates'),
      ).toHaveAttribute('aria-selected', 'false');
      expect(
        screen.getByTestId('settings-category-clone-roots'),
      ).toHaveAttribute('aria-selected', 'false');
      expect(
        screen.getByTestId('settings-category-appearance'),
      ).toHaveAttribute('aria-selected', 'false');
      expect(
        screen.getByTestId('settings-category-default-provider'),
      ).toHaveAttribute('aria-selected', 'false');
    });

  });
});
