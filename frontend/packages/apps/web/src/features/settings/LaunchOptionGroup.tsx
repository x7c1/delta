import { useId, useRef, useState, type ReactNode } from 'react';
import type { AgentProvider, LaunchOption } from '@delta/wire-gen';
import { settingsMutationErrorMessage } from './settingsMutationErrorMessage';

/**
 * How a grouped row's default control behaves: one radio of its group's
 * default radio group instead of an independent checkbox.
 */
export interface DefaultChoice {
  /** The radio `name` every row of the group shares. */
  name: string;
  /** Whether this row is the group's default. */
  checked: boolean;
  /** Make this row the group's default. */
  onChoose: () => void;
}

/**
 * The rows of one choice group of two or more rows — values of one
 * single-valued setting, such as every Claude `--model` row — listed together
 * under the group key, with their `default_enabled` controls as one radio group
 * and an explicit "No default" option first. The server holds one default per
 * group, so switching it is clear-then-set (see `switchGroupDefault`); a legacy
 * group carrying two defaults shows the first one in list order as chosen — the
 * row the composer picker seeds too, unless that row is dangerous (the picker
 * never seeds one).
 *
 * The radios stay enabled while a switch is being saved, and the group shows
 * the user's latest choice at once. A native radio group moves the selection
 * on every arrow key, so disabling it for each in-flight save dropped keyboard
 * focus after one step. Instead the saves are serialized and coalesced: one
 * switch runs at a time, and choices made while it runs only update the target,
 * so when it lands the next switch goes straight to the latest choice. Arrowing
 * across a group therefore saves at most the first step and the last one, never
 * every option passed over, and ends with the option the user stopped on. Once
 * the target stops moving the list is refetched and the server's state takes
 * over the display again. Committing only on a separate confirm action was the
 * alternative; it was rejected because a click on a radio would then no longer
 * be the whole action, unlike every other default control in this section.
 *
 * The dangerous-row rules apply unchanged inside a group, judged against the
 * choice on screen: a dangerous row that is not the shown default has its radio
 * disabled (choosing it would set its default, which is refused), and one that
 * already carries a stale default keeps its disarm hint — "No default" (or
 * another row) is how it is cleared. A failed save reverts the display to the
 * refetched server state and reports the error beside the group.
 */
export function LaunchOptionGroup({
  provider,
  groupKey,
  options,
  onSwitchDefault,
  onSettled,
  error,
  renderRow,
}: {
  provider: AgentProvider;
  groupKey: string;
  options: LaunchOption[];
  /** One clear-then-set from `holders` to `id`; resolves whether it saved. */
  onSwitchDefault: (
    holders: readonly number[],
    id: number | null,
  ) => Promise<boolean>;
  /** Refetch the list once the switching settles. */
  onSettled: () => Promise<unknown>;
  error: unknown;
  renderRow: (option: LaunchOption, defaultChoice: DefaultChoice) => ReactNode;
}) {
  const headingId = useId();
  const name = `launch-option-default-${provider}-${groupKey}`;
  const holder = options.find((option) => option.default_enabled);
  // The choice on screen while switching is under way (`null` once the
  // refetched list shows the server's state again), wrapped so "No default"
  // (`id: null`) is distinguishable from "nothing pending".
  const [pending, setPending] = useState<{ id: number | null } | null>(null);
  // The latest choice, read by the running switch loop between saves, and
  // whether that loop is running. Refs, not state: the loop is one async
  // closure that must see choices made after it started.
  const target = useRef<number | null>(null);
  const running = useRef(false);
  const shownId = pending !== null ? pending.id : (holder?.id ?? null);

  const choose = (id: number | null) => {
    setPending({ id });
    target.current = id;
    if (running.current) {
      return;
    }
    running.current = true;
    // Snapshot of the server's holders at the start; each successful switch
    // leaves exactly its target holding the default.
    let holders = options
      .filter((option) => option.default_enabled)
      .map((option) => option.id);
    void (async () => {
      try {
        for (;;) {
          const next = target.current;
          if (!(await onSwitchDefault(holders, next))) {
            break;
          }
          holders = next === null ? [] : [next];
          if (target.current !== next) {
            continue;
          }
          await onSettled();
          if (target.current === next) {
            return;
          }
        }
        // A refused save: show what the server holds now.
        await onSettled();
      } finally {
        running.current = false;
        setPending(null);
      }
    })();
  };

  return (
    <li
      className="flex flex-col gap-2"
      data-testid={`launch-option-group-${groupKey}`}
    >
      <div
        role="radiogroup"
        aria-labelledby={headingId}
        aria-busy={pending !== null}
        className="flex flex-col gap-2"
      >
        <div className="flex items-center justify-between gap-3 px-1">
          <span
            id={headingId}
            className="truncate font-mono text-code text-fg-muted"
          >
            {groupKey}
          </span>
          <label className="flex shrink-0 items-center gap-1.5 text-caption text-fg-muted">
            <input
              type="radio"
              name={name}
              checked={shownId === null}
              onChange={() => choose(null)}
              aria-label={`No default for ${groupKey}`}
              data-testid={`launch-option-group-${groupKey}-none`}
              className="h-3.5 w-3.5"
            />
            No default
          </label>
        </div>
        <ul className="flex flex-col gap-2 pl-3">
          {options.map((option) =>
            renderRow(option, {
              name,
              checked: shownId === option.id,
              onChoose: () => choose(option.id),
            }),
          )}
        </ul>
      </div>
      {error !== null && (
        <p className="px-1 text-caption text-danger" role="alert">
          {settingsMutationErrorMessage(error, 'Could not change the default.')}
        </p>
      )}
    </li>
  );
}
