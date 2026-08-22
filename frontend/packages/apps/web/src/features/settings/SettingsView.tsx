import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
  type ReactNode,
} from 'react';
import {
  ApiError,
  useAddCloneRootMutation,
  useCreateLaunchOptionMutation,
  useCreatePromptTemplateMutation,
  useDeleteLaunchOptionMutation,
  useDeletePromptTemplateMutation,
  useHomeDirQuery,
  useLaunchOptionsQuery,
  usePromptTemplatesQuery,
  useRemoveCloneRootMutation,
  useProvidersQuery,
  useCloneRootsQuery,
  useUpdateLaunchOptionMutation,
  useUpdatePromptTemplateMutation,
} from '@delta/api-client';
import type {
  AgentProvider,
  LaunchOption,
  LaunchOptionStyle,
  CloneRoot,
  PromptTemplate,
} from '@delta/wire-gen';
import { Button, cn, Dialog, ProviderName, Spinner } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useThemeContext } from '../../hooks/themeContext';
import { useNavStore } from '../../store/navStore';
import {
  type SettingsCategoryId,
  type VisualEffectsSetting,
  useSettingsStore,
} from '../../store/settingsStore';
import { SYSTEM_PREFERENCE, type ThemePreference } from '../../hooks/useTheme';
import { THEMES } from '../../themes/registry';
import { displayPath } from '../../utils/displayPath';
import { PROVIDER_OPTIONS } from '../../providers';
import { WorkdirPickerBody } from '../composer/WorkdirPickerBody';

/**
 * The settings modal: hosts the registry of per-provider CLI launch options,
 * the registry of prompt templates, and the registry of clone roots, each a
 * top-level category in a VS Code-style 2-pane layout. The left rail lists
 * categories; the right pane renders the active category's content. The
 * categories are conceptually unrelated (one targets session startup flags,
 * another reusable prompt text, another where to look for git repos to start
 * sessions in), so they live in separate panes rather than stacked sections —
 * keeping each category's UI undivided by the others.
 *
 * Rendered as a {@link Dialog} overlay layered on top of the workspace rather
 * than replacing the center pane, so the conversation stays in place beneath
 * it. Opened from the navigator's lower-left settings entry (`openSettings`)
 * and closed via the dialog's Close button, Esc, or a backdrop click
 * (`closeSettings`). The active category is persisted to localStorage so a
 * reload (or a dialog close/reopen) restores the last view.
 */
