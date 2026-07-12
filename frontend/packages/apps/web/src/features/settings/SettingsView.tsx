import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
  type ReactNode,
} from 'react';
import {
  ApiError,
  useAddRepositoryScanRootMutation,
  useCreateLaunchOptionMutation,
  useDeleteLaunchOptionMutation,
  useHomeDirQuery,
  useLaunchOptionsQuery,
  useRemoveRepositoryScanRootMutation,
  useRepositoryScanRootsQuery,
  useUpdateLaunchOptionMutation,
} from '@delta/api-client';
import type { LaunchOption, RepositoryScanRoot } from '@delta/wire-gen';
import { Button, cn, Dialog, Spinner } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useThemeContext } from '../../hooks/themeContext';
import { useNavStore } from '../../store/navStore';
import {
  type SettingsCategoryId,
  useSettingsStore,
} from '../../store/settingsStore';
import { SYSTEM_PREFERENCE, type ThemePreference } from '../../hooks/useTheme';
import { THEMES } from '../../themes/registry';
import { displayPath } from '../../utils/displayPath';
import { WorkdirPickerBody } from '../composer/WorkdirPickerBody';

/**
 * The settings modal: hosts the registry of custom `claude` CLI launch options
 * and the registry of repository scan roots, each a top-level category in a
 * VS Code-style 2-pane layout. The left rail lists categories; the right pane
 * renders the active category's content. The categories are conceptually
 * unrelated (one targets session startup flags, the other where to look for
 * git repos to start sessions in), so they live in separate panes rather than
 * stacked sections — keeping each category's UI undivided by the other.
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
      id: 'scan-roots',
      label: 'Repository scan roots',
      render: (active) => <RepositoryScanRootsSection active={active} />,
    },
    {
      id: 'appearance',
      label: 'Appearance',
      // The Appearance section has no data fetch of its own; the `active`
      // prop is ignored.
      render: () => <AppearanceSection />,
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
      className="max-w-4xl"
      footer={
        <Button variant="ghost" onClick={closeSettings} data-testid="settings-close">
          Close
        </Button>
      }
    >
      <div className="flex min-h-[24rem] w-full gap-4">
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
          className="min-w-0 flex-1 overflow-y-auto"
          data-testid={`settings-panel-${active.id}`}
        >
          {active.render(settingsOpen)}
        </div>
      </div>
    </Dialog>
  );
}

/**
 * Launch options category content: manage the registry of custom `claude` CLI
 * launch options (flat `(label?, name, value?)` flag records). Lists the
 * registered options and lets the user add one (label and value optional,
 * name required) and delete one. Selecting which options to apply when
 * starting a session is a separate concern handled elsewhere.
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

  const options = launchOptionsQuery.data?.launch_options ?? [];
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
      },
      {
        onSuccess: () => {
          setLabel('');
          setName('');
          setValue('');
          setDefaultEnabled(false);
        },
      },
    );
  };

  return (
    <section className="w-full" data-testid="launch-options-section">
      <h3 className="mb-1 text-secondary font-semibold text-fg">Launch options</h3>
      <p className="mb-4 text-caption text-fg-muted">
        Register custom <code>claude</code> CLI flags to apply when starting a
        session. <span className="font-medium">Name</span> is the flag (e.g.{' '}
        <code>--permission-mode</code>); <span className="font-medium">value</span>{' '}
        is its argument (e.g. <code>auto</code>) and is optional for valueless
        flags. <span className="font-medium">Label</span> is an optional note.
      </p>

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
            Name (the flag)
          </label>
          <input
            id="lo-name"
            type="text"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="--permission-mode"
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
            placeholder="auto"
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
      ) : options.length === 0 ? (
        <p className="py-6 text-center text-secondary text-fg-subtle">
          No launch options registered yet.
        </p>
      ) : (
        <ul className="flex flex-col gap-2" data-testid="launch-options-list">
          {options.map((option) => (
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
 * Repository scan roots category content: list of registered parent
 * directories whose direct children every Repository tab refetch probes for
 * git clones, plus a one-shot picker to register a new one. Drives the same
 * backend the New session screen consults, so adding a scan root here
 * surfaces previously-hidden clones on the very next refetch.
 *
 * `active` mirrors the dialog's `settingsOpen` AND the category being the
 * visible one, so the query only runs while the section is mounted.
 */
