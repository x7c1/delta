import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { useComposerStore } from '../../store/composerStore';
import { useSettingsStore } from '../../store/settingsStore';
import { ProviderSelector } from './ProviderSelector';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

/**
 * Override `GET /api/providers` so a specific set of providers is unavailable,
 * each with a reason. Providers not listed default to available.
 */
function useProvidersAvailability(
  overrides: Partial<Record<'claude' | 'codex', string>>,
) {
  server.use(
    http.get('*/api/providers', () =>
      HttpResponse.json({
        providers: (['claude', 'codex'] as const).map((provider) => {
          const detail = overrides[provider];
          return detail
            ? { provider, available: false, detail }
            : { provider, available: true, detail: null };
        }),
      }),
    ),
  );
}

function renderSelector() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <ProviderSelector />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

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
    renderSelector();

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
    renderSelector();

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
    renderSelector();

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
    renderSelector();

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
    renderSelector();

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
    renderSelector();

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

  it('disables an unavailable provider and shows the server reason', async () => {
    useProvidersAvailability({
      codex: "The 'codex' binary for codex was not found on PATH.",
    });
    renderSelector();

    // Once availability lands, the Codex radio is disabled and the reason is
    // shown; Claude stays available and selectable.
    const codexRadio = within(
      await screen.findByTestId('provider-option-codex'),
    ).getByRole('radio');
    await waitFor(() => expect(codexRadio).toBeDisabled());

    const notice = await screen.findByTestId('provider-unavailable-codex');
    expect(notice).toHaveTextContent(
      "The 'codex' binary for codex was not found on PATH.",
    );

    const claudeRadio = within(
      screen.getByTestId('provider-option-claude'),
    ).getByRole('radio');
    expect(claudeRadio).toBeEnabled();
    expect(claudeRadio).toBeChecked();
  });

  it('does not select a disabled provider on click', async () => {
    useProvidersAvailability({
      codex: 'Codex is not installed on this host.',
    });
    renderSelector();

    const codexRadio = within(
      await screen.findByTestId('provider-option-codex'),
    ).getByRole('radio');
    await waitFor(() => expect(codexRadio).toBeDisabled());

    // A click on a disabled radio must not change the selection.
    fireEvent.click(codexRadio);
    expect(useComposerStore.getState().newSessionProvider).toBe('claude');
    expect(codexRadio).not.toBeChecked();
  });

  it('falls back off an unavailable default onto an available provider', async () => {
    // The persisted default is Codex, but Codex is unavailable on this host: the
    // selector must not leave the form on a provider that cannot launch — it
    // falls back to Claude (available).
    useComposerStore.setState({
      newSessionProvider: 'claude',
      newSessionProviderSeeded: false,
    });
    useSettingsStore.setState({ defaultProvider: 'codex' });
    useProvidersAvailability({
      codex: "The 'codex' binary for codex was not found on PATH.",
    });
    renderSelector();

    await waitFor(() => {
      expect(useComposerStore.getState().newSessionProvider).toBe('claude');
    });
    const claudeRadio = within(
      screen.getByTestId('provider-option-claude'),
    ).getByRole('radio');
    expect(claudeRadio).toBeChecked();
    const codexRadio = within(
      screen.getByTestId('provider-option-codex'),
    ).getByRole('radio');
    expect(codexRadio).toBeDisabled();
  });
});
