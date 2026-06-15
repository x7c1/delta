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
 */
export function LaunchOptionsPicker() {
  const client = useApiClient();
  const query = useLaunchOptionsQuery(client, true);
  const selected = useComposerStore((state) => state.newSessionLaunchOptionIds);
  const setSelected = useComposerStore(
    (state) => state.setNewSessionLaunchOptionIds,
  );

  const options = query.data?.launch_options ?? [];
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
      className="space-y-1 rounded border border-slate-200 bg-slate-50 px-2 py-1.5 text-xs"
      data-testid="launch-options-picker"
    >
      <h3 className="font-semibold uppercase tracking-wide text-slate-500">
        Launch options
      </h3>
      <ul className="space-y-0.5">
        {options.map((option) => (
          <li key={option.id}>
            <label
              className="flex cursor-pointer items-center gap-2 rounded px-1 py-0.5 hover:bg-slate-100"
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
                <span className="font-medium text-slate-700">
                  {option.label}
                </span>
              )}
              <span className="min-w-0 truncate font-mono text-slate-600">
                {option.name}
                {option.value !== null && (
                  <span className="text-slate-400"> {option.value}</span>
                )}
              </span>
            </label>
          </li>
        ))}
      </ul>
    </section>
  );
}
