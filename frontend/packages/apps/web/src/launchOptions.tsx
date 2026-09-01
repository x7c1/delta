import { Badge } from '@delta/ui-kit';

/**
 * Shared vocabulary for a launch option the server flags `dangerous` — one that
 * switches the agent's own safety mechanism off (Claude's
 * `--dangerously-skip-permissions`, a Codex `danger-full-access` sandbox).
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
