import { ProviderBadge, cn } from '@delta/ui-kit';
import type { AgentProvider } from '@delta/wire-gen';
import { useComposerStore } from '../../store/composerStore';

/**
 * The AI-agent providers a new session can launch on, in display order, with
 * the full product name shown beside the shared {@link ProviderBadge}. The
 * badge already carries the accent hue and the accessible name; the label here
 * spells the name out so the segmented control reads at a glance.
 */
const PROVIDER_OPTIONS: { value: AgentProvider; label: string }[] = [
  { value: 'claude', label: 'Claude Code' },
  { value: 'codex', label: 'Codex' },
];

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
 */
export function ProviderSelector() {
  const provider = useComposerStore((state) => state.newSessionProvider);
  const setProvider = useComposerStore((state) => state.setNewSessionProvider);

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
          const selected = provider === option.value;
          return (
            <label
              key={option.value}
              className={cn(
                'flex flex-1 cursor-pointer items-center justify-center gap-2 rounded px-3 py-1.5 text-secondary transition',
                selected
                  ? 'bg-accent/10 text-fg ring-1 ring-accent/30'
                  : 'text-fg-muted hover:bg-surface',
              )}
              data-testid={`provider-option-${option.value}`}
            >
              {/* The radio itself is visually hidden — the segment's highlight
                  conveys the selection — but kept in the DOM so the control is
                  focusable and announced as a radio. */}
              <input
                type="radio"
                name="new-session-provider"
                value={option.value}
                checked={selected}
                onChange={() => setProvider(option.value)}
                className="sr-only"
              />
              <ProviderBadge provider={option.value} />
              <span className="font-medium">{option.label}</span>
            </label>
          );
        })}
      </div>
    </section>
  );
}
