import { beforeEach, describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import {
  DEFAULT_NEW_SESSION_TAB,
  useComposerStore,
} from '../../store/composerStore';
import { NewSessionTabBar } from './NewSessionTabBar';

describe('NewSessionTabBar', () => {
  beforeEach(() => {
    useComposerStore.setState({ newSessionTab: DEFAULT_NEW_SESSION_TAB });
  });

  it('marks the store-selected tab as active', () => {
    useComposerStore.setState({ newSessionTab: 'pr' });
    render(<NewSessionTabBar />);
    expect(screen.getByTestId('new-session-tab-pr')).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(screen.getByTestId('new-session-tab-repository')).toHaveAttribute(
      'aria-selected',
      'false',
    );
  });

  it('clicking a tab persists the choice to the store', () => {
    render(<NewSessionTabBar />);
    fireEvent.click(screen.getByTestId('new-session-tab-pr'));
    expect(useComposerStore.getState().newSessionTab).toBe('pr');

    fireEvent.click(screen.getByTestId('new-session-tab-directory'));
    expect(useComposerStore.getState().newSessionTab).toBe('directory');
  });

  it('exposes a tablist role with the three named tabs', () => {
    render(<NewSessionTabBar />);
    expect(screen.getByRole('tablist')).toBeInTheDocument();
    // PR / Repository / Directory left-to-right, regardless of how the
    // union type is declared in the store.
    const tabs = screen.getAllByRole('tab');
    expect(tabs.map((t) => t.textContent)).toEqual([
      'PR',
      'Repository',
      'Directory',
    ]);
  });
});
