import { useMemo, useRef, useState, type FormEvent } from 'react';
import {
  queryKeys,
  useCreateLaunchOptionMutation,
  useDeleteLaunchOptionMutation,
  useLaunchOptionsQuery,
  useProvidersQuery,
  useUpdateLaunchOptionMutation,
} from '@delta/api-client';
import { useQueryClient } from '@tanstack/react-query';
import type {
  AgentProvider,
  LaunchOption,
  LaunchOptionStyle,
} from '@delta/wire-gen';
import { Button, cn, ProviderName, Spinner } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { partitionByChoiceGroup } from '../../launchOptions';
import { useSettingsStore } from '../../store/settingsStore';
import { PROVIDER_OPTIONS } from '../../providers';
import { settingsMutationErrorMessage } from './settingsMutationErrorMessage';
import { LaunchOptionRow } from './LaunchOptionRow';
import { LaunchOptionGroup } from './LaunchOptionGroup';

/**
 * Wording for the launch-option form, one variant per {@link LaunchOptionStyle}.
 *
 * A launch option is a provider-neutral `(name, value?)` pair, but what the
 * pair *means* differs: an argv-launched agent reads `name` as a CLI flag, a
 * request-driven one reads it as a field of its session-start request. The
 * server never validates either — it passes the pair through to the agent — so
 * this copy is the only thing telling a user which vocabulary to write in.
 * Getting it wrong is not cosmetic: registering `--model` for a field-style
 * provider produces a request field literally named `--model`, which fails at
 * the agent with no useful feedback.
 *
 * Keyed by the style the server reports (never by provider name), so a new
 * provider inherits the right wording from its declared capability rather than
 * needing a case added here.
 */
interface LaunchOptionCopy {
  /** Opening sentence of the section's helper paragraph. */
  intro: string;
  /** How the paragraph describes what `name` is ("is the flag"). */
  nameRole: string;
  /**
   * An example `name` in this style's vocabulary, shown both in the helper
   * text and as the name input's placeholder.
   */
  nameExample: string;
  /** How the paragraph describes what `value` is ("is its argument"). */
  valueRole: string;
  /**
   * An example value for {@link nameExample}, shown both in the helper text and
   * as the value input's placeholder.
   *
   * Illustrative of the *shape* a value takes in this style rather than
   * authoritative — but where the shape means naming something from the
   * provider's own catalog (a model slug), it must be one that actually exists
   * at the time of writing. Delta passes launch-option values straight through
   * without validating them, so a user who copies an invented placeholder gets
   * no feedback from Delta at all: the session-start request just fails at the
   * agent. Expect to revisit these as provider catalogs move.
   */
  valueExample: string;
  /** What omitting `value` means for this style. */
  valueOptionalNote: string;
  /** The name input's visible label. */
  nameLabel: string;
}

const LAUNCH_OPTION_COPY = {
  cli_flag: {
    intro: 'Register custom CLI flags to apply when starting a session with the selected agent.',
    nameRole: 'is the flag',
    nameExample: '--permission-mode',
    valueRole: 'is its argument',
    valueExample: 'auto',
    valueOptionalNote: 'and is optional for valueless flags',
    nameLabel: 'Name (the flag)',
  },
  request_field: {
    intro:
      'Register custom session-start settings to apply when starting a session with the selected agent.',
    nameRole: 'is the field',
    nameExample: 'model',
    valueRole: "is that field's value",
    valueExample: 'gpt-5.6-sol',
    valueOptionalNote: 'and is optional for boolean fields, which are switched on when left empty',
    nameLabel: 'Name (the field)',
  },
} as const satisfies Record<LaunchOptionStyle, LaunchOptionCopy>;

/**
 * The launch-option style to word the form in until the server's capability
 * profile has landed. Flags are what the form has always described, and
 * `GET /api/providers` is already warm by the time Settings can be opened (the
 * workspace fetches it on mount), so this only covers a cold-start blink rather
 * than a state a user reads and acts on.
 */
const FALLBACK_LAUNCH_OPTION_STYLE: LaunchOptionStyle = 'cli_flag';

