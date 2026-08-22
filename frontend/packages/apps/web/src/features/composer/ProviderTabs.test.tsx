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
import { ComposerRail } from './ComposerRail';
import { ProviderTabs } from './ProviderTabs';
import { ProviderUnavailableNotice } from './ProviderUnavailableNotice';

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
          // Capabilities mirror the real backend so this override stays a
          // faithful `/api/providers` shape: Claude has a terminal and
          // flag-style launch options, Codex has neither. The selector reads
          // `available`, not capabilities, but the shape must remain complete.
          const capabilities = {
            has_terminal: provider === 'claude',
            launch_option_style:
              provider === 'claude' ? ('cli_flag' as const) : ('request_field' as const),
          };
          return detail
            ? { provider, available: false, detail, capabilities }
            : { provider, available: true, detail: null, capabilities };
        }),
      }),
    ),
  );
}

/**
 * Render the provider control the way the composer composes it: the tabs on the
 * rail, and the reasons (too long for a tab) below it, standing in for the
 * composer card. Both halves read one availability verdict set, so the pair is
 * exercised together.
 */
function renderSelector() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <ComposerRail providerTabs={<ProviderTabs />} />
        <ProviderUnavailableNotice />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('ProviderTabs', () => {
  beforeEach(() => {
    // A fresh, not-yet-seeded new-session compose state, and the default
    // provider preference back at Claude so each test starts from a known seed.
    useComposerStore.setState({
      newSessionProvider: 'claude',
      newSessionProviderSeeded: false,
    });
    useSettingsStore.setState({ defaultProvider: 'claude' });
  });

  it('renders both providers as radios with their hue-tinted names', () => {
    renderSelector();

    const radios = screen.getAllByRole('radio');
    expect(radios).toHaveLength(2);

    const claude = screen.getByTestId('provider-option-claude');
    const codex = screen.getByTestId('provider-option-codex');
    // Each option writes its product name through the shared ProviderName,
    // which tints the words in the provider hue — the same hue channel as the
    // session card's kebab trigger.
    const claudeName = within(claude).getByText('Claude Code');
    expect(claudeName.className).toContain('text-provider-claude');
    const codexName = within(codex).getByText('Codex');
    expect(codexName.className).toContain('text-provider-codex');
  });

  it('rides the rail as one item resting on the card top border', () => {
    renderSelector();

    // The tabs live on the rail, not in the card's stack.
    const rail = screen.getByTestId('composer-rail');
    const tabs = screen.getByTestId('provider-selector');
    expect(rail).toContainElement(tabs);

    // A rail item rests ON the card's top border: top/left/right borders and
    // rounded top corners, but no bottom border and no negative margin, so the
    // card's border — and the context-usage fill riding it — stays uncovered.
    expect(tabs.className).toContain('border-b-0');
    expect(tabs.className).toContain('rounded-t-md');
    expect(tabs.className).not.toMatch(/(^|\s|:)-m[btxy]?-/);
    // Nothing on the rail is absolutely positioned: the rail is measured with
    // the rest of the bottom overlay only while it stays in normal flow.
    expect(tabs.className).not.toContain('absolute');
    expect(rail.className).not.toContain('absolute');

    // The selected tab is marked by fill/color/weight alone — it does not draw
    // over the card's top border to "merge" with the card.
    const claude = screen.getByTestId('provider-option-claude');
    const codex = screen.getByTestId('provider-option-codex');
    expect(claude.className).toMatch(/(^|\s)bg-surface(\s|$)/);
    expect(claude.className).toContain('font-medium');
    expect(codex.className).toContain('bg-surface-elevated');
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

    // The disabled tab also stays flat under the pointer: a hover lift would
    // advertise a click the disabled radio refuses. The available one keeps it.
    expect(screen.getByTestId('provider-option-codex').className).not.toContain(
      'hover:',
    );
    expect(screen.getByTestId('provider-option-claude').className).toContain(
      'cursor-pointer',
    );
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