export function SettingsView() {
  const settingsOpen = useNavStore((state) => state.settingsOpen);
  const closeSettings = useNavStore((state) => state.closeSettings);
  const activeCategory = useSettingsStore((state) => state.activeCategory);
  const setActiveCategory = useSettingsStore((state) => state.setActiveCategory);

  // Single source of truth for category id + label + content. Adding a new
  // top-level category is one entry here plus a new id in `settingsStore.ts`.
  // The renderer receives `active` so each category can gate its data queries
  // on the dialog being open AND the category being the visible one — an
  // inactive category does not fire its initial fetch.
  const categories: {
    id: SettingsCategoryId;
    label: string;
    render: (active: boolean) => ReactNode;
  }[] = [
    {
      id: 'launch-options',
      label: 'Launch options',
      render: (active) => <LaunchOptionsSection active={active} />,
    },
    {
      id: 'prompt-templates',
      label: 'Prompt templates',
      render: (active) => <PromptTemplatesSection active={active} />,
    },
    {
      id: 'clone-roots',
      label: 'Clone roots',
      render: (active) => <CloneRootsSection active={active} />,
    },
    {
      id: 'appearance',
      label: 'Appearance',
      // The Appearance section has no data fetch of its own; the `active`
      // prop is ignored.
      render: () => <AppearanceSection />,
    },
    {
      id: 'default-provider',
      label: 'Default provider',
      // The Default provider section reads a persisted preference only; no data
      // fetch, so the `active` prop is ignored.
      render: () => <DefaultProviderSection />,
    },
  ];

  const activeIndex = categories.findIndex((c) => c.id === activeCategory);
  const railRef = useRef<HTMLDivElement>(null);

  // Arrow Up / Arrow Down on the left rail moves focus between categories
  // (standard ARIA vertical-tablist behavior). Home / End jump to first /
  // last. We focus the destination button and switch the active category in
  // the same step, so the right pane follows the keyboard focus.
  const onRailKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (categories.length === 0) {
      return;
    }
    const current = activeIndex >= 0 ? activeIndex : 0;
    let next: number | null = null;
    if (event.key === 'ArrowDown') {
      next = (current + 1) % categories.length;
    } else if (event.key === 'ArrowUp') {
      next = (current - 1 + categories.length) % categories.length;
    } else if (event.key === 'Home') {
      next = 0;
    } else if (event.key === 'End') {
      next = categories.length - 1;
    }
    if (next === null) {
      return;
    }
    event.preventDefault();
    const target = categories[next];
    setActiveCategory(target.id);
    const button = railRef.current?.querySelector<HTMLButtonElement>(
      `[data-testid="settings-category-${target.id}"]`,
    );
    button?.focus();
  };

  const active = categories[activeIndex >= 0 ? activeIndex : 0];

  return (
    <Dialog
      open={settingsOpen}
      onClose={closeSettings}
      title="Settings"
      // Wider than the single-pane prompt: the 2-pane layout needs room for a
      // ~180px left rail plus the right pane's option rows (label + monospace
      // flag/value) without truncating either column.
      //
      // The height is a fixed frame rather than content-derived, because the
      // categories differ wildly in natural height (a two-option radio group
      // vs. a form plus an unbounded list). Sizing to the content made the
      // panel resize on every category switch, and since the backdrop centers
      // it, both edges moved — including the rail button the user had just
      // clicked. The frame belongs to the dialog, not to whichever category
      // happens to be showing; overflow is the right pane's business. `min()`
      // keeps it viewport-bound on short screens, where it degrades to the
      // `max-h-full` behavior it would have had anyway.
      className="h-[min(42rem,100%)] max-w-4xl"
      footer={
        <Button variant="ghost" onClick={closeSettings} data-testid="settings-close">
          Close
        </Button>
      }
    >
      <div className="flex h-full w-full gap-4">
        <div
          ref={railRef}
          role="tablist"
          aria-label="Settings categories"
          aria-orientation="vertical"
          className="flex w-44 shrink-0 flex-col gap-1 border-r border-border-default pr-3"
          data-testid="settings-categories"
          onKeyDown={onRailKeyDown}
        >
          {categories.map((category) => {
            const selected = category.id === active.id;
            return (
              <button
                key={category.id}
                type="button"
                role="tab"
                aria-selected={selected}
                aria-controls={`settings-panel-${category.id}`}
                id={`settings-tab-${category.id}`}
                // The active tab is in the focus order; the others are skipped
                // by Tab and reached via the arrow-key handler above (standard
                // ARIA "roving tabindex" pattern for a tablist).
                tabIndex={selected ? 0 : -1}
                onClick={() => setActiveCategory(category.id)}
                className={cn(
                  'rounded px-3 py-1.5 text-left text-caption font-medium transition',
                  selected
                    ? 'bg-accent/10 text-accent ring-1 ring-accent/30'
                    : 'text-fg-muted hover:bg-surface-elevated hover:text-fg',
                )}
                data-testid={`settings-category-${category.id}`}
              >
                {category.label}
              </button>
            );
          })}
        </div>
        <div
          role="tabpanel"
          id={`settings-panel-${active.id}`}
          aria-labelledby={`settings-tab-${active.id}`}
          // The right pane scrolls independently of the rail so a long
          // launch-options list never pushes the rail out of view.
          // `scrollbar-hover` (not `scrollbar-none`): the fixed-frame dialog
          // clips overflowing categories with no other cue that more content
          // exists, so keep a hover-revealed thumb as a position indicator.
          className="min-w-0 flex-1 overflow-y-auto scrollbar-hover"
          data-testid={`settings-panel-${active.id}`}
        >
          {active.render(settingsOpen)}
        </div>
      </div>
    </Dialog>
  );
}

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
 * `active` mirrors the dialog's `settingsOpen` AND the category being the
 * visible one, so the query only runs while this section is mounted in the
 * right pane.
 */
