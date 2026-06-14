import { useState, type FormEvent } from 'react';
import {
  useCreateLaunchOptionMutation,
  useDeleteLaunchOptionMutation,
  useLaunchOptionsQuery,
} from '@delta/api-client';
import type { LaunchOption } from '@delta/wire-gen';
import { Button, Panel, Spinner } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';

/**
 * The full-pane settings screen: manage the registry of custom `claude` CLI
 * launch options (flat `(label?, name, value?)` flag records). Lists the
 * registered options and lets the user add one (label and value optional, name
 * required) and delete one. Selecting which options to apply when starting a
 * session is a separate concern handled elsewhere.
 *
 * Reachable from the navigator's lower-left settings entry; left via the header
 * Close button (the conversation view then returns).
 */
export function SettingsView() {
  const client = useApiClient();
  const closeSettings = useNavStore((state) => state.closeSettings);

  // The query only runs while this view is mounted (it owns the settings mode).
  const launchOptionsQuery = useLaunchOptionsQuery(client, true);
  const createLaunchOption = useCreateLaunchOptionMutation(client);
  const deleteLaunchOption = useDeleteLaunchOptionMutation(client);

  const [label, setLabel] = useState('');
  const [name, setName] = useState('');
  const [value, setValue] = useState('');

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
      },
      {
        onSuccess: () => {
          setLabel('');
          setName('');
          setValue('');
        },
      },
    );
  };

  return (
    <Panel
      header={
        <div className="flex items-center justify-between gap-2">
          <span className="text-sm font-semibold text-slate-700">
            Launch options
          </span>
          <Button size="sm" variant="ghost" onClick={closeSettings}>
            Close
          </Button>
        </div>
      }
    >
      <div className="mx-auto w-full max-w-2xl px-4 py-4">
        <p className="mb-4 text-xs text-slate-500">
          Register custom <code>claude</code> CLI flags to apply when starting a
          session. <span className="font-medium">Name</span> is the flag (e.g.{' '}
          <code>--permission-mode</code>); <span className="font-medium">value</span>{' '}
          is its argument (e.g. <code>auto</code>) and is optional for valueless
          flags. <span className="font-medium">Label</span> is an optional note.
        </p>

        {/* Add form */}
        <form
          onSubmit={onSubmit}
          className="mb-6 flex flex-col gap-3 rounded-lg border border-slate-200 bg-slate-50 p-3"
          aria-label="Add launch option"
        >
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-slate-600" htmlFor="lo-label">
              Label (optional)
            </label>
            <input
              id="lo-label"
              type="text"
              value={label}
              onChange={(event) => setLabel(event.target.value)}
              placeholder="My plugins"
              className="rounded border border-slate-300 px-2 py-1 text-sm focus:border-indigo-400 focus:outline-none"
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-slate-600" htmlFor="lo-name">
              Name (the flag)
            </label>
            <input
              id="lo-name"
              type="text"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="--permission-mode"
              required
              className="rounded border border-slate-300 px-2 py-1 text-sm focus:border-indigo-400 focus:outline-none"
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-slate-600" htmlFor="lo-value">
              Value (optional)
            </label>
            <input
              id="lo-value"
              type="text"
              value={value}
              onChange={(event) => setValue(event.target.value)}
              placeholder="auto"
              className="rounded border border-slate-300 px-2 py-1 text-sm focus:border-indigo-400 focus:outline-none"
            />
          </div>
          {createLaunchOption.isError && (
            <p className="text-xs text-red-600" role="alert">
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
          <div className="flex flex-col items-center gap-2 py-6 text-sm text-slate-500">
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
          <p className="py-6 text-center text-sm text-slate-400">
            No launch options registered yet.
          </p>
        ) : (
          <ul className="flex flex-col gap-2" data-testid="launch-options-list">
            {options.map((option) => (
              <LaunchOptionRow
                key={option.id}
                option={option}
                onDelete={() => deleteLaunchOption.mutate(option.id)}
                deleting={
                  deleteLaunchOption.isPending &&
                  deleteLaunchOption.variables === option.id
                }
              />
            ))}
          </ul>
        )}
      </div>
    </Panel>
  );
}

interface LaunchOptionRowProps {
  option: LaunchOption;
  onDelete: () => void;
  deleting: boolean;
}

function LaunchOptionRow({ option, onDelete, deleting }: LaunchOptionRowProps) {
  return (
    <li className="flex items-center justify-between gap-3 rounded-lg border border-slate-200 px-3 py-2">
      <div className="min-w-0">
        {option.label && (
          <div className="truncate text-xs font-medium text-slate-500">
            {option.label}
          </div>
        )}
        <div className="truncate font-mono text-sm text-slate-800">
          <span>{option.name}</span>
          {option.value !== null && (
            <span className="text-slate-500"> {option.value}</span>
          )}
        </div>
      </div>
      <Button
        size="sm"
        variant="ghost"
        onClick={onDelete}
        disabled={deleting}
        aria-label={`Delete launch option ${option.name}`}
      >
        Delete
      </Button>
    </li>
  );
}
