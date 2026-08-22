import { useMemo } from 'react';
import type { AgentProvider, ProviderAvailability } from '@delta/wire-gen';
import { useProvidersQuery } from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { PROVIDER_OPTIONS } from '../../providers';

/** One provider the server reported as unable to launch, with its reason. */
export interface UnavailableProvider {
  value: AgentProvider;
  label: string;
  /** The server's explanation, or a generic fallback when it sent none. */
  detail: string;
}

/** The availability verdicts, read the way the new-session controls need them. */
export interface ProviderAvailabilityView {
  /**
   * The raw verdict list as it arrived, or `undefined` until it lands. Read for
   * two things, never for the verdicts themselves (use {@link isAvailable}):
   * as the "have the verdicts landed yet" guard, and as an effect dependency —
   * an effect that closes over {@link isAvailable} lists this so it reconsiders
   * once availability lands or changes on a refetch, which a plain "loaded"
   * boolean would miss.
   */
  verdicts: ProviderAvailability[] | undefined;
  /**
   * Fail-open: an unknown verdict (still loading, or the query failed) counts
   * as available, so a transient error never wrongly locks a user out of
   * starting a session.
   */
  isAvailable: (value: AgentProvider) => boolean;
  /** The first provider in display order that can launch, if any. */
  firstAvailable: () => AgentProvider | null;
  /** The server's reason for a provider, or `undefined` when it gave none. */
  detailOf: (value: AgentProvider) => string | undefined;
  /** Every provider that cannot launch, in display order, with its reason. */
  unavailable: UnavailableProvider[];
}

/**
 * Provider availability (`GET /api/providers`) as the new-session controls read
 * it: which providers can launch on the server host, and why the others cannot.
 *
 * Shared by the two halves of the provider control, which live in different
 * places on screen — the tabs on the composer rail and the explanatory notice
 * inside the card — so both read one verdict set rather than each deriving its
 * own. The hook is pure: it never writes the selection, so mounting it twice
 * costs nothing beyond the (deduplicated) query subscription. The selection
 * policy that acts on these verdicts lives in the tabs alone.
 */
export function useProviderAvailability(): ProviderAvailabilityView {
  const client = useApiClient();
  const { data: providersData } = useProvidersQuery(client);

  return useMemo(() => {
    const byProvider = new Map<AgentProvider, ProviderAvailability>();
    for (const entry of providersData?.providers ?? []) {
      byProvider.set(entry.provider, entry);
    }
    const isAvailable = (value: AgentProvider): boolean =>
      byProvider.get(value)?.available ?? true;
    return {
      verdicts: providersData?.providers,
      isAvailable,
      firstAvailable: () =>
        PROVIDER_OPTIONS.find((option) => isAvailable(option.value))?.value ??
        null,
      detailOf: (value: AgentProvider) =>
        byProvider.get(value)?.detail ?? undefined,
      unavailable: PROVIDER_OPTIONS.filter(
        (option) => !isAvailable(option.value),
      ).map((option) => ({
        value: option.value,
        label: option.label,
        detail:
          byProvider.get(option.value)?.detail ??
          'This provider is not available on this host.',
      })),
    };
  }, [providersData]);
}
