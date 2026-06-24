/**
 * Placeholder for the Pull Request tab. Phase B does not populate the list;
 * Phase C will spawn `gh search prs` to back the reviewer / author lenses.
 *
 * The empty state explains what is coming and points at `gh auth login`,
 * which is the prerequisite for the eventual list to materialise — so users
 * can fix their environment now and have working PR data ready when the
 * follow-up ships.
 */
export function PRTab() {
  return (
    <div
      className="rounded-md border border-dashed border-slate-300 bg-slate-50 px-4 py-6 text-sm text-slate-600"
      data-testid="new-session-pr-tab"
    >
      <p className="font-medium text-slate-700">
        Pull requests will appear here.
      </p>
      <p className="mt-2 text-xs text-slate-500">
        Phase C will list your reviewer-requested and authored PRs from
        GitHub. To get ready, sign in with{' '}
        <code className="rounded bg-slate-200 px-1 py-0.5 font-mono text-[0.7rem] text-slate-700">
          gh auth login
        </code>{' '}
        in your terminal.
      </p>
    </div>
  );
}
