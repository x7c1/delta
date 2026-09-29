import { Badge } from '@delta/ui-kit';
import type { LaunchOption } from '@delta/wire-gen';

/**
 * Shared vocabulary for the verdicts the server attaches to a launch option:
 * `dangerous` — one that switches the agent's own safety mechanism off
 * (Claude's `--dangerously-skip-permissions`, a Codex `danger-full-access`
 * sandbox) — and `choice_group`, the exclusive group a single-valued option's
 * rows form.
 *
 * At app root rather than inside a feature because both surfaces that show such
 * a row need the same words: the settings registry (which marks it and refuses
 * to switch its default on) and the composer picker (which marks it, never
 * pre-checks it, and warns when it is selected). Written once so the two cannot
 * describe the same rule differently.
 *
 * The verdict itself is never computed here. `dangerous` is derived server-side
 * from the provider's own vocabulary and arrives on the wire, so the browser
 * never has to know which spellings mean "stop asking".
 */

/**
 * One entry of a launch-option list as both surfaces render it: an independent
 * option, or the rows of one choice group.
 */
export type LaunchOptionEntry =
  | { kind: 'option'; option: LaunchOption }
  | { kind: 'group'; key: string; options: LaunchOption[] };

/**
 * Partition a provider's rows into entries: two or more rows sharing a
 * non-null `choice_group` become one group entry, placed where its first row
 * sits in the list; every other row is its own entry. Rows inside a group keep
 * list order.
 *
 * A group holding a single row is emitted as an ordinary option entry in its
 * list position, so both surfaces render it exactly like an ungrouped row (a
 * plain checkbox). Exclusivity only becomes visible to the user once a group
 * has a second row, and a radio group with one real choice beside "Agent
 * default" is strictly worse than a checkbox. The row keeps its
 * `choice_group`: the server still refuses a second default or selection in
 * the group once a sibling is registered, and the next list fetch then renders
 * the pair as a group.
 *
 * Grouped by `choice_group` and never by `name`, in both the composer picker
 * and the settings registry: which rows are values of one single-valued
 * setting is the server's verdict, derived from the provider's vocabulary, so
 * the browser never learns the rule — the same reason `dangerous` arrives as a
 * verdict rather than as spellings.
 */
export function partitionByChoiceGroup(
  options: LaunchOption[],
): LaunchOptionEntry[] {
  const entries: LaunchOptionEntry[] = [];
  const groups = new Map<string, LaunchOption[]>();
  for (const option of options) {
    const key = option.choice_group;
    if (key === null) {
      entries.push({ kind: 'option', option });
      continue;
    }
    const existing = groups.get(key);
    if (existing) {
      existing.push(option);
      continue;
    }
    const members = [option];
    groups.set(key, members);
    entries.push({ kind: 'group', key, options: members });
  }
  return entries.map((entry): LaunchOptionEntry =>
    entry.kind === 'group' && entry.options.length < 2
      ? { kind: 'option', option: entry.options[0] }
      : entry,
  );
}

/**
 * How a dangerous option is marked wherever one is listed. Not exported: every
 * surface renders it through {@link DangerousBadge}, so the word itself has one
 * reader.
 */
const DANGEROUS_BADGE_LABEL = 'Dangerous';

/** The tooltip both surfaces hang off that marker. */
export const DANGEROUS_OPTION_HINT =
  "This option turns off the agent's own safety mechanism, so it can never be enabled by default — select it per session, deliberately.";

/**
 * The tooltip for the one dangerous row whose default control is still live: a
 * row that already says `default_enabled` because it was registered before the
 * rule existed.
 *
 * The server refuses to *set* the flag on such a row but always accepts clearing
 * it, so unticking is how the row is disarmed — and if the control were locked
 * shut here the only way out would be deleting the row.
 */
export const DANGEROUS_OPTION_DISARM_HINT =
  "This option turns off the agent's own safety mechanism and was enabled by default before that was disallowed. It is no longer pre-checked when starting a session; untick this to clear the setting for good.";

/** The marker shown beside a dangerous option's name. */
export function DangerousBadge() {
  return (
    <Badge tone="warning" title={DANGEROUS_OPTION_HINT} className="shrink-0">
      {DANGEROUS_BADGE_LABEL}
    </Badge>
  );
}
