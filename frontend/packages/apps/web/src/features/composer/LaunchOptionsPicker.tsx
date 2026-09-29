import { type ReactNode, useEffect, useId, useMemo, useRef } from 'react';
import { useLaunchOptionsQuery } from '@delta/api-client';
import type { AgentProvider, LaunchOption } from '@delta/wire-gen';
import { useApiClient } from '../../data/apiContext';
import { DangerousBadge, partitionByChoiceGroup } from '../../launchOptions';
import { useComposerStore } from '../../store/composerStore';

/**
 * The new-session launch-option picker shown above the composer: the registered
 * launch options (managed in Settings) the user can apply to the next session's
 * launch. Selecting options writes their ids — in click order — to
 * `composerStore.newSessionLaunchOptionIds`; the composer attaches them as
 * `launch_option_ids` on the new-session send.
 *
 * Launch options are registered per provider (Claude's argv flags mean nothing
 * to Codex and vice-versa), so the picker only offers the options whose
 * `provider` matches the new session's selected provider
 * (`composerStore.newSessionProvider`, chosen in the provider selector above).
 *
 * Rows the server puts in one **choice group** (`choice_group`, e.g. every
 * Claude `--model` row) are values of one single-valued setting, so a session
 * takes at most one of them: each group renders as a radio group headed by its
 * key, with an explicit first "Agent default" option meaning "none of these" —
 * the agent's own default is a legitimate choice. Picking a row replaces any
 * sibling in the selection. Rows are grouped by `choice_group` and never by
 * `name`, so the grouping rule stays on the server. A group holding a single
 * row renders as a plain checkbox, like an ungrouped row: exclusivity only
 * shows once a second row joins, and a radio with one real option is worse
 * than a checkbox (see `partitionByChoiceGroup`). Every other row is an
 * independent checkbox. Groups and ungrouped rows interleave by the list
 * position of their first row; rows inside a group keep list order.
 *
 * Selection is optional (unlike the mandatory working directory), so this is an
 * inline panel rather than a blocking dialog. It renders nothing until the
 * registry has at least one option for the selected provider, so a user who
 * never registered any (or is on a provider with none) sees no extra chrome.
 *
 * The initial selection is seeded from the selected provider's `default_enabled`
 * options, once, the first time the registry loads for a fresh new-session
 * compose state (tracked by `composerStore.newSessionLaunchOptionsSeeded`).
 * Within a choice group only the **first** eligible default in list order is
 * seeded: the server refuses a second default in a group now, but rows stored
 * before that rule can both carry one. The seed only ever supplies the initial
 * value: an in-place change — even clearing every option — is preserved, never
 * re-seeded. The failed-spawn Retry path restores its own preserved selection
 * directly (it does not flow through this store field), so it is unaffected.
 *
 * When the user switches provider mid-compose the picker re-filters and resets
 * the selection to the new provider's seeded defaults — dropping any selection
 * that belonged to the previous provider, so a send never carries an option id
 * from a different provider.
 *
 * An option the server flags `dangerous` — one that switches the agent's own
 * safety mechanism off — is treated differently in two ways. It is **never**
 * seeded, even if its stored row still says `default_enabled` (the server
 * refuses to set that now, but a row registered before the rule can carry it),
 * so a safety bypass is never pre-checked. And selecting one reveals an inline
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

  const entries = useMemo(
    () => partitionByChoiceGroup(providerOptions),
    [providerOptions],
  );

  const defaultEnabledIds = useMemo(
    () => seedableDefaultIds(providerOptions),
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

  // Choosing within a group drops its siblings first, so at most one of the
  // group's rows is ever selected; `null` is "Agent default" (none of them).
  // The rest of the selection keeps its click order.
  const choose = (group: LaunchOption[], id: number | null) => {
    const groupIds = new Set(group.map((option) => option.id));
    const rest = selected.filter((each) => !groupIds.has(each));
    setSelected(id === null ? rest : [...rest, id]);
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
        {entries.map((entry) =>
          entry.kind === 'group' ? (
            <li key={`group:${entry.key}`}>
              <ChoiceGroup
                provider={provider}
                groupKey={entry.key}
                options={entry.options}
                selected={selected}
                onChoose={(id) => choose(entry.options, id)}
              />
            </li>
          ) : (
            <li key={entry.option.id}>
              <OptionLabel option={entry.option}>
                <input
                  type="checkbox"
                  checked={selected.includes(entry.option.id)}
                  onChange={() => toggle(entry.option.id)}
                  data-testid={`launch-option-${entry.option.id}`}
                />
              </OptionLabel>
            </li>
          ),
        )}
      </ul>
      {selectedDangerous.length > 0 && (
        // Revealed on selection rather than shown always, and inline rather
        // than as a blocking dialog: selecting a launch option is not a
        // confirmable act, so the warning belongs beside the control that
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

/**
 * The ids a fresh selection is seeded with: every eligible default of an
 * independent row, and the **first** eligible default of each choice group in
 * list order — a legacy second default in a group is ignored rather than seeded
 * into a selection the server would refuse. Dangerous rows are filtered out
 * rather than trusted to be undefaulted: the server refuses to *set*
 * `default_enabled` on one, but a row stored before that rule can still carry
 * it.
 */
