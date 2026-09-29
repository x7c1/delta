import type {
  AgentProvider,
  RateLimitWindow,
  SessionListItem,
} from '@delta/wire-gen';
import { PROVIDER_OPTIONS } from '../../providers';
import { NEW_SESSION_FOCUS, type FocusedSession } from '../../store/navStore';
import type {
  RateLimitsByProvider,
  RateLimitsObservedAt,
} from '../../store/statusTypes';

/**
 * One provider's account meters as the navigator footer renders them: the
 * windows the account reported, and — while those windows are still a
 * restored guess rather than a live reading — when they were observed.
 */
export interface FooterRateLimitGroup {
  provider: AgentProvider;
  /** Never empty: a provider with no windows gets no group at all. */
  windows: RateLimitWindow[];
  /**
   * Non-null only while this provider's windows are a restored guess. The
   * whole group shares one observation, since a provider's windows arrive
   * (and are persisted) together.
   */
  restoredObservedAt: number | null;
  /**
   * Whether the group carries its provider-name heading. True on the
   * new-session screen, where several providers' groups sit side by side
   * before the user has picked one, and rows are labelled by duration only —
   * two providers can report windows of the same length. False in a focused
   * thread, where the heading would be redundant: the user already knows
   * which provider that session runs on.
   */
  showHeading: boolean;
}

/**
 * Which providers' account meters the navigator footer lists, and in what
 * order.
 *
 * - A focused thread lists exactly its session's provider. Rate limits belong
 *   to an account, so showing another provider's windows under this session
 *   would read as a statement about the wrong account.
 * - The new-session screen lists every provider that has reported a non-empty
 *   window list, in the fixed {@link PROVIDER_OPTIONS} order (never arrival
 *   order, so the groups do not swap places as snapshots land). This is the
 *   moment the user picks which provider to start on, and how much budget
 *   each account has left is part of that choice — so no single provider may
 *   stand in for the rest.
 * - With nothing focused there is no account to speak for, so it lists
 *   nothing.
 *
 * Only the new-session groups carry a provider-name heading (see
 * {@link FooterRateLimitGroup.showHeading}): it is needed where the provider
 * is not yet known and redundant where it is.
 *
 * A provider with no entry or an empty list never gets a group, so a user who
 * has never used a provider sees no meters for it — the same "no empty bars"
 * rule a single provider's rows follow.
 */
export function footerRateLimitGroups(
  sessions: readonly SessionListItem[],
  focusedSessionId: FocusedSession,
  rateLimitsByProvider: RateLimitsByProvider,
  restoredRateLimitsObservedAt: RateLimitsObservedAt,
): FooterRateLimitGroup[] {
  const newSession = focusedSessionId === NEW_SESSION_FOCUS;
  const providers: readonly AgentProvider[] = newSession
    ? PROVIDER_OPTIONS.map((option) => option.value)
    : focusedProvider(sessions, focusedSessionId);
  return providers.flatMap((provider) => {
    const windows = rateLimitsByProvider[provider] ?? [];
    if (windows.length === 0) {
      return [];
    }
    return [
      {
        provider,
        windows,
        restoredObservedAt: restoredRateLimitsObservedAt[provider] ?? null,
        showHeading: newSession,
      },
    ];
  });
}

function focusedProvider(
  sessions: readonly SessionListItem[],
  focusedSessionId: FocusedSession,
): AgentProvider[] {
  if (focusedSessionId === null) {
    return [];
  }
  const provider = sessions.find(
    (item) => item.session.id === focusedSessionId,
  )?.session.provider;
  return provider ? [provider] : [];
}
