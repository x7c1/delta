import { Spinner } from '@delta/ui-kit';
import type { SubagentActivity } from '../../store/liveStore';

export interface SubagentRunningIndicatorProps {
  /** The subagents running in the focused session's turn, oldest first. */
  subagents: SubagentActivity[];
}

/** A readable label for one running subagent (its description, else its type). */
function subagentLabel(subagent: SubagentActivity): string {
  return (
    subagent.description ?? subagent.subagentType ?? 'subagent'
  );
}

/**
 * A small "subagent running" indicator shown at the conversation tail while one
 * or more subagents (the `Agent`/`Task` tool) work.
 *
 * A subagent runs in its own transcript that Delta never tails, so nothing else
 * appears in the conversation pane while it works — without this the user would
 * see a silent, seemingly-idle conversation during a long subagent run. Each
 * running subagent is listed with its description (falling back to its type),
 * so multiple concurrent subagents are all visible. Renders nothing when none
 * is running.
 */
export function SubagentRunningIndicator({
  subagents,
}: SubagentRunningIndicatorProps) {
  if (subagents.length === 0) {
    return null;
  }
  return (
    <div className="px-3 pt-1.5 pb-2" data-testid="subagent-running-indicator">
      <div className="rounded-lg border border-sky-200 bg-sky-50 px-3 py-2 text-sm text-sky-900">
        <div className="flex items-center gap-2">
          <Spinner />
          <span className="font-medium">
            {subagents.length > 1
              ? `${subagents.length} subagents running`
              : 'Subagent running'}
          </span>
        </div>
        <ul className="mt-1 space-y-0.5">
          {subagents.map((subagent) => (
            <li
              key={subagent.toolUseId}
              className="truncate text-xs text-sky-800"
              title={subagentLabel(subagent)}
            >
              {subagentLabel(subagent)}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
