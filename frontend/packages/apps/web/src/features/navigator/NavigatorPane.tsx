import { useEffect, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { RateLimitWindow, SessionListItem } from '@delta/wire-gen';
import {
  Button,
  cn,
  Meter,
  Panel,
  Spinner,
  StatusDot,
  type DotTone,
} from '@delta/ui-kit';
import {
  useCloseSessionMutation,
  type ConnectionStatus,
} from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { noticeOf, useLiveStore } from '../../store/liveStore';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
import { SessionNode } from './SessionNode';
import {
  computeBudgetLinePercentage,
  formatResetCountdown,
} from './rateLimitReset';

/**
 * Estimated height of a collapsed session card, in pixels. Only a seed for the
 * virtualizer's spacer math: each rendered row reports its true height back via
 * `measureElement` (ResizeObserver-backed), so the focused card expanding its
 * thread tree is measured rather than assumed. The session list's scrollbar is
 * hidden (`scrollbar-none`), so the brief estimate-vs-actual jitter that the
 * spacer approach can cause is not visible.
 */
const ESTIMATED_SESSION_NODE_HEIGHT = 64;

/**
 * Extra rows rendered above and below the visible window. A small buffer keeps
 * scrolling smooth (rows exist before they scroll into view) while still
 * recycling off-screen DOM so the node count stays bounded regardless of how
 * many sessions have loaded.
 */
const SESSION_OVERSCAN = 8;

export interface NavigatorPaneProps {
  /** The loaded sessions so far, ordered most-recently-active first. */
  sessions: SessionListItem[];
  /** Whether more session pages remain to be fetched. */
  hasMoreSessions: boolean;
  /** Whether the next session page is currently in flight. */
  isLoadingMoreSessions: boolean;
  /** Request the next session page (cursor-paginated). */
  onLoadMoreSessions: () => void;
}

const CONNECTION_TONE: Record<ConnectionStatus, DotTone> = {
  connecting: 'amber',
  open: 'green',
  closed: 'red',
};

const CONNECTION_TITLE: Record<ConnectionStatus, string> = {
  connecting: 'Server connection: connecting…',
  open: 'Server connection: connected',
  closed: 'Server connection: disconnected',
};

// Short status word shown beside the dot in the footer, so the indicator reads
// as the live connection state rather than a static brand label.
const CONNECTION_LABEL: Record<ConnectionStatus, string> = {
  connecting: 'Connecting…',
  open: 'Connected',
  closed: 'Disconnected',
};

/**
 * Plus glyph marking the "New session" header action so it reads as an
 * affordance to create something. Decorative — always `aria-hidden`, so the
 * button's accessible name stays its "New session" label. This file is the
 * only user.
 */
function PlusIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  );
}

/**
 * Gear glyph marking the footer "Settings" entry so it reads as a button.
 * Decorative — always `aria-hidden`, so the button's accessible name stays its
 * "Settings" label. This file is the only user.
 */
function SettingsIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

/**
 * One account-wide rate-limit row in the footer: a window label (`5h` / `7d`),
 * a {@link Meter} bar, the percentage, and a compact relative reset countdown.
 * The two rows share the same neutral dark accent; they are told apart by the
 * `5h` / `7d` label, not colour. The fill is anchored to the right edge of the
 * track and grows leftward (see the `className` passed at the call site), so
 * the bar's right edge represents the moment of reset. The caller hides the row
 * entirely when its window is absent, so this only renders a present window; a
 * `null` percentage within a present window reads as 0%.
 *
 * A 1px budget-line marker is overlaid on the bar at the right edge of the
 * current bucket (the window split into `bucketCount` equal parts — 7 days for
 * `7d`, 5 hours for `5h`). It sits at distance `budgetLinePercentage` from the
 * right and steps one bucket to the left each time the clock crosses a
 * boundary: right after a reset the line is at `1 / bucketCount` from the
 * right (the first bucket's share of the window is fair game); on the final
 * bucket the line reaches the left edge (the whole window is fair game). The
 * invariant is intuitive: fill INSIDE (right of) the line means consumption is
 * within this bucket's share; fill CROSSING (left of) the line means
 * consumption is running ahead of the per-bucket pace. This lets you read
 * "how much can I still spend today" at a glance — no numbers.
 *
 * The numeric percentage cell reserves a `min-width` and right-aligns its
 * text so the trailing `↻` reset countdown column lines up across the 5h /
 * 7d rows without needing to zero-pad shorter numbers into a `021%` form.
 *
 * The row uses a monospace family on purpose so EVERY character — digits, `%`,
 * the `↻` reset glyph, the letters in `5d04h` / `02h13m`, and any spaces —
 * sits in a fixed-width cell, which is what keeps the two rows' columns
 * aligned. Tabular-figures (`tabular-nums`) alone equalises digit glyphs only,
 * leaving symbols and letters at proportional widths — so it is not sufficient
 * here.
 */