function RepositoryScanRootsSection({ active }: { active: boolean }) {
  const client = useApiClient();
  const scanRootsQuery = useRepositoryScanRootsQuery(client, active);
  const addScanRoot = useAddRepositoryScanRootMutation(client);
  const removeScanRoot = useRemoveRepositoryScanRootMutation(client);
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
    addScanRoot.error instanceof ApiError &&
    addScanRoot.error.code === 'scan_root_duplicate';

  // React Query's mutation handle is a fresh object every render, so it
  // cannot sit in the effect dependencies (it would re-fire forever). Pull
  // `reset` out and depend on it (a stable function reference within a given
  // QueryClient lifetime), which keeps the effect well-behaved.
  const resetScanRootMutation = addScanRoot.reset;
  useEffect(() => {
    if (!pickerOpen) {
      setCandidate(null);
      resetScanRootMutation();
    }
  }, [pickerOpen, resetScanRootMutation]);

  const submit = () => {
    if (candidate === null) {
      return;
    }
    addScanRoot.mutate(
      { path: candidate },
      {
        onSuccess: () => setPickerOpen(false),
      },
    );
  };

  const scanRoots = scanRootsQuery.data?.scan_roots ?? [];

  return (
    <section className="space-y-3" data-testid="scan-roots-section">
      <div>
        <h3 className="mb-1 text-secondary font-semibold text-fg">
          Repository scan roots
        </h3>
        <p className="text-caption text-fg-muted">
          Delta scans the direct children of each path below for git
          repositories so you can pick them from the Repository tab without
          having to start a session there first.
        </p>
      </div>

      {scanRootsQuery.isPending ? (
        <div className="flex justify-center py-4">
          <Spinner label="loading scan roots" />
        </div>
      ) : scanRootsQuery.isError ? (
        <div className="flex flex-col items-center gap-2 py-4 text-secondary text-fg-muted">
          <p>Could not load scan roots.</p>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => scanRootsQuery.refetch()}
          >
            Retry
          </Button>
        </div>
      ) : scanRoots.length === 0 ? (
        <p className="py-3 text-center text-secondary text-fg-subtle">
          No scan roots registered yet.
        </p>
      ) : (
        <ul className="flex flex-col gap-2" data-testid="scan-roots-list">
          {scanRoots.map((root) => (
            <ScanRootRow
              key={root.path}
              root={root}
              home={home}
              onRemove={() => removeScanRoot.mutate(root.path)}
              removing={
                removeScanRoot.isPending && removeScanRoot.variables === root.path
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
          data-testid="add-scan-root"
        >
          Add scan root…
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
              disabled={candidate === null || addScanRoot.isPending}
              data-testid="scan-root-confirm"
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
              data-testid="scan-root-duplicate"
            >
              Already registered.
            </p>
          )}
          {addScanRoot.isError && !duplicate && (
            <p className="text-caption text-danger" role="alert">
              Could not add the scan root. Please try again.
            </p>
          )}
        </div>
      </Dialog>
    </section>
  );
}

interface ScanRootRowProps {
  root: RepositoryScanRoot;
  home: string | null;
  onRemove: () => void;
  removing: boolean;
}

function ScanRootRow({ root, home, onRemove, removing }: ScanRootRowProps) {
  return (
    <li className="flex items-center justify-between gap-3 rounded-lg border border-border-default px-3 py-2">
      <span
        className="truncate font-mono text-secondary text-fg"
        title={root.path}
      >
        {displayPath(root.path, home)}
      </span>
      <Button
        size="sm"
        variant="ghost"
        onClick={onRemove}
        disabled={removing}
        aria-label={`Remove scan root ${root.path}`}
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
      <div className="min-w-0">
        {option.label && (
          <div className="truncate text-caption font-medium text-fg-muted">
            {option.label}
          </div>
        )}
        <div className="truncate font-mono text-secondary text-fg">
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
