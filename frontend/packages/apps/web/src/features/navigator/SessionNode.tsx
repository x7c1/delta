import { useState, type CSSProperties, type Ref } from 'react';
import type { ThreadId } from '@delta/model';
import type { SessionListItem } from '@delta/wire-gen';
import { useSessionThreadsQuery } from '@delta/api-client';
import { Badge, Menu, Spinner, StatusDot, cn } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { threadIsRunning, useLiveStore } from '../../store/liveStore';
import { useNavStore } from '../../store/navStore';
import { formatLocalDateTime } from '../../utils/formatLocalDateTime';
import { ThreadTree } from './ThreadTree';

export interface SessionNodeProps {
  item: SessionListItem;
  isFocused: boolean;
  /**
   * Whether this session has a pending permission request (a tool blocked on a
   * prompt in its terminal). Surfaced as a badge so a request on a non-focused
   * session is discoverable from the list; the actionable notice lives in the
   * session's conversation pane.
   */
  needsPermission?: boolean;
  onFocus: () => void;
  onClose: () => void;
  /**
   * Ref to the row's `<li>`, used by the virtualizer's `measureElement` to read
   * the card's real height (it varies: the focused card expands its thread
   * tree). Omitted when the list is rendered without windowing.
   */
  rowRef?: Ref<HTMLLIElement>;
  /**
   * Virtual-row index, mirrored onto `data-index` so the virtualizer can map a
   * measured element back to its row. Paired with {@link rowRef}.
   */
  index?: number;
  /**
   * Inline style for the row's `<li>`. The virtualizer absolutely positions each
   * row via `transform: translateY(...)`, which it supplies here.
   */
  style?: CSSProperties;
}

/** A short, readable stand-in for a session that has no title yet. */
function sessionLabel(item: SessionListItem): string {
  if (item.session.title) {
    return item.session.title;
  }
  // Show a short slice of the id so distinct sessions remain distinguishable.
  return `session ${item.session.id.slice(0, 8)}`;
}

/**
 * The basename (last path segment) of `path`, or `''` when there is no usable
 * basename (an empty string or `/`). Trailing slashes are stripped first so
 * `/a/b/` resolves to `b`, mirroring `basename` in POSIX shells.
 */
function basename(path: string): string {
  const trimmed = path.replace(/\/+$/, '');
  const slash = trimmed.lastIndexOf('/');
  return slash >= 0 ? trimmed.slice(slash + 1) : trimmed;
}

/**
 * One top-level navigator node: a session, rendered as a card. The card holds a
 * header row — the focus button (a two-line block: line 1 is the open/closed
 * indicator plus the session's *launch-time* local git branch (the primary
 * identifier, right-truncated with the full name on hover; falls back to the
 * session label when the launch directory was not in a git repo); line 2 shows
 * the launch-time repository identity on the left (preferring the backend's
 * short `repository_display_name` label, e.g. `org/repo`, and falling back to
 * the cwd basename — RTL-truncated only on the fallback so a long local path's
 * tail stays visible; omitted entirely when both yield no name) and the
 * last-activity time on the right) plus the kebab actions menu in a
 * fixed-width slot at the right end. The menu always offers `Copy session ID`
 * (useful even for a closed session — copying its id, e.g. to feed
 * `claude --resume`, does not require the session to be running) and
 * additionally exposes `Close` while the session is open. The focused card is
 * lifted with an indigo border, tint, and ring.
 *
 * Every session that has branched into sub-threads shows its {@link ThreadTree}
 * expanded by default — focused or not — so the whole visible list reads as a
 * navigable session → thread tree. Each mounted row fetches its own thread tree;
 * because the list is windowed, that fetch is bounded to the visible window, and
 * it shares the focused session's query key so the two are deduped into one
 * request per session. Clicking a sub-thread in a non-focused session focuses
 * that session and activates the thread, switching the center pane to it.
 */