function LaunchOptionsSection({ active }: { active: boolean }) {
  const client = useApiClient();
  const launchOptionsQuery = useLaunchOptionsQuery(client, active);
  const createLaunchOption = useCreateLaunchOptionMutation(client);
  const updateLaunchOption = useUpdateLaunchOptionMutation(client);
  const deleteLaunchOption = useDeleteLaunchOptionMutation(client);

  const [label, setLabel] = useState('');
  const [name, setName] = useState('');
  const [value, setValue] = useState('');
  const [defaultEnabled, setDefaultEnabled] = useState(false);
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
  // `name` is the only required field; trim so an all-whitespace entry cannot
  // be submitted (the server rejects it too, but gating here avoids a round-trip
  // and keeps the button state honest).
  const canSubmit = name.trim().length > 0 && !createLaunchOption.isPending;

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
          <input
            id="lo-value"
            type="text"
            value={value}
            onChange={(event) => setValue(event.target.value)}
            placeholder={copy.valueExample}
            className="rounded border border-border-default bg-surface px-2 py-1 text-secondary text-fg placeholder:text-fg-subtle focus:border-accent-hover focus:outline-none"
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
          <p className="text-caption text-danger" role="alert">
            Could not add the launch option. Please try again.
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
          {providerOptions.map((option) => (
            <LaunchOptionRow
              key={option.id}
              option={option}
              onToggleDefault={(next) =>
                updateLaunchOption.mutate({
                  id: option.id,
                  body: { default_enabled: next },
                })
              }
              toggling={
                updateLaunchOption.isPending &&
                updateLaunchOption.variables?.id === option.id
              }
              onDelete={() => deleteLaunchOption.mutate(option.id)}
              deleting={
                deleteLaunchOption.isPending &&
                deleteLaunchOption.variables === option.id
              }
            />
          ))}
        </ul>
      )}
    </section>
  );
}

/**
 * The prompt-template editor's working copy, or `null` while the list view is
 * showing — the single piece of state that decides which of the category's two
 * views the right pane renders.
 *
 * `id` is `null` for a brand-new template (the save is a `POST`) and the
 * template's id when editing an existing one (a `PATCH`). The content fields
 * start empty or pre-filled accordingly.
 *
 * Deliberately local component state rather than anything persisted: the
 * section unmounts when its category is left or the dialog closes, which is
 * exactly the documented v1 behaviour — an abandoned draft is discarded
 * without a confirmation.
 */
interface PromptTemplateDraft {
  /** The template being edited, or `null` for a new one. */
  id: number | null;
  label: string;
  text: string;
}

/**
 * The message shown for a failed prompt-template mutation. The server's own
 * explanation is appended when it sent one — {@link ApiError} carries the
 * parsed `error` field — so a user is told *why* rather than only that
 * something failed. A transport failure or an opaque status leaves just the
 * fallback sentence.
 */
function promptTemplateErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof ApiError && error.message.length > 0) {
    return `${fallback} ${error.message}`;
  }
  return fallback;
}

/**
 * Prompt templates category content: manage the registry of named, reusable
 * blocks of instruction text. Unlike launch options, templates are global —
 * the body is prose the agent reads, so it means the same on every provider —
 * and there is no provider selector scoping the list.
 *
 * The pane shows one of two views at a time, never both: the **list** (a "New
 * template" button plus one row per registered template) or the **editor**
 * (label + text, reached from "New template" or a row's "Edit"). A template's
 * body is expected to run to many lines, so the list shows the label only —
 * no preview, no truncated first line — and the editor hands the textarea the
 * whole remaining pane height, scrolling internally rather than growing the
 * dialog.
 *
 * Deleting asks for confirmation first, which the launch-option rows
 * deliberately do not: a long template represents real writing, and there is
 * no undo.
 *
 * `active` mirrors the dialog's `settingsOpen` AND the category being the
 * visible one, so the query only runs while this section is mounted in the
 * right pane.
 */
