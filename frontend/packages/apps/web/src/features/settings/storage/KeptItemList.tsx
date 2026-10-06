import type { KeptItem } from '@delta/wire-gen';

/** What a kept item's reason says, after "because". */
const KEEP_REASON_TEXT: Record<KeptItem['reason'], string> = {
  dirty: 'it has uncommitted or untracked files',
  unmerged: 'it is not merged',
  not_created_by_delta: 'Delta did not create it',
  worktree_kept: 'its worktree was kept',
  in_use_by_another_session: 'another session works in it',
  not_registered: 'git does not know it as a worktree',
  failed: 'removing it failed',
};

const KIND_TEXT: Record<KeptItem['kind'], string> = {
  worktree: 'Worktree',
  branch: 'Branch',
  trust_entry: 'Trust entry for',
};

/** A reason with the server's detail appended, when it carried one. */
export function withDetail(text: string, detail: string | null): string {
  return detail ? `${text}: ${detail}` : text;
}

/**
 * The worktrees, branches and trust entries a removal kept on disk, each with
 * why — the `kept` list both the bulk session removal and the erase answer
 * with.
 */
export function KeptItemList({ kept, testId }: { kept: KeptItem[]; testId: string }) {
  return (
    <ul className="flex flex-col gap-0.5 text-fg-muted" data-testid={testId}>
      {kept.map((item) => (
        <li key={`${item.session_id ?? ''}:${item.kind}:${item.target}`}>
          {KIND_TEXT[item.kind]} <span className="font-mono">{item.target}</span>,
          because {withDetail(KEEP_REASON_TEXT[item.reason], item.detail)}.
        </li>
      ))}
    </ul>
  );
}
