import { useEffect, useMemo } from 'react';
import { ProviderDot, cn } from '@delta/ui-kit';
import type { AgentProvider, ProviderAvailability } from '@delta/wire-gen';
import { useProvidersQuery } from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { useComposerStore } from '../../store/composerStore';
import { useSettingsStore } from '../../store/settingsStore';
import { PROVIDER_OPTIONS } from '../../providers';

/**
 * The top-level axis of the new-session form: which AI-agent provider the next
 * session launches on. It sits above the working-directory and launch-option
 * controls because the choice changes the backend binary and (in later slices)
 * gates capability-dependent controls beneath it.
 *
 * Rendered as a segmented radio group — one native radio per provider, styled
 * as adjacent segments — so it stays keyboard-navigable and screen readers
 * announce the role correctly, matching the radio-group pattern used elsewhere
 * (Settings' appearance picker). The selection writes to
 * `composerStore.newSessionProvider`; the composer attaches it to the
 * new-session send (omitting it for the Claude default).
 *
 * The initial selection is seeded from the persisted default-provider setting
 * (`settingsStore.defaultProvider`) once, when a fresh new-session compose is
 * entered — this component mounts only in the new-session state. The seed only
 * ever supplies the initial value: an explicit pick (which marks the selection
 * seeded) is preserved, never re-seeded, even if the default later changes
 * mid-compose. The seed guard resets when the new-session compose state is
 * left (see {@link resetNewSessionProvider}).
 *
 * Provider availability (`GET /api/providers`) gates the control: a provider
 * whose launch binary is missing on the server host is disabled with the
 * server's reason, so a user cannot pick a provider that would fail at spawn.
 * The gate is fail-open — until availability is known (loading, or the query
 * failed) every provider stays selectable, so a transient error never wrongly
 * locks a user out of starting a session. If the persisted default (or an
 * already-seeded selection) turns out to be unavailable, the selection falls
 * back to the first available provider.
 */
export function ProviderSelector() {
  const provider = useComposerStore((state) => state.newSessionProvider);
  const setProvider = useComposerStore((state) => state.setNewSessionProvider);
  const seedProvider = useComposerStore(
    (state) => state.seedNewSessionProvider,
  );
  const defaultProvider = useSettingsStore((state) => state.defaultProvider);

  const client = useApiClient();
  const { data: providersData } = useProvidersQuery(client);

  const availabilityByProvider = useMemo(() => {
    const map = new Map<AgentProvider, ProviderAvailability>();
    for (const entry of providersData?.providers ?? []) {
      map.set(entry.provider, entry);
    }
    return map;
  }, [providersData]);

  // Fail-open: an unknown verdict (still loading, or the query failed) counts as
  // available so the selector never wrongly disables a provider on a transient
  // error.
  const isAvailable = (value: AgentProvider): boolean =>
    availabilityByProvider.get(value)?.available ?? true;

  const firstAvailableProvider = (): AgentProvider | null =>
    PROVIDER_OPTIONS.find((option) => isAvailable(option.value))?.value ?? null;

  // Seed the initial provider from the persisted default the first time a fresh
  // new-session compose renders — but never seed onto an unavailable provider:
  // if the default cannot launch, seed the first available one instead so the
  // form never opens on a provider that would fail at spawn.
  // `seedNewSessionProvider` is a no-op once the selection has been seeded or
  // the user has picked one, so this never clobbers an explicit choice; it runs
  // again only after a reset (re)enters new-session compose. Effect (not render)
  // so it does not set store state during render.
  useEffect(() => {
    const seedValue = isAvailable(defaultProvider)
      ? defaultProvider
      : (firstAvailableProvider() ?? defaultProvider);
    seedProvider(seedValue);
    // `isAvailable`/`firstAvailableProvider` close over `providersData`; it is
    // in the dep list so the seed reconsiders once availability lands.
  }, [defaultProvider, seedProvider, providersData]);

  // If availability arrives after the selection was already seeded (e.g. a
  // persisted default seeded onto a now-unavailable provider before the verdict
  // landed), move off the unavailable provider onto an available one. A disabled
  // option can never be picked, so this only corrects a stale/seeded-before-load
  // selection, never fights an explicit pick of an available provider.
  useEffect(() => {
    if (!providersData) return;
    if (isAvailable(provider)) return;
    const fallback = firstAvailableProvider();
    if (fallback && fallback !== provider) {
      setProvider(fallback);
    }
  }, [providersData, provider, setProvider]);

  const unavailableNotices = PROVIDER_OPTIONS.filter(
    (option) => !isAvailable(option.value),
  ).map((option) => ({
    value: option.value,
    label: option.label,
    detail:
      availabilityByProvider.get(option.value)?.detail ??
      'This provider is not available on this host.',
  }));

  return (
    <section data-testid="provider-selector">
      <div
        role="radiogroup"
        aria-labelledby="provider-selector-heading"
        className="flex gap-1 rounded border border-border-default bg-surface-elevated p-1"
      >
        <span id="provider-selector-heading" className="sr-only">
          Session provider
        </span>
        {PROVIDER_OPTIONS.map((option) => {
          const available = isAvailable(option.value);
          const detail = availabilityByProvider.get(option.value)?.detail;
          const selected = provider === option.value && available;
          return (
            <label
              key={option.value}
              className={cn(
                'flex flex-1 items-center justify-center gap-2 rounded px-3 py-1.5 text-secondary transition',
                available ? 'cursor-pointer' : 'cursor-not-allowed opacity-50',
                selected
                  ? 'bg-accent/10 text-fg ring-1 ring-accent/30'
                  : available
                    ? 'text-fg-muted hover:bg-surface'
                    : 'text-fg-muted',
              )}
              data-testid={`provider-option-${option.value}`}
              aria-disabled={!available}
              title={!available && detail ? detail : undefined}
            >
              {/* The radio itself is visually hidden — the segment's highlight
                  conveys the selection — but kept in the DOM so the control is
                  focusable and announced as a radio. A disabled radio is skipped
                  by keyboard navigation and announced as unavailable. */}
              <input
                type="radio"
                name="new-session-provider"
                value={option.value}
                checked={selected}
                disabled={!available}
                onChange={() => setProvider(option.value)}
                className="sr-only"
              />
              <ProviderDot provider={option.value} />
              <span className="font-medium">{option.label}</span>
            </label>
          );
        })}
      </div>
      {unavailableNotices.length > 0 && (
        <div data-testid="provider-unavailable-notice" className="mt-1 space-y-0.5">
          {unavailableNotices.map((notice) => (
            <p
              key={notice.value}
              data-testid={`provider-unavailable-${notice.value}`}
              role="note"
              className="text-caption text-fg-muted"
            >
              <span className="font-medium">{notice.label}:</span> {notice.detail}
            </p>
          ))}
        </div>
      )}
    </section>
  );
}