export function SessionNode({
  item,
  isFocused,
  needsPermission = false,
  onFocus,
  onClose,
  rowRef,
  index,
  style,
}: SessionNodeProps) {
  const client = useApiClient();
  const setFocusedSession = useNavStore((state) => state.setFocusedSession);
  const setActiveThread = useNavStore((state) => state.setActiveThread);
  // Running and unread are THREAD-keyed in the store. The collapsed session row
  // OR-aggregates them over the session's threads (main + every sub-thread): the
  // spinner shows if ANY thread is running, the dot if ANY thread is unread.
  // This preserves the previous session-level row behaviour while keeping the
  // thread-keyed state the single source of truth (the tree below shows the
  // per-thread breakdown). The main thread is included so a turn on main — the
  // thread reached from this card's header — is not invisible.
  const sessionRunningThreads = useLiveStore(
    (state) => state.runningThreads[item.session.id],
  );
  // The session's running subagents fold into per-thread "running" too: a
  // subagent (a BACKGROUND one in particular) keeps its launching thread
  // running until it finishes, even past the launching turn's end. Including it
  // here lights the row spinner and suppresses the `unread && !running` dot
  // while any subagent runs on a thread of this session, so a thread reads as
  // "still working" — not "done while you were away" — until the subagent ends.
  const sessionRunningSubagents = useLiveStore(
    (state) => state.runningSubagents[item.session.id],
  );
  const unreadByThread = useLiveStore((state) => state.unread);
  // The kebab menu's dropdown opens below the trigger, but each windowed row is
  // an absolutely-positioned `transform` stacking context, so the dropdown is
  // painted under the next row's card. While the menu is open, lift this row
  // above its siblings so the dropdown is visible (see the `zIndex` on `<li>`).
  const [menuOpen, setMenuOpen] = useState(false);
  // Fetch this row's thread tree. Mounted only for sessions in the windowed
  // viewport (+overscan), so the number of in-flight thread queries is bounded
  // by the visible window, not the full session list. Shares the focused
  // session's query key, so React Query serves both from one request.
  const threadsQuery = useSessionThreadsQuery(client, item.session.id);
  const threads = threadsQuery.data?.threads;

  const lastActivity = formatLocalDateTime(item.last_activity_at);
  const label = sessionLabel(item);
  // Line 1: the local branch checked out in the launch directory at spawn time,
  // captured once by the backend on `insert_spawning_session`. Falls back to
  // the session label for sessions launched outside a git repo (or that
  // predate the snapshot — older databases store NULL).
  const branchAtLaunch = item.session.branch_at_launch;
  // Line 2 left: the launch-time repository identity, captured at spawn time.
  // Prefer the backend's short `repository_display_name` label (e.g.
  // `org/repo`, normalised from the launch dir's `origin` URL — stable across
  // worktrees of the same clone). When that is `null` (a session launched
  // outside any git repo, or a legacy row that predates this column), fall
  // back to the cwd basename so the line still identifies the working
  // directory. `repoFullPath` carries the full path the basename was derived
  // from, used as the hover tooltip on the fallback path; on the primary
  // path the tooltip carries `repo_root` (or `cwd` when that is also `null`)
  // so the user can still see exactly where the session is running. An empty
  // `repoLabel` means no usable label and the line-2 left span is omitted.
  const repositoryDisplayName = item.session.repository_display_name;
  const repoRoot = item.session.repo_root;
  const cwd = item.session.cwd;
  const repoLabel = repositoryDisplayName ?? basename(cwd);
  const repoTooltip = repositoryDisplayName ? (repoRoot ?? cwd) : cwd;
  // Only the fallback path needs RTL truncation: a long local path is more
  // meaningful from the tail (e.g. `…/projects/delta`), but the primary
  // `org/repo` label is short and reads more naturally left-aligned.
  const repoUsesFallback = repositoryDisplayName === null;
  // Show the sub-thread list only once the session has branched. The main
  // thread itself is never listed (it is reached by clicking this card's
  // header — see NavigatorPane); a session with no sub-threads shows no tree at
  // all. A sub-thread is any thread with a parent.
  const hasSubThreads =
    threads?.some((t) => t.parent_thread_id !== null) ?? false;

  // OR-aggregate running/unread over the session's threads for the collapsed
  // row. The thread ids are main plus every fetched thread; until the tree
  // loads, fall back to main alone so a running/unread main thread still shows.
  const sessionThreadIds: ThreadId[] = threads
    ? threads.map((t) => t.id)
    : [item.main_thread_id];
  const running = sessionThreadIds.some((id) =>
    threadIsRunning(sessionRunningThreads, sessionRunningSubagents, id),
  );
  // The dot is gated off the focused row: while a session is focused the user is
  // viewing it, and activating its threads clears their unread — but a just-
  // focused session may still hold unread on sub-threads not yet visited, which
  // is exactly what the per-thread badges in the tree are for. Mirror the prior
  // row behaviour (no dot on the focused row) and let the tree carry the detail.
  const unread =
    !isFocused && sessionThreadIds.some((id) => (unreadByThread[id] ?? 0) > 0);

  // Selecting a sub-thread switches the center pane to it. Focus the owning
  // session first (a focus switch clears the active thread), then set the
  // active thread — order matters so the activation is not cleared. Re-selecting
  // within the already-focused session is a no-op focus, leaving the active
  // thread set as expected.
  const selectThread = (threadId: ThreadId) => {
    setFocusedSession(item.session.id);
    setActiveThread(threadId);
  };

  return (
    // Horizontal inset (px-2) and the inter-card gap (pb-1.5) live *inside* the
    // measured box: the virtualizer measures `getBoundingClientRect().height`,
    // which excludes margins, so spacing expressed as margins would not be
    // accounted for and rows would overlap. Padding is included, so the gap is
    // preserved under windowing.
    <li
      ref={rowRef}
      data-index={index}
      // Spread the virtualizer's positioning style, then lift this row above its
      // siblings while its menu is open so the dropdown is not covered by the
      // next row's card (sibling rows are z-auto, painting in DOM order).
      style={menuOpen ? { ...style, zIndex: 20 } : style}
      // pb-1.5 is the inter-card gap (baked into each measured row). The first
      // card also needs that gap above it: the windowed rows are absolutely
      // positioned, so a `pt` on the list container is ignored — give the top
      // row a matching pt-1.5 so it is not flush against the panel top.
      className={cn('px-2 pb-1.5', index === 0 && 'pt-1.5')}
    >
      <div
        className={cn(
          'rounded-md border bg-white shadow-md transition-colors',
          isFocused
            ? 'border-indigo-300 bg-indigo-50/70 ring-1 ring-indigo-200'
            : 'border-slate-300 hover:border-slate-400',
        )}
      >
        <div className="flex items-center gap-2 px-2 py-2">
          <button
            type="button"
            onClick={onFocus}
            className="flex min-w-0 flex-1 flex-col gap-0.5 text-left text-sm"
            aria-current={isFocused ? 'true' : undefined}
            data-testid="session-node"
          >
            <span className="flex min-w-0 items-center gap-2">
              <StatusDot
                tone={item.open ? 'green' : 'slate'}
                title={item.open ? 'Open' : 'Closed'}
              />
              {/* Line 1: the *launch-time* local git branch, captured once on
                  spawn and never updated on resume or a later `git checkout`.
                  Distinct from the per-message `git_branch` carried on each
                  transcript line (a per-turn snapshot). Right-truncates: a
                  branch like `feat/some-very-long-name` should keep the
                  meaningful prefix and clip the tail (`feat/some-very…`), not
                  the other way around. Falls back to the session label when no
                  launch branch was recorded. */}
              <span
                className={cn(
                  'min-w-0 truncate text-left',
                  isFocused && 'font-medium text-indigo-800',
                )}
                title={branchAtLaunch ?? label}
                data-testid="session-branch"
              >
                {branchAtLaunch ?? label}
              </span>
              {running && (
                // Compact: the rotating circle alone reads as "processing". The
                // Spinner's glyph is aria-hidden, so pair it with a
                // visually-hidden label for assistive tech.
                <span className="shrink-0" data-testid="session-running">
                  <Spinner />
                  <span className="sr-only">running</span>
                </span>
              )}
              {unread && !running && (
                // A static filled dot — deliberately NOT the rotating spinner —
                // marking a turn that completed while this session was in the
                // background. Running takes precedence (a session processing
                // again shows the spinner instead), so a stale dot never sits
                // next to a live spinner. Cleared when the session is focused.
                <span
                  className="shrink-0"
                  data-testid="session-unread"
                  title="Finished while you were away"
                >
                  <span
                    className="block h-2 w-2 rounded-full bg-indigo-500"
                    aria-hidden
                  />
                  <span className="sr-only">unread</span>
                </span>
              )}
              {needsPermission && (
                <span className="shrink-0" data-testid="session-permission-badge">
                  <Badge tone="warning">permission</Badge>
                </span>
              )}
            </span>
            {/* Line 2: the launch-time repository identity on the left and
                the last-activity time on the right. Primary path renders the
                backend's short `repository_display_name` label left-aligned;
                fallback path renders the cwd basename RTL-truncated so a
                long local path keeps its meaningful tail (e.g.
                `…/projects/delta`). The repo span is omitted entirely when
                neither yields a usable label. */}
            <span className="flex items-baseline gap-2 text-xs text-slate-400">
              {repoLabel && (
                <span
                  className={cn(
                    'min-w-0 flex-1 truncate text-left',
                    repoUsesFallback && '[direction:rtl]',
                  )}
                  title={repoTooltip}
                  data-testid="session-repo"
                >
                  {repoLabel}
                </span>
              )}
              {lastActivity && (
                <span className="ml-auto shrink-0 tabular-nums">
                  {lastActivity}
                </span>
              )}
            </span>
          </button>
          {/* Fixed-width slot, vertically centered against the two-line block. */}
          <Menu
            label={`Session actions for ${label}`}
            onOpenChange={setMenuOpen}
            items={[
              {
                label: 'Copy session ID',
                onSelect: () => {
                  // Delta runs on localhost, so the page is a secure context and
                  // clipboard permission is granted by default; fire-and-forget
                  // is fine for local dogfooding. If we ever want toast feedback
                  // on failure, add it in a follow-up.
                  void navigator.clipboard.writeText(item.session.id);
                },
              },
              ...(item.open
                ? [
                    {
                      label: 'Close',
                      onSelect: onClose,
                      tone: 'danger' as const,
                    },
                  ]
                : []),
            ]}
          />
        </div>

        {hasSubThreads && threads && (
          <div
            className={cn(
              'border-t px-2 py-1.5',
              isFocused ? 'border-indigo-200' : 'border-slate-200',
            )}
          >
            <ThreadTree
              threads={threads}
              runningThreads={sessionRunningThreads}
              runningSubagents={sessionRunningSubagents}
              onSelectThread={selectThread}
            />
          </div>
        )}
      </div>
    </li>
  );
}