function PromptTemplatesSection({ active }: { active: boolean }) {
  const client = useApiClient();
  const templatesQuery = usePromptTemplatesQuery(client, active);
  const createTemplate = useCreatePromptTemplateMutation(client);
  const updateTemplate = useUpdatePromptTemplateMutation(client);
  const deleteTemplate = useDeletePromptTemplateMutation(client);

  const [draft, setDraft] = useState<PromptTemplateDraft | null>(null);
  // The template a delete has been requested for, i.e. the confirmation
  // dialog's subject; `null` while no confirmation is pending.
  const [pendingDelete, setPendingDelete] = useState<PromptTemplate | null>(
    null,
  );

  // React Query hands back a fresh mutation object every render, so pull the
  // stable `reset` functions out before using them in callbacks (the same
  // reason CloneRootsSection does it for its effect).
  const resetCreate = createTemplate.reset;
  const resetUpdate = updateTemplate.reset;
  const resetDelete = deleteTemplate.reset;

  /** Open the editor on a draft, clearing any error left by an earlier save. */
  const openEditor = (next: PromptTemplateDraft) => {
    resetCreate();
    resetUpdate();
    setDraft(next);
  };

  /** Return to the list, discarding the draft and any save error with it. */
  const closeEditor = () => {
    resetCreate();
    resetUpdate();
    setDraft(null);
  };

  const templates = templatesQuery.data?.prompt_templates ?? [];
  const saving = createTemplate.isPending || updateTemplate.isPending;
  // Both fields are required and the server rejects a blank one, so gate the
  // button on the trimmed values — the trim decides only whether saving is
  // allowed, never what is sent (see below).
  const canSave =
    draft !== null &&
    draft.label.trim().length > 0 &&
    draft.text.trim().length > 0 &&
    !saving;
  const saveError = createTemplate.error ?? updateTemplate.error;

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (draft === null || !canSave) {
      return;
    }
    // `text` goes over the wire verbatim: the server stores it byte-for-byte
    // and the composer inserts it the same way, so trimming here would
    // silently rewrite a template whose body deliberately opens or closes
    // with blank lines. The label is a one-line name, so stray edge
    // whitespace there is noise and is trimmed, as it is for launch options.
    const body = { label: draft.label.trim(), text: draft.text };
    const onSuccess = () => setDraft(null);
    if (draft.id === null) {
      createTemplate.mutate(body, { onSuccess });
    } else {
      updateTemplate.mutate({ id: draft.id, body }, { onSuccess });
    }
  };

  const confirmDelete = () => {
    if (pendingDelete === null) {
      return;
    }
    deleteTemplate.mutate(pendingDelete.id, {
      onSuccess: () => setPendingDelete(null),
    });
  };

  return (
    // A column that fills the pane: the header is fixed and the view below it
    // takes the rest, which is what lets the editor's textarea occupy the full
    // remaining height instead of a fixed number of rows.
    <section
      className="flex h-full w-full flex-col"
      data-testid="prompt-templates-section"
    >
      <h3 className="mb-1 text-secondary font-semibold text-fg">
        Prompt templates
      </h3>
      <p className="mb-4 text-caption text-fg-muted">
        Register the instruction text you write often — a review checklist, a
        merge routine — so it is written once and kept here instead of
        retyped. Templates are provider-independent: the same text reads the
        same on every agent.
      </p>

      {draft === null ? (
        <>
          <div className="mb-3 flex shrink-0 justify-end">
            <Button
              size="sm"
              variant="primary"
              onClick={() => openEditor({ id: null, label: '', text: '' })}
              data-testid="prompt-template-new"
            >
              New template
            </Button>
          </div>
          {/* The list scrolls within the pane rather than growing it, so the
              "New template" button stays put no matter how many templates
              are registered. */}
          <div className="min-h-0 flex-1 overflow-y-auto scrollbar-hover">
            {templatesQuery.isPending ? (
              <div className="flex justify-center py-6">
                <Spinner label="loading prompt templates" />
              </div>
            ) : templatesQuery.isError ? (
              <div className="flex flex-col items-center gap-2 py-6 text-secondary text-fg-muted">
                <p>Could not load prompt templates.</p>
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => templatesQuery.refetch()}
                >
                  Retry
                </Button>
              </div>
            ) : templates.length === 0 ? (
              <p
                className="py-6 text-center text-secondary text-fg-subtle"
                data-testid="prompt-templates-empty"
              >
                No prompt templates yet.
              </p>
            ) : (
              <ul
                className="flex flex-col gap-2"
                data-testid="prompt-templates-list"
              >
                {templates.map((template) => (
                  <PromptTemplateRow
                    key={template.id}
                    template={template}
                    onEdit={() =>
                      openEditor({
                        id: template.id,
                        label: template.label,
                        text: template.text,
                      })
                    }
                    onDelete={() => {
                      resetDelete();
                      setPendingDelete(template);
                    }}
                  />
                ))}
              </ul>
            )}
          </div>
        </>
      ) : (
        <form
          onSubmit={onSubmit}
          className="flex min-h-0 flex-1 flex-col gap-3"
          aria-label={
            draft.id === null ? 'New prompt template' : 'Edit prompt template'
          }
          data-testid="prompt-template-editor"
        >
          <div className="flex shrink-0 flex-col gap-1">
            <label
              className="text-caption font-medium text-fg-muted"
              htmlFor="pt-label"
            >
              Label
            </label>
            <input
              id="pt-label"
              type="text"
              value={draft.label}
              onChange={(event) =>
                setDraft({ ...draft, label: event.target.value })
              }
              placeholder="Review checklist"
              required
              className="rounded border border-border-default bg-surface px-2 py-1 text-secondary text-fg placeholder:text-fg-subtle focus:border-accent-hover focus:outline-none"
            />
          </div>
          <div className="flex min-h-0 flex-1 flex-col gap-1">
            <label
              className="text-caption font-medium text-fg-muted"
              htmlFor="pt-text"
            >
              Text
            </label>
            {/* `flex-1` + `min-h-0` hands the textarea whatever the pane has
                left, and `resize-none` keeps that the only thing sizing it —
                a drag handle here would fight the dialog's fixed frame. Long
                bodies scroll inside it. The body text style is the app's own,
                so a template reads in the editor the way it will read in the
                composer. */}
            <textarea
              id="pt-text"
              value={draft.text}
              onChange={(event) =>
                setDraft({ ...draft, text: event.target.value })
              }
              placeholder="Review the diff on this branch with a critic's eye…"
              required
              className="min-h-0 flex-1 resize-none rounded border border-border-default bg-surface px-2 py-1.5 text-body text-fg placeholder:text-fg-subtle focus:border-accent-hover focus:outline-none scrollbar-hover"
            />
          </div>
          {saveError !== null && (
            <p
              className="shrink-0 text-caption text-danger"
              role="alert"
              data-testid="prompt-template-save-error"
            >
              {promptTemplateErrorMessage(
                saveError,
                'Could not save the prompt template.',
              )}
            </p>
          )}
          <div className="flex shrink-0 justify-end gap-2">
            <Button variant="ghost" size="sm" onClick={closeEditor}>
              Cancel
            </Button>
            <Button
              type="submit"
              variant="primary"
              size="sm"
              disabled={!canSave}
            >
              Save
            </Button>
          </div>
        </form>
      )}

      <Dialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        title="Delete prompt template"
        footer={
          <>
            <Button variant="ghost" onClick={() => setPendingDelete(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={confirmDelete}
              disabled={deleteTemplate.isPending}
              data-testid="prompt-template-delete-confirm"
            >
              Delete
            </Button>
          </>
        }
      >
        <div className="space-y-3">
          <p className="text-secondary text-fg">
            Delete{' '}
            <span className="font-medium">{pendingDelete?.label}</span>? Its
            text is not recoverable.
          </p>
          {/* A failed delete leaves the confirmation open so the user can
              retry from where they are, with the reason next to the button
              that failed. */}
          {deleteTemplate.isError && (
            <p
              className="text-caption text-danger"
              role="alert"
              data-testid="prompt-template-delete-error"
            >
              {promptTemplateErrorMessage(
                deleteTemplate.error,
                'Could not delete the prompt template.',
              )}
            </p>
          )}
        </div>
      </Dialog>
    </section>
  );
}

