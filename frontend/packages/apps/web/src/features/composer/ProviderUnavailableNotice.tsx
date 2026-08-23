import { useProviderAvailability } from './providerAvailability';

/**
 * Why a provider tab is disabled, spelled out in the server's own words.
 *
 * The other half of the new-session provider control: {@link ProviderTabs} does
 * the choosing from the composer rail, but a rail tab has room for a name and
 * nothing else, so the reason lives here — inside the composer card, at the top
 * of its stack, directly under the tab it explains. Renders nothing when every
 * provider can launch, and (fail-open, like the tabs) while the verdicts are
 * still unknown.
 */
export function ProviderUnavailableNotice() {
  const { unavailable } = useProviderAvailability();
  if (unavailable.length === 0) return null;

  return (
    <div data-testid="provider-unavailable-notice" className="space-y-0.5">
      {unavailable.map((notice) => (
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
  );
}
