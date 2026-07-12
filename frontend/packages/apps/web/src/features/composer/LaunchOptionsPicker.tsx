import { useEffect } from 'react';
import { useLaunchOptionsQuery } from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { useComposerStore } from '../../store/composerStore';

/**
 * The new-session launch-option picker shown above the composer: a checklist of
 * the registered launch options (managed in Settings) the user can apply to the
 * next session's `claude` launch. Selecting options writes their ids — in click
 * order — to `composerStore.newSessionLaunchOptionIds`; the composer attaches
 * them as `launch_option_ids` on the new-session send.
 *
 * Selection is optional (unlike the mandatory working directory), so this is an
 * inline panel rather than a blocking dialog. It renders nothing until the
 * registry has at least one option, so a user who never registered any sees no
 * extra chrome.
 *
 * The initial selection is seeded from the options marked `default_enabled`,
 * once, the first time the registry loads for a fresh new-session compose state
 * (tracked by `composerStore.newSessionLaunchOptionsSeeded`). The seed only ever
 * supplies the initial value: an in-place uncheck — even unchecking every
 * option — is preserved, never re-seeded. The failed-spawn Retry path restores
 * its own preserved selection directly (it does not flow through this store
 * field), so it is unaffected.
 */
export function LaunchOptionsPicker() {
  const client = useApiClient();
  const query = useLaunchOptionsQuery(client, true);
  const selected = useComposerStore((state) => state.newSessionLaunchOptionIds);
  const setSelected = useComposerStore(
    (state) => state.setNewSessionLaunchOptionIds,
  );
  const seedSelected = useComposerStore(
    (state) => state.seedNewSessionLaunchOptionIds,
  );

  const options = query.data?.launch_options ?? [];

  // Seed the initial selection from the `default_enabled` options the first time
  // the registry loads. `seedNewSessionLaunchOptionIds` is a no-op once the
  // selection has been seeded or the user has touched it, so this never clobbers
  // an explicit choice; it runs again only after a reset (re)enters new-session
  // compose. Effect (not render) so it does not set store state during render.
  useEffect(() => {
    if (options.length === 0) {
      return;
    }
    seedSelected(options.filter((o) => o.default_enabled).map((o) => o.id));
  }, [options, seedSelected]);

  if (options.length === 0) {
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
        {options.map((option) => (
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
            </label>
          </li>
        ))}
      </ul>
    </section>
  );
}