function seedableDefaultIds(options: LaunchOption[]): number[] {
  const seededGroups = new Set<string>();
  const ids: number[] = [];
  for (const option of options) {
    if (!option.default_enabled || option.dangerous) {
      continue;
    }
    const key = option.choice_group;
    if (key !== null) {
      if (seededGroups.has(key)) {
        continue;
      }
      seededGroups.add(key);
    }
    ids.push(option.id);
  }
  return ids;
}

/**
 * One choice group: a native radio group headed by the group key, with an
 * explicit "Agent default" option first. Native radios rather than checkboxes
 * dressed up as radios, and no click-the-checked-radio-to-clear trick: "Agent
 * default" is how the group is cleared.
 */
function ChoiceGroup({
  provider,
  groupKey,
  options,
  selected,
  onChoose,
}: {
  provider: AgentProvider;
  groupKey: string;
  options: LaunchOption[];
  selected: number[];
  onChoose: (id: number | null) => void;
}) {
  const headingId = useId();
  // Scoped by provider too, so a provider switch never leaves two mounted
  // groups sharing one radio name.
  const name = `launch-option-group-${provider}-${groupKey}`;
  const chosen = options.find((option) => selected.includes(option.id));
  return (
    <div
      role="radiogroup"
      aria-labelledby={headingId}
      className="space-y-0.5"
      data-testid={`launch-option-group-${groupKey}`}
    >
      <span
        id={headingId}
        className="block px-1 font-mono text-code text-fg-muted"
      >
        {groupKey}
      </span>
      <ul className="space-y-0.5 pl-3">
        <li>
          <label className="flex cursor-pointer items-center gap-2 rounded px-1 py-0.5 hover:bg-surface-elevated-hover">
            <input
              type="radio"
              name={name}
              checked={chosen === undefined}
              onChange={() => onChoose(null)}
              data-testid={`launch-option-group-${groupKey}-none`}
            />
            <span className="text-fg-muted">Agent default</span>
          </label>
        </li>
        {options.map((option) => (
          <li key={option.id}>
            <OptionLabel option={option}>
              <input
                type="radio"
                name={name}
                checked={chosen?.id === option.id}
                onChange={() => onChoose(option.id)}
                data-testid={`launch-option-${option.id}`}
              />
            </OptionLabel>
          </li>
        ))}
      </ul>
    </div>
  );
}

/**
 * A row's clickable label: its control (checkbox or radio), its optional label,
 * its `name value` pair and, for a dangerous row, the badge.
 */
function OptionLabel({
  option,
  children,
}: {
  option: LaunchOption;
  children: ReactNode;
}) {
  return (
    <label
      className="flex cursor-pointer items-center gap-2 rounded px-1 py-0.5 hover:bg-surface-elevated-hover"
      title={
        option.value === null ? option.name : `${option.name} ${option.value}`
      }
    >
      {children}
      {option.label && (
        <span className="font-medium text-fg">{option.label}</span>
      )}
      <span className="min-w-0 truncate font-mono text-code text-fg-muted">
        {option.name}
        {option.value !== null && (
          <span className="text-fg-subtle"> {option.value}</span>
        )}
      </span>
      {/* Marked in the picker too, not just in Settings: this is where the
          option is actually applied to a session. */}
      {option.dangerous && <DangerousBadge />}
    </label>
  );
}
