import { useEffect, useMemo, useRef } from 'react';
import { useLaunchOptionsQuery } from '@delta/api-client';
import type { AgentProvider } from '@delta/wire-gen';
import { useApiClient } from '../../data/apiContext';
import { DangerousBadge } from '../../launchOptions';
import { useComposerStore } from '../../store/composerStore';

/**
 * The new-session launch-option picker shown above the composer: a checklist of
 * the registered launch options (managed in Settings) the user can apply to the
 * next session's launch. Selecting options writes their ids — in click order —
 * to `composerStore.newSessionLaunchOptionIds`; the composer attaches them as
 * `launch_option_ids` on the new-session send.
 *
 * Launch options are registered per provider (Claude's argv flags mean nothing
 * to Codex and vice-versa), so the picker only offers the options whose
 * `provider` matches the new session's selected provider
 * (`composerStore.newSessionProvider`, chosen in the provider selector above).
 *
 * Selection is optional (unlike the mandatory working directory), so this is an
 * inline panel rather than a blocking dialog. It renders nothing until the
 * registry has at least one option for the selected provider, so a user who
 * never registered any (or is on a provider with none) sees no extra chrome.
 *
 * The initial selection is seeded from the selected provider's `default_enabled`
 * options, once, the first time the registry loads for a fresh new-session
 * compose state (tracked by `composerStore.newSessionLaunchOptionsSeeded`). The
 * seed only ever supplies the initial value: an in-place uncheck — even
 * unchecking every option — is preserved, never re-seeded. The failed-spawn
 * Retry path restores its own preserved selection directly (it does not flow
 * through this store field), so it is unaffected.
 *
 * When the user switches provider mid-compose the picker re-filters and resets
 * the selection to the new provider's `default_enabled` options — dropping any
 * selection that belonged to the previous provider, so a send never carries an
 * option id from a different provider.
 *
 * An option the server flags `dangerous` — one that switches the agent's own
 * safety mechanism off — is treated differently in two ways. It is **never**
 * seeded, even if its stored row still says `default_enabled` (the server
 * refuses to set that now, but a row registered before the rule can carry it),
 * so a safety bypass is never pre-checked. And ticking one reveals an inline
 * warning naming it: it stays selectable, it just never happens quietly.
 */
export function LaunchOptionsPicker() {
  const client = useApiClient();
  const query = useLaunchOptionsQuery(client, true);
  const provider = useComposerStore((state) => state.newSessionProvider);
  const selected = useComposerStore((state) => state.newSessionLaunchOptionIds);
  const setSelected = useComposerStore(
    (state) => state.setNewSessionLaunchOptionIds,
  );
  const seedSelected = useComposerStore(
    (state) => state.seedNewSessionLaunchOptionIds,
  );

  const options = query.data?.launch_options ?? [];

  // Only the selected provider's options are offered; the picker filters
  // client-side (the list endpoint returns every provider's options).
  const providerOptions = useMemo(
    () => options.filter((o) => o.provider === provider),
    [options, provider],
  );

  // Dangerous options are filtered out rather than trusted to be undefaulted:
  // the server refuses to *set* `default_enabled` on one, but a row stored
  // before that rule can still carry it.
  const defaultEnabledIds = useMemo(
    () =>
      providerOptions
        .filter((o) => o.default_enabled && !o.dangerous)
        .map((o) => o.id),
    [providerOptions],
  );

  // The dangerous options the user has actually ticked, in list order, so the
  // warning below can name them.
  const selectedDangerous = providerOptions.filter(
    (o) => o.dangerous && selected.includes(o.id),
  );

  // Seed the initial selection from the selected provider's `default_enabled`
  // options the first time the registry loads. `seedNewSessionLaunchOptionIds`
  // is a no-op once the selection has been seeded or the user has touched it, so
  // this never clobbers an explicit choice; it runs again only after a reset
  // (re)enters new-session compose. Effect (not render) so it does not set store
  // state during render.
  useEffect(() => {
    if (options.length === 0) {
      return;
    }
    seedSelected(defaultEnabledIds);
  }, [options.length, defaultEnabledIds, seedSelected]);

  // On a provider switch mid-compose, reset the selection to the new provider's
  // `default_enabled` options. This both drops any ids selected under the
  // previous provider (so a send never mixes providers) and re-seeds the new
  // provider's defaults. The ref lets us skip the initial render (where there is
  // no previous provider to switch away from), preserving a restored/seeded
  // selection. Guarded on options being loaded so a switch that lands before the
  // registry does is reconciled once the options arrive.
  const prevProviderRef = useRef<AgentProvider | null>(null);
  useEffect(() => {
    if (options.length === 0) {
      return;
    }
    const prev = prevProviderRef.current;
    prevProviderRef.current = provider;
    if (prev === null || prev === provider) {
      return;
    }
    setSelected(defaultEnabledIds);
  }, [provider, options.length, defaultEnabledIds, setSelected]);

  if (providerOptions.length === 0) {
    return null;
  }

  // Append on select, drop on deselect — keeping the array in click order so
  // the resulting argv follows the order the user picked the flags in.
  const toggle = (id: number) => {
    setSelected(
      selected.includes(id)
        ? selected.filter((each) => each !== id)
        : [...selected, id],
    );
  };

  return (
    <section
      className="space-y-1 rounded border border-border-default bg-surface-elevated px-2 py-1.5 text-caption"
      data-testid="launch-options-picker"
    >
      <h3 className="font-semibold uppercase tracking-wide text-fg-muted">
        Launch options
      </h3>
      <ul className="space-y-0.5">
        {providerOptions.map((option) => (
          <li key={option.id}>
            <label
              className="flex cursor-pointer items-center gap-2 rounded px-1 py-0.5 hover:bg-surface-elevated-hover"
              title={
                option.value === null
                  ? option.name
                  : `${option.name} ${option.value}`
              }
            >
              <input
                type="checkbox"
                checked={selected.includes(option.id)}
                onChange={() => toggle(option.id)}
                data-testid={`launch-option-${option.id}`}
              />
              {option.label && (
                <span className="font-medium text-fg">
                  {option.label}
                </span>
              )}
              <span className="min-w-0 truncate font-mono text-code text-fg-muted">
                {option.name}
                {option.value !== null && (
                  <span className="text-fg-subtle"> {option.value}</span>
                )}
              </span>
              {/* Marked in the picker too, not just in Settings: this is where
                  the option is actually applied to a session. */}
              {option.dangerous && <DangerousBadge />}
            </label>
          </li>
        ))}
      </ul>
      {selectedDangerous.length > 0 && (
        // Revealed on selection rather than shown always, and inline rather
        // than as a blocking dialog: selecting a launch option is not a
        // confirmable act, so the warning belongs beside the checkbox that
        // caused it. `role="alert"` so a screen reader hears it the moment it
        // appears.
        <p role="alert" className="text-caption text-warning">
          {selectedDangerous
            .map((option) => option.label ?? option.name)
            .join(', ')}{' '}
          {selectedDangerous.length === 1 ? 'turns off' : 'turn off'} the agent's
          own safety mechanism for this session: it will act without asking for
          permission.
        </p>
      )}
    </section>
  );
}
