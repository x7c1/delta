import { beforeEach, describe, expect, it } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { useComposerStore } from '../../store/composerStore';
import { ProviderSelector } from './ProviderSelector';

describe('ProviderSelector', () => {
  beforeEach(() => {
    // Reset to the default provider so each test starts from a known selection.
    useComposerStore.setState({ newSessionProvider: 'claude' });
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
});