/**
 * Launch options category content: manage the registry of custom agent launch
 * options (flat `(label?, name, value?)` records). Lists the registered options
 * and lets the user add one (label and value optional, name required) and
 * delete one. Selecting which options to apply when starting a session is a
 * separate concern handled elsewhere.
 *
 * Launch options belong to one provider each (Claude's flags mean nothing to
 * Codex and vice-versa), so the provider selector at the top of the section
 * scopes everything below it: new options are registered under the selected
 * provider, and only that provider's options are listed. The list endpoint
 * returns every provider's options, so filtering happens client-side, as in
 * `LaunchOptionsPicker`.
 *
 * The form's wording follows the selected provider's `launch_option_style`
 * capability from `GET /api/providers` — read as a capability, never branched
 * on the provider name — so the labels, examples and placeholders describe the
 * vocabulary that provider actually accepts (see {@link LAUNCH_OPTION_COPY}).
 *
 * Rows the server puts in one choice group (`choice_group`) are listed together
 * under the group key, with one default radio group per group (see
 * {@link LaunchOptionGroup}); every other row, a one-row group included, has
 * the plain default checkbox. The grouping and its rationale are
 * `partitionByChoiceGroup`'s, shared with the composer picker. The add form
 * stays as it is: a row registered as `--model` / `E` joins the `--model` group
 * on the next list fetch.
 *
 * `active` mirrors the dialog's `settingsOpen` AND the category being the
 * visible one, so the query only runs while this section is mounted in the
 * right pane.
 */
