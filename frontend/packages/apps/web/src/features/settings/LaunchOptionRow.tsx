import type { LaunchOption } from '@delta/wire-gen';
import { Badge, Button, cn } from '@delta/ui-kit';
import {
  DANGEROUS_OPTION_DISARM_HINT,
  DANGEROUS_OPTION_HINT,
  DangerousBadge,
} from '../../launchOptions';
import type { DefaultChoice } from './LaunchOptionGroup';

interface LaunchOptionRowProps {
  option: LaunchOption;
  onToggleDefault: (next: boolean) => void;
  toggling: boolean;
  onDelete: () => void;
  deleting: boolean;
  onDuplicate: () => void;
  /**
   * Set for a row of a choice group: its default control becomes one radio of
   * the group's default radio group rather than an independent checkbox.
   */
  defaultChoice?: DefaultChoice;
}

/**
 * One row of the launch-option registry.
 *
 * Two things distinguish a row Delta ships (`builtin`): a badge, and no delete
 * control at all. Omitted rather than disabled — a button that cannot be pressed
 * only invites the question why — and the server refuses the call anyway with a
 * `409`. The `default_enabled` checkbox stays live on those rows: ticking a
 * shipped option is the point of shipping it.
 *
 * A row the server flags `dangerous` — one that switches the agent's own safety
 * mechanism off — carries a warning badge and has its `default_enabled` checkbox
 * **disabled**, because turning that flag on is the one write the server refuses
 * on such a row (`400 launch_option_rejected`), and offering a control that can
 * only fail is worse than not offering it. Disabled rather than omitted, unlike
 * the delete button on a shipped row: this control sits in a column every other
 * row fills, so removing it would read as a rendering gap rather than a rule —
 * its `title` says which rule. The row is otherwise ordinary: still deletable,
 * and still selectable per session from the composer.
 *
 * The one dangerous row whose control stays live is one that *already* says
 * `default_enabled`, which a row registered before this rule can. Clearing the
 * flag is the write the server always accepts, and it is the only way to get rid
 * of a stored default the picker now ignores — locking the checkbox shut there
 * would leave deleting the row as the sole way out. Such a row also says beside
 * the tick that it is no longer pre-checked: the tick alone would promise
 * something that stopped being true.
 *
 * A row of a choice group (see {@link LaunchOptionGroup}) renders its default
 * control as one radio of the group's radio group instead of a checkbox; the
 * rules above apply to it unchanged.
 *
 * Every row shows its value in full, wrapped, however long it is: saying what
 * the row will pass to the agent is the row's whole purpose, so there is nothing
 * to gain by putting that behind a click. It stays selectable, so a registered
 * JSON `config` value can be read and copied straight out of the row.
 */
export function LaunchOptionRow({
  option,
  onToggleDefault,
  toggling,
  onDelete,
  deleting,
  onDuplicate,
  defaultChoice,
}: LaunchOptionRowProps) {
  // Only *setting* the flag is refused, so the control is locked only where
  // ticking it is what it would do (see this component's doc). In a group that
  // is judged against the choice on screen, which runs ahead of the server
  // while a switch is saved: once the user has moved off a stale dangerous
  // default, choosing it again would be a set.
  const defaultLocked =
    option.dangerous &&
    !(defaultChoice ? defaultChoice.checked : option.default_enabled);
  // Inside a group the siblings share one `name`, so the control's label adds
  // the value to say which row it is.
  const described =
    defaultChoice && option.value !== null
      ? `${option.name} ${option.value}`
      : option.name;
  return (
    <li
      className="flex flex-col gap-2 rounded-lg border border-border-default px-3 py-2"
      data-testid={`launch-option-row-${option.id}`}
    >
      <div className="flex items-center justify-between gap-3">
        {/* No provider name here: the list is already scoped to one, so a
            per-row repeat is noise. */}
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-2">
            {option.label && (
              <span className="truncate text-caption font-medium text-fg-muted">
                {option.label}
              </span>
            )}
            {/* Next to the name rather than the label, because a shipped row is
                not guaranteed a label and the badge must never have nothing to
                hang off. */}
            {option.builtin && (
              <Badge
                tone="info"
                title="Shipped with Delta: its name and value come from Delta, and it cannot be deleted."
                className="shrink-0"
              >
                Built-in
              </Badge>
            )}
            {/* Beside the Built-in badge, for the same reason: a dangerous row
                is not guaranteed a label either. Both can appear at once. */}
            {option.dangerous && <DangerousBadge />}
          </div>
          <div
            className="truncate font-mono text-code text-fg"
            title={option.name}
          >
            {option.name}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-3">
          <label
            className={cn(
              'flex items-center gap-1.5 text-caption text-fg-muted',
              defaultLocked && 'cursor-not-allowed',
            )}
            title={
              option.dangerous
                ? defaultLocked
                  ? DANGEROUS_OPTION_HINT
                  : DANGEROUS_OPTION_DISARM_HINT
                : undefined
            }
          >
            {defaultChoice ? (
              <input
                type="radio"
                name={defaultChoice.name}
                checked={defaultChoice.checked}
                onChange={defaultChoice.onChoose}
                disabled={defaultLocked}
                aria-label={`Enable launch option ${described} by default`}
                className="h-3.5 w-3.5"
              />
            ) : (
              <input
                type="checkbox"
                checked={option.default_enabled}
                onChange={(event) => onToggleDefault(event.target.checked)}
                disabled={toggling || defaultLocked}
                aria-label={`Enable launch option ${described} by default`}
                className="h-3.5 w-3.5"
              />
            )}
            Default
            {/* Without this the `title` is the only place saying the tick is
                inert (see this component's doc); the wording negates what the
                add form's own checkbox promises. */}
            {option.dangerous && option.default_enabled && (
              <span className="text-warning">(no longer pre-checked)</span>
            )}
          </label>
          <Button
            size="sm"
            variant="ghost"
            onClick={onDuplicate}
            aria-label={`Duplicate launch option ${option.name}`}
          >
            Duplicate
          </Button>
          {/* Omitted, not disabled, on a row Delta ships. */}
          {!option.builtin && (
            <Button
              size="sm"
              variant="ghost"
              onClick={onDelete}
              disabled={deleting}
              aria-label={`Delete launch option ${option.name}`}
            >
              Delete
            </Button>
          )}
        </div>
      </div>
      {option.value !== null && (
        // Wrapped and selectable, with its own scroll so a very long value
        // cannot make one row dominate the list. `break-all` because a JSON
        // blob or a path has no spaces to wrap at.
        <pre
          className="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded bg-surface-sunken px-2 py-1 font-mono text-code text-fg"
          data-testid={`launch-option-value-${option.id}`}
        >
          {option.value}
        </pre>
      )}
    </li>
  );
}