interface PromptTemplateRowProps {
  template: PromptTemplate;
  onEdit: () => void;
  onDelete: () => void;
}

/**
 * One row of the prompt-template list: the label and the two actions, and
 * deliberately nothing of the body. A template is a multi-paragraph block, so
 * any preview would either truncate it into nonsense or swamp the list; the
 * label is the name the user chose for exactly this purpose.
 */
function PromptTemplateRow({
  template,
  onEdit,
  onDelete,
}: PromptTemplateRowProps) {
  return (
    <li className="flex items-center justify-between gap-3 rounded-lg border border-border-default px-3 py-2">
      <span className="truncate text-secondary text-fg" title={template.label}>
        {template.label}
      </span>
      <div className="flex shrink-0 items-center gap-1">
        <Button
          size="sm"
          variant="ghost"
          onClick={onEdit}
          aria-label={`Edit prompt template ${template.label}`}
        >
          Edit
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={onDelete}
          aria-label={`Delete prompt template ${template.label}`}
        >
          Delete
        </Button>
      </div>
    </li>
  );
}

/**
 * Clone roots category content: list of the registered directories where the
 * user's git clones live — every Repository tab refetch probes their direct
 * children for clones — plus a one-shot picker to register a new one. Drives
 * the same backend the New session screen consults, so adding a clone root
 * here surfaces previously-hidden clones on the very next refetch.
 *
 * `active` mirrors the dialog's `settingsOpen` AND the category being the
 * visible one, so the query only runs while the section is mounted.
 */