export function LaunchOptionsSection({ active }: { active: boolean }) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const launchOptionsQuery = useLaunchOptionsQuery(client, active);
  const createLaunchOption = useCreateLaunchOptionMutation(client);
  const updateLaunchOption = useUpdateLaunchOptionMutation(client);
  const deleteLaunchOption = useDeleteLaunchOptionMutation(client);

  const [label, setLabel] = useState('');
  const [name, setName] = useState('');
  const [value, setValue] = useState('');
  const [defaultEnabled, setDefaultEnabled] = useState(false);
  // Focused after a Duplicate, so the copy lands with the cursor already in the
  // field that is actually going to be edited — a duplicated row differs from
  // its original in its value far more often than in its name.
  const valueInputRef = useRef<HTMLTextAreaElement | null>(null);
  // Seeded from the persisted default-provider setting, the same source the
  // new-session provider selector reads: a Codex-first user would otherwise
  // land on Claude's (for them, likely empty) list every time they open
  // Settings. Seeded only, never synced — the section unmounts when its
  // category is left, so the next visit picks up the current setting.
  const defaultProvider = useSettingsStore((state) => state.defaultProvider);
  const [provider, setProvider] = useState<AgentProvider>(defaultProvider);

  // How the selected provider reads a launch option's `(name, value?)` pair.
  // Server-declared, so the form's wording follows the provider's capability
  // rather than its name.
  const providersQuery = useProvidersQuery(client);
  const copy = useMemo(() => {
    const style =
      providersQuery.data?.providers.find((entry) => entry.provider === provider)
        ?.capabilities.launch_option_style ?? FALLBACK_LAUNCH_OPTION_STYLE;
    return LAUNCH_OPTION_COPY[style];
  }, [providersQuery.data, provider]);

  const options = launchOptionsQuery.data?.launch_options ?? [];
  const providerOptions = useMemo(
    () => options.filter((option) => option.provider === provider),
    [options, provider],
  );
  const entries = useMemo(
    () => partitionByChoiceGroup(providerOptions),
    [providerOptions],
  );
  // The last failed default switch in a choice group, keyed by the group, so
  // the refusal is shown beside the group it belongs to.
  const [defaultSwitchError, setDefaultSwitchError] = useState<{
    group: string;
    error: unknown;
  } | null>(null);
  // `name` is the only required field; trim so an all-whitespace entry cannot
  // be submitted (the server rejects it too, but gating here avoids a round-trip
  // and keeps the button state honest).
  const canSubmit = name.trim().length > 0 && !createLaunchOption.isPending;

  /**
   * Seed the add form from an existing row, built-in or the user's own.
   *
   * This is the supported way to build on a row Delta ships: those are not
   * editable, and a real Codex `config` value carries machine-specific paths, so
   * the flow is "duplicate the shipped row, add your paths, register yours" —
   * safer than selecting a JSON value out of the page by hand.
   *
   * `default_enabled` is deliberately not copied: pre-checking a row the user
   * has not finished writing would arm it for the next session they start.
   */
  const onDuplicate = (option: LaunchOption) => {
    setLabel(option.label ?? '');
    setName(option.name);
    setValue(option.value ?? '');
    valueInputRef.current?.focus();
  };

  /**
   * Make `id` the default of a choice group (`null` clears the group), given
   * the rows that hold its default now. Resolves `true` once the server has it,
   * `false` after recording the refusal beside the group.
   *
   * Two writes, in order: clear every holder other than `id`, then set `id`
   * unless it already holds the default. The server holds one default per group
   * and never flips a sibling itself, so the reverse order would be refused.
   * More than one holder is possible only for rows stored before that rule; all
   * of them are cleared. The holders are passed in rather than read from the
   * list because {@link LaunchOptionGroup} chains switches faster than the list
   * refetches, and only it knows what the previous switch left behind.
   */
  const switchGroupDefault = async (
    group: string,
    holders: readonly number[],
    id: number | null,
  ): Promise<boolean> => {
    setDefaultSwitchError(null);
    try {
      for (const holder of holders) {
        if (holder !== id) {
          await updateLaunchOption.mutateAsync({
            id: holder,
            body: { default_enabled: false },
          });
        }
      }
      if (id !== null && !holders.includes(id)) {
        await updateLaunchOption.mutateAsync({
          id,
          body: { default_enabled: true },
        });
      }
      return true;
    } catch (error) {
      setDefaultSwitchError({ group, error });
      return false;
    }
  };

  /** Refetch the list, resolving once the fresh rows are in the cache. */
  const refreshLaunchOptions = () =>
    queryClient.invalidateQueries({ queryKey: queryKeys.launchOptions });

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (!canSubmit) {
      return;
    }
    const trimmedLabel = label.trim();
    const trimmedValue = value.trim();
    createLaunchOption.mutate(
      {
        // Omit empty optionals so they serialize as absent rather than "".
        label: trimmedLabel.length > 0 ? trimmedLabel : undefined,
        name: name.trim(),
        value: trimmedValue.length > 0 ? trimmedValue : undefined,
        default_enabled: defaultEnabled,
        provider,
      },
      {
        onSuccess: () => {
          setLabel('');
          setName('');
          setValue('');
          setDefaultEnabled(false);
          // The provider is deliberately NOT reset: it scopes the list too, so
          // resetting it would yank the view away from the option just added.
        },
      },
    );
  };

  return (
    <section className="w-full" data-testid="launch-options-section">
      <h3 className="mb-1 text-secondary font-semibold text-fg">Launch options</h3>
      <p className="mb-4 text-caption text-fg-muted">
        {copy.intro} <span className="font-medium">Name</span> {copy.nameRole} (e.g.{' '}
        <code>{copy.nameExample}</code>); <span className="font-medium">value</span>{' '}
        {copy.valueRole} (e.g. <code>{copy.valueExample}</code>){' '}
        {copy.valueOptionalNote}. <span className="font-medium">Label</span> is an
        optional note.
      </p>

      <div className="mb-4 flex flex-col gap-1">
        <span className="text-caption font-medium text-fg-muted">Provider</span>
        <div
          role="radiogroup"
          aria-label="Launch options provider"
          className="flex gap-1 rounded border border-border-default bg-surface p-1"
          data-testid="launch-option-provider-selector"
        >
          {PROVIDER_OPTIONS.map((option) => {
            const selected = provider === option.value;
            return (
              <label
                key={option.value}
                className={cn(
                  'flex flex-1 cursor-pointer items-center justify-center gap-2 rounded px-3 py-1.5 text-secondary transition',
                  selected
                    ? 'bg-accent/10 text-fg ring-1 ring-accent/30'
                    : 'text-fg-muted hover:bg-surface-elevated',
                )}
                data-testid={`launch-option-provider-${option.value}`}
              >
                <input
                  type="radio"
                  name="launch-option-provider"
                  value={option.value}
                  checked={selected}
                  onChange={() => setProvider(option.value)}
                  className="sr-only"
                />
                <ProviderName provider={option.value} className="font-medium" />
              </label>
            );
          })}
        </div>
      </div>

      {/* Add form */}
      <form
        onSubmit={onSubmit}
        className="mb-6 flex flex-col gap-3 rounded-lg border border-border-default bg-surface-elevated p-3"
        aria-label="Add launch option"
      >
        <div className="flex flex-col gap-1">
          <label className="text-caption font-medium text-fg-muted" htmlFor="lo-label">
            Label (optional)
          </label>
          <input
            id="lo-label"
            type="text"
            value={label}
            onChange={(event) => setLabel(event.target.value)}
            placeholder="My plugins"
            className="rounded border border-border-default bg-surface px-2 py-1 text-secondary text-fg placeholder:text-fg-subtle focus:border-accent-hover focus:outline-none"
          />
        </div>
        <div className="flex flex-col gap-1">
          <label className="text-caption font-medium text-fg-muted" htmlFor="lo-name">
            {copy.nameLabel}
          </label>
          <input
            id="lo-name"
            type="text"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={copy.nameExample}
            required
            className="rounded border border-border-default bg-surface px-2 py-1 text-secondary text-fg placeholder:text-fg-subtle focus:border-accent-hover focus:outline-none"
          />
        </div>
        <div className="flex flex-col gap-1">
          <label className="text-caption font-medium text-fg-muted" htmlFor="lo-value">
            Value (optional)
          </label>
          {/* A textarea, not a one-line input: a Codex `config` value is a JSON
              object that does not fit on one line, and Duplicate drops exactly
              such a value in here to be edited. Monospaced because every value
              is read in the provider's own vocabulary, resizable and internally
              scrolling so a long one neither clips nor pushes the button off
              screen. A textarea does not submit on Enter, so the form's own
              button stays the only way to submit — which is what a multi-line
              value needs. */}
          <textarea
            id="lo-value"
            ref={valueInputRef}
            rows={3}
            value={value}
            onChange={(event) => setValue(event.target.value)}
            placeholder={copy.valueExample}
            className="resize-y overflow-auto rounded border border-border-default bg-surface px-2 py-1 font-mono text-code text-fg placeholder:text-fg-subtle focus:border-accent-hover focus:outline-none"
          />
        </div>
        <label className="flex items-center gap-2 text-caption font-medium text-fg-muted">
          <input
            type="checkbox"
            checked={defaultEnabled}
            onChange={(event) => setDefaultEnabled(event.target.checked)}
            className="h-3.5 w-3.5"
          />
          Enabled by default (pre-checked when starting a session)
        </label>
        {createLaunchOption.isError && (
          // The server's own message when it sent one: this form's one
          // *rule-based* refusal is a safety-bypassing option asked to be
          // enabled by default, and "please try again" would send the user round
          // the same loop instead of telling them to untick the box. Whether a
          // typed name is such an option is only knowable server-side, so the
          // control cannot be pre-disabled the way a registered row's is.
          <p className="text-caption text-danger" role="alert">
            {settingsMutationErrorMessage(
              createLaunchOption.error,
              'Could not add the launch option.',
            )}
          </p>
        )}
        <div className="flex justify-end">
          <Button type="submit" variant="primary" size="sm" disabled={!canSubmit}>
            Add option
          </Button>
        </div>
      </form>

      {/* Registered options */}
      {launchOptionsQuery.isPending ? (
        <div className="flex justify-center py-6">
          <Spinner label="loading launch options" />
        </div>
      ) : launchOptionsQuery.isError ? (
        <div className="flex flex-col items-center gap-2 py-6 text-secondary text-fg-muted">
          <p>Could not load launch options.</p>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => launchOptionsQuery.refetch()}
          >
            Retry
          </Button>
        </div>
      ) : providerOptions.length === 0 ? (
        // Names the provider: the other one may well have options, so this
        // must not read as an empty registry.
        <p
          className="py-6 text-center text-secondary text-fg-subtle"
          data-testid="launch-options-empty"
        >
          No launch options registered for{' '}
          <ProviderName provider={provider} className="font-medium" /> yet.
        </p>
      ) : (
        <ul className="flex flex-col gap-2" data-testid="launch-options-list">
          {entries.map((entry) => {
            const rowProps = (option: LaunchOption) => ({
              option,
              onToggleDefault: (next: boolean) =>
                updateLaunchOption.mutate({
                  id: option.id,
                  body: { default_enabled: next },
                }),
              toggling:
                updateLaunchOption.isPending &&
                updateLaunchOption.variables?.id === option.id,
              onDelete: () => deleteLaunchOption.mutate(option.id),
              deleting:
                deleteLaunchOption.isPending &&
                deleteLaunchOption.variables === option.id,
              onDuplicate: () => onDuplicate(option),
            });
            if (entry.kind === 'option') {
              return (
                <LaunchOptionRow
                  key={entry.option.id}
                  {...rowProps(entry.option)}
                />
              );
            }
            return (
              <LaunchOptionGroup
                key={`group:${entry.key}`}
                provider={provider}
                groupKey={entry.key}
                options={entry.options}
                onSwitchDefault={(holders, id) =>
                  switchGroupDefault(entry.key, holders, id)
                }
                onSettled={refreshLaunchOptions}
                error={
                  defaultSwitchError !== null &&
                  defaultSwitchError.group === entry.key
                    ? defaultSwitchError.error
                    : null
                }
                renderRow={(option, defaultChoice) => (
                  <LaunchOptionRow
                    key={option.id}
                    {...rowProps(option)}
                    defaultChoice={defaultChoice}
                  />
                )}
              />
            );
          })}
        </ul>
      )}
    </section>
  );
}
