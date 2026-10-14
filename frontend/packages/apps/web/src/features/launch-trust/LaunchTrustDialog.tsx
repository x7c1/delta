import { Button, Dialog } from '@delta/ui-kit';
import type { LaunchTrustResponse } from '@delta/wire-gen';
import { useLaunchTrustStore } from './launchTrustStore';

/** The sentences saying why this launch is being asked about. */
function reasons(check: LaunchTrustResponse): string[] {
  const lines: string[] = [];
  if (check.repository_untrusted) {
    lines.push(
      'This repository has not been trusted in Claude Code yet. Launching will trust it, so its agent configuration (hooks, MCP servers, CLAUDE.md) takes effect here and in later sessions.',
    );
  }
  if (check.agent_config_changes.length > 0) {
    lines.push(
      'This branch changes agent configuration compared with the repository you trust. Review the changes below before Claude Code runs with them.',
    );
  }
  return lines;
}

/**
 * The launch-trust confirmation: shown before a new worktree session launches
 * when the server says Claude Code's trust should not be pre-accepted without
 * asking — the repository is not trusted yet, and/or the branch changes agent
 * configuration. It says why, lists every changed path, and shows each one's
 * unified diff (monospace, scrolling within the dialog). *Launch and trust*
 * sends the session bound to the commit shown; *Cancel* (or Esc) sends
 * nothing.
 *
 * Mounted once for the whole app; the question comes from
 * {@link useLaunchTrustStore}.
 */
export function LaunchTrustDialog() {
  const pending = useLaunchTrustStore((state) => state.pending);
  const answer = useLaunchTrustStore((state) => state.answer);
  const check = pending?.check ?? null;

  return (
    <Dialog
      open={check !== null}
      onClose={() => answer(false)}
      title="Trust this launch?"
      className="max-w-3xl"
      footer={
        <>
          <Button
            variant="ghost"
            onClick={() => answer(false)}
            data-testid="launch-trust-cancel"
          >
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={() => answer(true)}
            data-testid="launch-trust-accept"
          >
            Launch and trust
          </Button>
        </>
      }
    >
      {check !== null && (
        <div className="flex flex-col gap-3" data-testid="launch-trust-dialog">
          {reasons(check).map((line) => (
            <p key={line} className="text-secondary text-fg">
              {line}
            </p>
          ))}
          {check.agent_config_changes.length > 0 && (
            <ul
              className="flex flex-col gap-3"
              data-testid="launch-trust-changes"
            >
              {check.agent_config_changes.map((change) => (
                <li key={change.path} className="flex flex-col gap-1">
                  <span
                    className="break-all font-mono text-code font-medium text-fg"
                    data-testid="launch-trust-path"
                  >
                    {change.path}
                  </span>
                  {change.diff.length > 0 && (
                    <pre
                      className="max-h-80 overflow-auto rounded border border-border-default bg-surface-sunken p-2 font-mono text-code text-fg-muted"
                      data-testid="launch-trust-diff"
                    >
                      {change.diff}
                    </pre>
                  )}
                  {change.diff_truncated && (
                    <span className="text-caption text-fg-subtle">
                      {change.diff.length > 0
                        ? 'The diff is too large to show in full.'
                        : 'The diff is too large to show.'}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          )}
          {check.commit !== null && (
            <p className="text-caption text-fg-subtle">
              Commit <span className="font-mono">{check.commit}</span>. If the
              branch moves before the worktree is built, the launch stops
              instead of trusting what you have not seen; retry it to review
              the new content.
            </p>
          )}
        </div>
      )}
    </Dialog>
  );
}