function CloneRootsSection({ active }: { active: boolean }) {
  const client = useApiClient();
  const cloneRootsQuery = useCloneRootsQuery(client, active);
  const addCloneRoot = useAddCloneRootMutation(client);
  const removeCloneRoot = useRemoveCloneRootMutation(client);
  const home = useHomeDirQuery(client, active).data?.path ?? null;

  const [pickerOpen, setPickerOpen] = useState(false);
  // The picker's candidate selection, lifted to this component so the dialog's
  // Add button can drive submission and the Cancel button can dismiss without
  // committing.
  const [candidate, setCandidate] = useState<string | null>(null);

  // The mutation's error is surfaced inline on the picker dialog so a 409
  // duplicate shows a tiny "Already registered." hint instead of a global
  // toast. Cleared whenever the picker reopens.
  const duplicate =
    addCloneRoot.error instanceof ApiError &&
    addCloneRoot.error.code === 'clone_root_duplicate';

  // React Query's mutation handle is a fresh object every render, so it
  // cannot sit in the effect dependencies (it would re-fire forever). Pull
  // `reset` out and depend on it (a stable function reference within a given
  // QueryClient lifetime), which keeps the effect well-behaved.
  const resetCloneRootMutation = addCloneRoot.reset;
  useEffect(() => {
    if (!pickerOpen) {
      setCandidate(null);
      resetCloneRootMutation();
    }
  }, [pickerOpen, resetCloneRootMutation]);

  const submit = () => {
    if (candidate === null) {
      return;
    }
    addCloneRoot.mutate(
      { path: candidate },
      {
        onSuccess: () => setPickerOpen(false),
      },
    );
  };

  const cloneRoots = cloneRootsQuery.data?.clone_roots ?? [];

  return (
    <section className="space-y-3" data-testid="clone-roots-section">
      <div>
        <h3 className="mb-1 text-secondary font-semibold text-fg">
          Clone roots
        </h3>
        <p className="text-caption text-fg-muted">
          Delta scans the direct children of each path below for git
          repositories so you can pick them from the Repository tab without
          having to start a session there first.
        </p>
      </div>

      {cloneRootsQuery.isPending ? (
        <div className="flex justify-center py-4">
          <Spinner label="loading clone roots" />
        </div>
      ) : cloneRootsQuery.isError ? (
        <div className="flex flex-col items-center gap-2 py-4 text-secondary text-fg-muted">
          <p>Could not load clone roots.</p>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => cloneRootsQuery.refetch()}
          >
            Retry
          </Button>
        </div>
      ) : cloneRoots.length === 0 ? (
        <p className="py-3 text-center text-secondary text-fg-subtle">
          No clone roots registered yet.
        </p>
      ) : (
        <ul className="flex flex-col gap-2" data-testid="clone-roots-list">
          {cloneRoots.map((root) => (
            <CloneRootRow
              key={root.path}
              root={root}
              home={home}
              onRemove={() => removeCloneRoot.mutate(root.path)}
              removing={
                removeCloneRoot.isPending && removeCloneRoot.variables === root.path
              }
            />
          ))}
        </ul>
      )}

      <div className="flex justify-end">
        <Button
          size="sm"
          variant="secondary"
          onClick={() => setPickerOpen(true)}
          data-testid="add-clone-root"
        >
          Add clone root…
        </Button>
      </div>

      <Dialog
        open={pickerOpen}
        onClose={() => setPickerOpen(false)}
        title="Pick a parent directory"
        footer={
          <>
            <Button variant="ghost" onClick={() => setPickerOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={submit}
              disabled={candidate === null || addCloneRoot.isPending}
              data-testid="clone-root-confirm"
            >
              Add
            </Button>
          </>
        }
      >
        <div className="space-y-3">
          <p className="text-caption text-fg-muted">
            Pick the parent of one or more git clones. The Repository tab will
            then probe its direct children for <code>.git</code> on every
            refetch.
          </p>
          <WorkdirPickerBody
            active={pickerOpen}
            candidate={candidate}
            setCandidate={setCandidate}
            onConfirm={submit}
            showHelpText={false}
          />
          {duplicate && (
            <p
              className="text-caption text-warning"
              role="alert"
              data-testid="clone-root-duplicate"
            >
              Already registered.
            </p>
          )}
          {addCloneRoot.isError && !duplicate && (
            <p className="text-caption text-danger" role="alert">
              Could not add the clone root. Please try again.
            </p>
          )}
        </div>
      </Dialog>
    </section>
  );
}

