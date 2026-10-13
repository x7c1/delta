import { cn, ProviderName } from '@delta/ui-kit';
import { useSettingsStore } from '../../store/settingsStore';
import { PROVIDER_OPTIONS } from '../../providers';

/**
 * Default provider category content: pick which AI-agent provider a new session
 * starts on. The choice is a persisted preference
 * ({@link useSettingsStore}'s `defaultProvider`) that seeds the new-session
 * provider selector's initial value; each session can still override it there.
 * It also seeds which provider {@link LaunchOptionsSection} opens scoped to.
 *
 * The control is a radio group so each option is independently focusable for
 * keyboard navigation and screen readers announce the role correctly, following
 * the same pattern as the Appearance picker and the new-session selector.
 */
export function DefaultProviderSection() {
  const defaultProvider = useSettingsStore((state) => state.defaultProvider);
  const setDefaultProvider = useSettingsStore(
    (state) => state.setDefaultProvider,
  );

  return (
    <section className="w-full" data-testid="default-provider-section">
      <h3 className="mb-1 text-secondary font-semibold text-fg">
        Default provider
      </h3>
      <p className="mb-4 text-caption text-fg-muted">
        Choose which AI-agent provider a new session starts on. This seeds the
        provider selector when you start a session; you can still switch it for
        an individual session.
      </p>
      <div
        role="radiogroup"
        aria-labelledby="default-provider-section-heading"
        className="flex flex-col gap-2 rounded-lg border border-border-default bg-surface-elevated p-3"
        data-testid="default-provider-options"
      >
        <span id="default-provider-section-heading" className="sr-only">
          Default provider
        </span>
        {PROVIDER_OPTIONS.map((option) => {
          const selected = defaultProvider === option.value;
          return (
            <label
              key={option.value}
              className={cn(
                'flex cursor-pointer items-center gap-3 rounded border px-3 py-2 text-secondary transition',
                selected
                  ? 'border-accent bg-accent/10 text-fg ring-1 ring-accent/30'
                  : 'border-border-default text-fg hover:bg-surface',
              )}
              data-testid={`default-provider-option-${option.value}`}
            >
              <input
                type="radio"
                name="default-provider"
                value={option.value}
                checked={selected}
                onChange={() => setDefaultProvider(option.value)}
                className="h-3.5 w-3.5 accent-accent"
              />
              <span className="flex flex-1 flex-col">
                <ProviderName provider={option.value} className="font-medium" />
                <span className="text-caption text-fg-muted">{option.hint}</span>
              </span>
            </label>
          );
        })}
      </div>
    </section>
  );
}