function RateLimitRow({
  label,
  window: rateWindow,
  windowDurationSeconds,
  bucketCount,
  fillClassName,
  meterClassName,
  testId,
}: {
  label: string;
  window: RateLimitWindow;
  windowDurationSeconds: number;
  bucketCount: number;
  fillClassName: string;
  meterClassName?: string;
  testId: string;
}) {
  const percentage = rateWindow.used_percentage ?? 0;
  const reset =
    rateWindow.resets_at !== null
      ? formatResetCountdown(rateWindow.resets_at)
      : null;
  const budgetLinePercentage =
    rateWindow.resets_at !== null
      ? computeBudgetLinePercentage(
          rateWindow.resets_at,
          windowDurationSeconds,
          bucketCount,
        )
      : null;
  return (
    <div
      className="flex items-center gap-1.5 font-mono text-xs text-fg-muted"
      data-testid={testId}
    >
      <span className="w-5 shrink-0 text-fg-subtle">{label}</span>
      <div className="relative flex-1 min-w-0">
        <Meter
          value={percentage}
          fillClassName={fillClassName}
          className={meterClassName}
          title={`${label} rate limit: ${Math.round(percentage)}% used`}
        />
        {budgetLinePercentage !== null && (
          <span
            aria-hidden
            className="pointer-events-none absolute inset-y-0 w-px bg-fg"
            style={{ right: `${budgetLinePercentage}%` }}
            data-testid={`${testId}-budget-line`}
          />
        )}
      </div>
      <span
        className="inline-block min-w-[2.5em] shrink-0 text-right"
        data-testid={`${testId}-pct`}
      >
        {Math.round(percentage)}%
      </span>
      {reset !== null && (
        <span
          className="shrink-0 text-fg-subtle"
          data-testid={`${testId}-reset`}
        >
          {`↻ ${reset}`}
        </span>
      )}
    </div>
  );
}

/**
 * The left pane: a session → thread nested tree, plus a "New" affordance and
 * the live connection status. Each session's open/closed state is shown by its
 * status dot, so no separate count is rendered. Per-session state — a pending
 * permission request and an in-flight turn (running) — is surfaced on the
 * owning session's row rather than globally, so it is clear which session it
 * refers to. Top-level nodes are sessions; every session that has branched
 * shows its thread tree expanded (each row fetches its own — see SessionNode).
 */