interface CloneRootRowProps {
  root: CloneRoot;
  home: string | null;
  onRemove: () => void;
  removing: boolean;
}

function CloneRootRow({ root, home, onRemove, removing }: CloneRootRowProps) {
  return (
    <li className="flex items-center justify-between gap-3 rounded-lg border border-border-default px-3 py-2">
      <span
        className="truncate font-mono text-code text-fg"
        title={root.path}
      >
        {displayPath(root.path, home)}
      </span>
      <Button
        size="sm"
        variant="ghost"
        onClick={onRemove}
        disabled={removing}
        aria-label={`Remove clone root ${root.path}`}
      >
        Remove
      </Button>
    </li>
  );
}

/**
 * Appearance category content: pick which theme drives the UI. Options are
 * sourced from the theme registry (every registered `:root[data-theme="…"]`
 * block in src/index.css) plus a `System` option that follows
 * `prefers-color-scheme`. Selection writes through {@link useThemeContext}'s
 * setter, which persists to localStorage and updates `<html data-theme="…">`
 * — the surrounding UI (and the embedded xterm canvas) re-resolves its
 * design tokens in the same tick.
 *
 * The control is a radio group so each option is independently focusable for
 * keyboard navigation and screen readers announce the role correctly. The
 * highlight reflects the user's stated preference (including `system`)
 * rather than the resolved id, so picking `System` stays visibly checked
 * regardless of which concrete theme the OS currently signals.
 */
