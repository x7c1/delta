import { useEffect } from 'react';
import { ProviderName, cn } from '@delta/ui-kit';
import { useComposerStore } from '../../store/composerStore';
import { useSettingsStore } from '../../store/settingsStore';
import { PROVIDER_OPTIONS } from '../../providers';
import { COMPOSER_RAIL_ITEM_CLASS } from './ComposerRail';
import { useProviderAvailability } from './providerAvailability';

/**
 * The top-level axis of the new-session form: which AI-agent provider the next
 * session launches on. It rides the composer rail — the strip on the composer
 * card's top edge — as a pair of tabs resting on the card's top border, so the
 * choice reads as "which card is this", above and outside the controls it
 * parameterizes, without costing a row inside the card.
 *
 * Rendered as a radio group — one native radio per provider, visually hidden
 * behind its tab — so it stays keyboard-navigable and screen readers announce
 * the role correctly, matching the radio-group pattern used elsewhere
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
 * Provider availability gates the control: a provider whose launch binary is
 * missing on the server host is disabled with the server's reason, so a user
 * cannot pick a provider that would fail at spawn. The verdicts come from
 * {@link useProviderAvailability} (fail-open until they land); the reasons
 * themselves are too long for a tab, so they are spelled out by
 * {@link ProviderUnavailableNotice} inside the card. This component owns the
 * selection policy that acts on those verdicts.
 */
export function ProviderTabs() {
  const provider = useComposerStore((state) => state.newSessionProvider);
  const setProvider = useComposerStore((state) => state.setNewSessionProvider);
  const seedProvider = useComposerStore(
    (state) => state.seedNewSessionProvider,
  );
  const defaultProvider = useSettingsStore((state) => state.defaultProvider);

  const { verdicts, isAvailable, firstAvailable, detailOf } =
    useProviderAvailability();

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
      : (firstAvailable() ?? defaultProvider);
    seedProvider(seedValue);
    // `isAvailable`/`firstAvailable` close over the verdicts; `verdicts` is in
    // the dep list so the seed reconsiders once availability lands.
  }, [defaultProvider, seedProvider, verdicts]);

  // If availability arrives after the selection was already seeded (e.g. a
  // persisted default seeded onto a now-unavailable provider before the verdict
  // landed), move off the unavailable provider onto an available one. A disabled
  // option can never be picked, so this only corrects a stale/seeded-before-load
  // selection, never fights an explicit pick of an available provider.
  useEffect(() => {
    if (!verdicts) return;
    if (isAvailable(provider)) return;
    const fallback = firstAvailable();
    if (fallback && fallback !== provider) {
      setProvider(fallback);
    }
  }, [verdicts, provider, setProvider]);

  return (
    <div
      // One rail item holding both tabs, so the pair reads as a single strip of
      // adjacent tabs (an internal divider separates them) rather than two
      // detached boxes. `overflow-hidden` keeps the tab fills inside the item's
      // rounded top corners.
      //
      // This item is the one rail item allowed to reach 1px DOWN onto the card's
      // top border (`-mb-px`, painted above the card via `relative z-10`): the
      // selected tab "opens" into the card by drawing no bottom border there,
      // while the unselected tabs redraw the border line themselves, so the
      // open/closed distinction is structural rather than a subtle fill
      // difference. Safe only because the tabs exist in new-session mode alone,
      // where the context-usage fill (which rides that same border in a thread)
      // can never be present.
      className={cn(
        COMPOSER_RAIL_ITEM_CLASS,
        'relative z-10 -mb-px flex overflow-hidden',
      )}
      role="radiogroup"
      aria-labelledby="provider-selector-heading"
      data-testid="provider-selector"
    >
      <span id="provider-selector-heading" className="sr-only">
        Session provider
      </span>
      {PROVIDER_OPTIONS.map((option, index) => {
        const available = isAvailable(option.value);
        const detail = detailOf(option.value);
        const selected = provider === option.value && available;
        return (
          <label
            key={option.value}
            className={cn(
              'flex items-center px-3 py-1 text-secondary leading-none transition',
              // The tabs butt against each other inside the shared item, so the
              // seam between them is drawn here rather than by a gap.
              index > 0 && 'border-l border-border-default',
              available ? 'cursor-pointer' : 'cursor-not-allowed opacity-50',
              // The selected tab is the "open" one: it shares the card's fill
              // and draws no bottom border, so the card's top border disappears
              // beneath it and the tab merges with the card. Each unselected
              // tab is "closed": a sunken fill plus its own bottom border that
              // continues the card's line under it.
              selected
                ? 'bg-surface text-fg font-medium'
                : 'border-b border-border-default bg-surface-elevated text-fg-muted',
              // Only a tab that can actually be picked lifts on hover. An
              // unavailable provider stays flat: brightening it would advertise
              // a click that its disabled radio refuses.
              !selected && available && 'hover:text-fg',
            )}
            data-testid={`provider-option-${option.value}`}
            aria-disabled={!available}
            title={!available && detail ? detail : undefined}
          >
            {/* The radio itself is visually hidden — the tab's fill conveys the
                selection — but kept in the DOM so the control is focusable and
                announced as a radio. A disabled radio is skipped by keyboard
                navigation and announced as unavailable. */}
            <input
              type="radio"
              name="new-session-provider"
              value={option.value}
              checked={selected}
              disabled={!available}
              onChange={() => setProvider(option.value)}
              className="sr-only"
            />
            <ProviderName provider={option.value} />
          </label>
        );
      })}
    </div>
  );
}