export function NavigatorPane({
  sessions,
  hasMoreSessions,
  isLoadingMoreSessions,
  onLoadMoreSessions,
}: NavigatorPaneProps) {
  const client = useApiClient();
  const closeSession = useCloseSessionMutation(client);

  // The Panel body is the scroll container; the virtualizer reads its scroll
  // position and viewport height to decide which rows to render.
  const scrollBodyRef = useRef<HTMLDivElement>(null);
  // The default `measureElement` is ResizeObserver-backed: each rendered row
  // reports its real height, so the focused card growing/shrinking its thread
  // tree re-measures instead of corrupting the positions of the rows below it.
  // Rows opt into measurement by attaching `virtualizer.measureElement` as their
  // ref (see below); `estimateSize` is only the spacer seed before measurement.
  //
  // `getItemKey` keys the measurement cache by session id, not by index. The
  // list is recency-ordered, so sending a message bumps that session to the top
  // and reindexes the rest. With the default index key, each row would inherit
  // the cached height of whichever session previously sat at its index — a tall
  // threaded card's height landing on a short collapsed one, leaving gaps (and
  // vice versa, overlaps). Keying by id makes a measured height travel with its
  // session across reorders, so the spacers stay correct.
  const virtualizer = useVirtualizer({
    count: sessions.length,
    getScrollElement: () => scrollBodyRef.current,
    estimateSize: () => ESTIMATED_SESSION_NODE_HEIGHT,
    overscan: SESSION_OVERSCAN,
    getItemKey: (index) => sessions[index].session.id,
  });

  const virtualItems = virtualizer.getVirtualItems();

  // Scroll-triggered page loading, driven off the virtual range rather than a
  // DOM sentinel: once the window reaches the last loaded session, fetch the
  // next page. This replaces the earlier IntersectionObserver trigger, which
  // could not see a sentinel that windowing keeps unmounted until scrolled to.
  useEffect(() => {
    const lastItem = virtualItems[virtualItems.length - 1];
    if (!lastItem) {
      return;
    }
    if (
      lastItem.index >= sessions.length - 1 &&
      hasMoreSessions &&
      !isLoadingMoreSessions
    ) {
      onLoadMoreSessions();
    }
  }, [
    virtualItems,
    sessions.length,
    hasMoreSessions,
    isLoadingMoreSessions,
    onLoadMoreSessions,
  ]);

  const connection = useLiveStore((state) => state.connection);
  // Account-wide rate limits (the latest `status_updated` snapshot, identical
  // across sessions). The footer is the natural home for this app-global state.
  // Each window's row is hidden when the window is absent — a non-Pro/Max
  // account, or before the first API response — rather than shown zeroed.
  const rateLimits = useLiveStore((state) => state.rateLimits);
  // Per-session notices. A pending permission request drives a badge on its
  // session's row (the notice card itself lives in the focused session's
  // conversation pane), so a request on a non-focused session is still
  // discoverable. A dismissed notice keeps its badge: the request is still
  // genuinely awaiting an answer.
  const notices = useLiveStore((state) => state.notices);
  // Running and unread are THREAD-keyed in the store now, so each SessionNode
  // OR-aggregates them over its own threads (and shows them per thread in its
  // tree). The collapsed-row spinner/dot is therefore computed inside the node
  // rather than passed down from here. A running subagent (foreground or
  // background) is folded into that per-thread "running" — see SessionNode —
  // so the row's spinner already covers it; the navigator shows no separate
  // subagent count.
  const focusedSessionId = useNavStore((state) => state.focusedSessionId);
  const settingsOpen = useNavStore((state) => state.settingsOpen);
  const openSettings = useNavStore((state) => state.openSettings);
  const setFocusedSession = useNavStore((state) => state.setFocusedSession);
  const startNewSession = useNavStore((state) => state.startNewSession);
  const setActiveThread = useNavStore((state) => state.setActiveThread);
  const setNewSessionWorkdir = useComposerStore(
    (state) => state.setNewSessionWorkdir,
  );
  const resetNewSessionLaunchOptions = useComposerStore(
    (state) => state.resetNewSessionLaunchOptions,
  );
  const setNewSessionSelectedPrUrl = useComposerStore(
    (state) => state.setNewSessionSelectedPrUrl,
  );
  return (
    <Panel
      className="border-r border-border-default"
      headerClassName="px-2"
      bodyRef={scrollBodyRef}
      // The session list is a side panel; hide its scrollbar entirely (no bar,
      // no reserved column) so it never shows a stray blank strip. It still
      // scrolls via wheel/trackpad. The transcript pane keeps its hover-reveal
      // scrollbar (Panel's default).
      bodyClassName="scrollbar-none"
      header={
        // The header holds the primary action: a full-width "New session" CTA,
        // styled as an outlined button (transparent with a thin border, a faint
        // fill on hover) so it reads clearly as a button while staying lighter
        // than a solid fill. It always (re)starts the new-session flow even when
        // already in that state — changing focus is not enough, the new-session
        // screen now leads with the inline 3-tab picker (PR / Repository /
        // Directory) so no modal is opened from here. Reset any prior selection
        // (directory and launch options) for a clean start. The header padding
        // is set to `px-2` (via `headerClassName`) so the full-width button
        // lines up with the body's 8px content column.
        <Button
          variant="ghost"
          size="sm"
          className="w-full justify-start border border-border-strong text-fg"
          onClick={() => {
            startNewSession();
            setNewSessionWorkdir(null);
            resetNewSessionLaunchOptions();
            setNewSessionSelectedPrUrl(null);
          }}
        >
          <PlusIcon className="h-3.5 w-3.5" />
          New session
        </Button>
      }
      footer={
        // A quiet utility bar, distinct from the primary action up top. The
        // account-wide rate-limit meters (5h / 7d) stack ABOVE the connection
        // row — the footer is the natural home for app-global state — and each is
        // omitted when its window is absent. Below them: the live connection
        // status (dot + a status word like "Connected") on the left, and an
        // icon-only Settings entry on the right (claude.ai-style, opens the
        // settings dialog overlaid on the workspace).
        <div className="flex flex-col gap-1.5">
          {(rateLimits?.fiveHour || rateLimits?.sevenDay) && (
            <div className="flex flex-col gap-1 pt-1.5" data-testid="rate-limits">
              {rateLimits.fiveHour && (
                <RateLimitRow
                  label="5h"
                  window={rateLimits.fiveHour}
                  windowDurationSeconds={5 * 60 * 60}
                  // The 5h window's budget line steps hourly; the 7d window's
                  // steps daily — so bucketCount matches the row's natural unit.
                  bucketCount={5}
                  // Shared neutral accent — the rows are told apart by the label.
                  fillClassName="bg-fg-muted"
                  // `flex justify-end` on the Meter's outer track pushes its
                  // inner fill div to the right edge, so the bar grows leftward
                  // from the reset side without modifying the Meter primitive.
                  meterClassName="flex justify-end"
                  testId="rate-limit-5h"
                />
              )}
              {rateLimits.sevenDay && (
                <RateLimitRow
                  label="7d"
                  window={rateLimits.sevenDay}
                  windowDurationSeconds={7 * 24 * 60 * 60}
                  bucketCount={7}
                  // Shared neutral accent — the rows are told apart by the label.
                  fillClassName="bg-fg-muted"
                  // `flex justify-end` on the Meter's outer track pushes its
                  // inner fill div to the right edge, so the bar grows leftward
                  // from the reset side without modifying the Meter primitive.
                  meterClassName="flex justify-end"
                  testId="rate-limit-7d"
                />
              )}
            </div>
          )}
          <div className="flex items-center justify-between gap-2">
            {/*
              data-connection exposes the live connection state structurally so
              the e2e suites can wait on disconnect/reconnect transitions without
              depending on the dot's color classes or title wording.
            */}
            <span className="inline-flex items-center gap-1.5">
              <span
                className="inline-flex px-1"
                data-testid="connection-indicator"
                data-connection={connection}
              >
                <StatusDot
                  tone={CONNECTION_TONE[connection]}
                  title={CONNECTION_TITLE[connection]}
                />
              </span>
              <span className="text-xs text-fg-muted">
                {CONNECTION_LABEL[connection]}
              </span>
            </span>
            {/*
              Icon-only Settings button: aria-label carries the accessible name
              since the gear glyph has no text, while data-testid and aria-pressed
              keep the existing wiring and the e2e/unit hooks stable.
            */}
            <Button
              variant="ghost"
              size="sm"
              className={cn(
                'px-1.5 -mr-2.5',
                settingsOpen && 'bg-surface-elevated-hover text-fg',
              )}
              data-testid="settings-entry"
              aria-label="Settings"
              aria-pressed={settingsOpen}
              onClick={openSettings}
            >
              <SettingsIcon className="h-4 w-4" />
            </Button>
          </div>
        </div>
      }
    >
      {focusedSessionId === NEW_SESSION_FOCUS && (
        <div
          className="mx-2 mb-1.5 mt-1.5 rounded-lg border border-accent-disabled bg-accent/10 px-2 py-2 text-xs text-accent shadow-sm ring-1 ring-accent-disabled"
          data-testid="new-session-node"
        >
          New session — send the first message to start it.
        </div>
      )}

      {/*
        Windowed session list. The `<ul>` is a spacer sized to the full virtual
        height (getTotalSize); only the rows in the current window (+overscan)
        are mounted, each absolutely positioned by the virtualizer. This bounds
        the live DOM-node count regardless of how many sessions have loaded.
      */}
      <ul
        data-testid="sessions-list"
        className="relative w-full"
        style={{ height: virtualizer.getTotalSize() }}
      >
        {virtualItems.map((virtualRow) => {
          const item = sessions[virtualRow.index];
          return (
            <SessionNode
              key={item.session.id}
              rowRef={virtualizer.measureElement}
              index={virtualRow.index}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                right: 0,
                transform: `translateY(${virtualRow.start}px)`,
              }}
              item={item}
              isFocused={focusedSessionId === item.session.id}
              needsPermission={
                noticeOf(notices, item.session.id, 'permission') !== null
              }
              onFocus={() => {
                setFocusedSession(item.session.id);
                // The main thread is not listed in the tree, so clicking the
                // session card is how you return to it. Always select main —
                // this also covers re-clicking the already-focused session
                // while viewing one of its sub-threads.
                setActiveThread(item.main_thread_id);
              }}
              onClose={() => closeSession.mutate(item.session.id)}
            />
          );
        })}
      </ul>

      {/*
        While the next page is in flight, surface a small loading row below the
        windowed list. The fetch itself is triggered from the virtualizer's
        range (see the effect above), not from this element.
      */}
      {hasMoreSessions && isLoadingMoreSessions && (
        <div
          data-testid="sessions-load-more"
          className="flex justify-center px-3 pb-3 pt-1 text-xs text-fg-subtle"
        >
          <Spinner label="loading more" />
        </div>
      )}
    </Panel>
  );
}
