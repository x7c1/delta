import { beforeEach, describe, expect, it } from 'vitest';
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { useComposerStore } from '../../store/composerStore';
import { useSettingsStore } from '../../store/settingsStore';
import { ProviderSelector } from './ProviderSelector';

describe('ProviderSelector', () => {
  beforeEach(() => {
    // A fresh, not-yet-seeded new-session compose state, and the default
    // provider preference back at Claude so each test starts from a known seed.
    useComposerStore.setState({
      newSessionProvider: 'claude',
      newSessionProviderSeeded: false,
    });
    useSettingsStore.setState({ defaultProvider: 'claude' });
  });

  it('renders both providers as radios with their badges and names', () => {
    render(<ProviderSelector />);

    const radios = screen.getAllByRole('radio');
    expect(radios).toHaveLength(2);

    const claude = screen.getByTestId('provider-option-claude');
    const codex = screen.getByTestId('provider-option-codex');
    // Each option carries the shared ProviderBadge (accessible name = product
    // name) plus its spelled-out label.
    expect(within(claude).getByLabelText('Claude Code')).toBeInTheDocument();
    expect(within(claude).getByText('Claude Code')).toBeInTheDocument();
    expect(within(codex).getByLabelText('Codex')).toBeInTheDocument();
    expect(within(codex).getByText('Codex')).toBeInTheDocument();
  });

  it('reflects the store selection: Claude checked by default', () => {
    render(<ProviderSelector />);

    const claude = within(
      screen.getByTestId('provider-option-claude'),
    ).getByRole('radio');
    const codex = within(
      screen.getByTestId('provider-option-codex'),
    ).getByRole('radio');
    expect(claude).toBeChecked();
    expect(codex).not.toBeChecked();
  });

  it('writes the picked provider to the composer store', () => {
    render(<ProviderSelector />);

    const codex = within(
      screen.getByTestId('provider-option-codex'),
    ).getByRole('radio');
    fireEvent.click(codex);
    expect(useComposerStore.getState().newSessionProvider).toBe('codex');
    expect(codex).toBeChecked();

    const claude = within(
      screen.getByTestId('provider-option-claude'),
    ).getByRole('radio');
    fireEvent.click(claude);
    expect(useComposerStore.getState().newSessionProvider).toBe('claude');
  });

  it('seeds the initial provider from the persisted default (Codex)', async () => {
    // A fresh new-session compose (unseeded, provider at the Claude constant)
    // with the default preference set to Codex: entering it seeds the selector
    // to Codex, so a resulting send would carry provider: 'codex'.
    useComposerStore.setState({
      newSessionProvider: 'claude',
      newSessionProviderSeeded: false,
    });
    useSettingsStore.setState({ defaultProvider: 'codex' });
    render(<ProviderSelector />);

    await waitFor(() => {
      expect(useComposerStore.getState().newSessionProvider).toBe('codex');
    });
    const codex = within(
      screen.getByTestId('provider-option-codex'),
    ).getByRole('radio');
    expect(codex).toBeChecked();
    expect(useComposerStore.getState().newSessionProviderSeeded).toBe(true);
  });

  it('does not re-seed once the provider has been seeded', async () => {
    // Already seeded to Claude for this compose; a Codex default must not
    // retroactively overwrite the seeded selection.
    useComposerStore.setState({
      newSessionProvider: 'claude',
      newSessionProviderSeeded: true,
    });
    useSettingsStore.setState({ defaultProvider: 'codex' });
    render(<ProviderSelector />);

    // Give any stray seed effect a chance to (incorrectly) fire.
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionProviderSeeded).toBe(true);
    });
    expect(useComposerStore.getState().newSessionProvider).toBe('claude');
  });

  it('preserves an explicit pick when the default changes mid-compose', async () => {
    // Seed to Codex, then the user explicitly picks Claude. A later change to
    // the persisted default must not clobber that explicit per-session choice.
    useComposerStore.setState({
      newSessionProvider: 'claude',
      newSessionProviderSeeded: false,
    });
    useSettingsStore.setState({ defaultProvider: 'codex' });
    render(<ProviderSelector />);

    await waitFor(() => {
      expect(useComposerStore.getState().newSessionProvider).toBe('codex');
    });

    const claude = within(
      screen.getByTestId('provider-option-claude'),
    ).getByRole('radio');
    fireEvent.click(claude);
    expect(useComposerStore.getState().newSessionProvider).toBe('claude');

    // The default flips again while the compose is still open; the seed guard
    // keeps the user's explicit Claude choice intact.
    act(() => {
      useSettingsStore.setState({ defaultProvider: 'codex' });
    });
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionProviderSeeded).toBe(true);
    });
    expect(useComposerStore.getState().newSessionProvider).toBe('claude');
  });
});