function AppearanceSection() {
  const { preference, setPreference } = useThemeContext();
  const visualEffects = useSettingsStore((state) => state.visualEffects);
  const setVisualEffects = useSettingsStore((state) => state.setVisualEffects);

  const options: { value: ThemePreference; label: string; hint: string }[] = [
    ...THEMES.map((theme) => ({
      value: theme.id as ThemePreference,
      label: theme.displayName,
      hint: theme.isDark ? 'Dark surfaces' : 'Light surfaces',
    })),
    {
      value: SYSTEM_PREFERENCE,
      label: 'System',
      hint: 'Follow the OS preference',
    },
  ];

  // Visual-effects options. "Auto" defers to the platform (flat on Linux
  // WebKit, rich elsewhere); "On"/"Off" force the look regardless of platform.
  const effectsOptions: {
    value: VisualEffectsSetting;
    label: string;
    hint: string;
  }[] = [
    {
      value: 'auto',
      label: 'Auto (platform default)',
      hint: 'Flat on Linux WebKit, rich elsewhere',
    },
    { value: 'on', label: 'On', hint: 'Always show the rich look' },
    { value: 'off', label: 'Off', hint: 'Always use the flat look' },
  ];

  return (
    <section className="w-full" data-testid="appearance-section">
      <h3 className="mb-1 text-secondary font-semibold text-fg">Appearance</h3>
      <p className="mb-4 text-caption text-fg-muted">
        Choose the theme used across the app. <span className="font-medium">System</span>{' '}
        follows your operating system&apos;s color-scheme preference and updates
        live when it changes.
      </p>
      <div
        role="radiogroup"
        aria-labelledby="appearance-section-heading"
        className="flex flex-col gap-2 rounded-lg border border-border-default bg-surface-elevated p-3"
        data-testid="appearance-theme-options"
      >
        <span id="appearance-section-heading" className="sr-only">
          Theme preference
        </span>
        {options.map((option) => {
          const selected = preference === option.value;
          return (
            <label
              key={option.value}
              className={cn(
                'flex cursor-pointer items-center gap-3 rounded border px-3 py-2 text-secondary transition',
                selected
                  ? 'border-accent bg-accent/10 text-fg ring-1 ring-accent/30'
                  : 'border-border-default text-fg hover:bg-surface',
              )}
              data-testid={`appearance-option-${option.value}`}
            >
              <input
                type="radio"
                name="appearance-theme"
                value={option.value}
                checked={selected}
                onChange={() => setPreference(option.value)}
                className="h-3.5 w-3.5 accent-accent"
              />
              <span className="flex flex-1 flex-col">
                <span className="font-medium">{option.label}</span>
                <span className="text-caption text-fg-muted">{option.hint}</span>
              </span>
            </label>
          );
        })}
      </div>

      <h4 className="mb-1 mt-6 text-secondary font-semibold text-fg">
        Visual effects
      </h4>
      <p className="mb-4 text-caption text-fg-muted">
        Card shadows and the timeline landing flash. These are cheap on most
        browsers but cost some (notably Linux WebKit) a repaint that reads as
        lag; <span className="font-medium">Auto</span> keeps the rich look
        everywhere except where it hurts.
      </p>
      <div
        role="radiogroup"
        aria-labelledby="appearance-effects-heading"
        className="flex flex-col gap-2 rounded-lg border border-border-default bg-surface-elevated p-3"
        data-testid="appearance-effects-options"
      >
        <span id="appearance-effects-heading" className="sr-only">
          Visual effects
        </span>
        {effectsOptions.map((option) => {
          const selected = visualEffects === option.value;
          return (
            <label
              key={option.value}
              className={cn(
                'flex cursor-pointer items-center gap-3 rounded border px-3 py-2 text-secondary transition',
                selected
                  ? 'border-accent bg-accent/10 text-fg ring-1 ring-accent/30'
                  : 'border-border-default text-fg hover:bg-surface',
              )}
              data-testid={`appearance-effects-option-${option.value}`}
            >
              <input
                type="radio"
                name="appearance-effects"
                value={option.value}
                checked={selected}
                onChange={() => setVisualEffects(option.value)}
                className="h-3.5 w-3.5 accent-accent"
              />
              <span className="flex flex-1 flex-col">
                <span className="font-medium">{option.label}</span>
                <span className="text-caption text-fg-muted">{option.hint}</span>
              </span>
            </label>
          );
        })}
      </div>
    </section>
  );
}

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
function DefaultProviderSection() {
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

interface LaunchOptionRowProps {
  option: LaunchOption;
  onToggleDefault: (next: boolean) => void;
  toggling: boolean;
  onDelete: () => void;
  deleting: boolean;
}

function LaunchOptionRow({
  option,
  onToggleDefault,
  toggling,
  onDelete,
  deleting,
}: LaunchOptionRowProps) {
  return (
    <li className="flex items-center justify-between gap-3 rounded-lg border border-border-default px-3 py-2">
      {/* No provider name here: the list is already scoped to one, so a
          per-row repeat is noise. */}
      <div className="min-w-0">
        {option.label && (
          <div className="truncate text-caption font-medium text-fg-muted">
            {option.label}
          </div>
        )}
        <div className="truncate font-mono text-code text-fg">
          <span>{option.name}</span>
          {option.value !== null && (
            <span className="text-fg-muted"> {option.value}</span>
          )}
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-3">
        <label className="flex items-center gap-1.5 text-caption text-fg-muted">
          <input
            type="checkbox"
            checked={option.default_enabled}
            onChange={(event) => onToggleDefault(event.target.checked)}
            disabled={toggling}
            aria-label={`Enable launch option ${option.name} by default`}
            className="h-3.5 w-3.5"
          />
          Default
        </label>
        <Button
          size="sm"
          variant="ghost"
          onClick={onDelete}
          disabled={deleting}
          aria-label={`Delete launch option ${option.name}`}
        >
          Delete
        </Button>
      </div>
    </li>
  );
}
