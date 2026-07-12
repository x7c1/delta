import { useEffect, useRef, useState } from 'react';
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
import { useVersionQuery, type ConnectionStatus } from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { useLiveStore } from '../../store/liveStore';
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

// Short status word shown beside the dot in the footer when either the
// connection is not open or the workspace version has not yet loaded. When
// the socket is open AND the version has resolved, the label swaps to
// `Delta <version>` (see the render site) — the dot itself still carries the
// live connection state, so the label doubles as a build-identity readout in
// the steady state without losing the disconnect/reconnect feedback.
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
 * `7d`, 5 hours for `5h`). It steps one bucket to the left each time the
 * clock crosses a boundary: right after a reset the line is at `1 /
 * bucketCount` from the right (the first bucket's share is fair game); on the
 * final bucket the line reaches the left edge (the whole window is fair
 * game). To keep the hairline crisp regardless of zoom and device-pixel-ratio,
 * the marker is pinned to `left: 0` and driven by `transform: translateX(<integer
 * px>)` — the same trick the thread-timeline playhead uses to avoid the
 * sub-pixel shimmer that fractional `right: NN.NN%` values cause. Its color
 * switches from `bg-fg` to `bg-surface` (the panel background token — the
 * color-negative of the fill's `bg-fg-muted` in every theme) as soon as the
 * fill overtakes it. Overlaying the surface color on the fill maxes out
 * contrast in dark / light / sepia alike, so the over-pace case reads at a
 * glance even where fill and line overlap. The invariant remains: fill INSIDE
 * (right of) the line = within this bucket's share; fill CROSSING (left of)
 * the line = over-pace.
 *
 * The numeric percentage cell is sized to `3ch` — a snug `99%` fit in the
 * monospace tabular column — and right-aligns its text so the trailing `↻`
 * reset countdown column lines up across the 5h / 7d rows for the common
 * 0–99% case without needing to zero-pad shorter numbers into a `021%` form.
 * A `100%` reading lets the cell grow to its natural width, which nudges that
 * row's reset column a few pixels right; that is acceptable given how rare a
 * full-window 100% reading is in practice.
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
  // Track the meter container's live pixel width so the marker's translateX
  // offset can be rounded to an integer — see the docstring above for why the
  // percentage-based `right` positioning was replaced with translateX.
  const trackRef = useRef<HTMLDivElement>(null);
  const [trackWidth, setTrackWidth] = useState(0);
  useEffect(() => {
    const el = trackRef.current;
    if (!el) return;
    const set = () => setTrackWidth(el.clientWidth);
    set();
    const ro = new ResizeObserver(set);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return (
    <div
      className="flex items-center gap-1.5 font-mono text-code text-fg-muted"
      data-testid={testId}
    >
      <span className="w-5 shrink-0 text-fg-subtle">{label}</span>
      <div ref={trackRef} className="relative flex-1 min-w-0">
        <Meter
          value={percentage}
          fillClassName={fillClassName}
          className={meterClassName}
          title={`${label} rate limit: ${Math.round(percentage)}% used`}
        />
        {budgetLinePercentage !== null && trackWidth > 0 && (
          <span
            aria-hidden
            className={`pointer-events-none absolute inset-y-0 left-0 w-px ${
              percentage > budgetLinePercentage ? 'bg-surface' : 'bg-fg'
            }`}
            style={{
              transform: `translateX(${Math.round(
                trackWidth * (1 - budgetLinePercentage / 100),
              )}px)`,
            }}
            data-testid={`${testId}-budget-line`}
          />
        )}
      </div>
      <span
        className="inline-block min-w-[3ch] shrink-0 text-right"
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
  // Delta workspace version. Pre-formatted server-side (`v0.2.1` on release,
  // `v0.2.1+dev.<sha>` on debug — see `crate::version::display_version`), held
  // in-memory only (no localStorage) — the query is cached for the page
  // lifetime and only re-fetched on reload, which is the only path that can
  // change the running server's version.
  //
  // The `Delta ` prefix is UI copy, prepended at render time. Not baked into
  // the backend so the version identifier itself stays free of a marketing
  // string — a future non-navigator surface (e.g. a settings-panel readout)
  // can render it without stripping the prefix back off.
  const versionQuery = useVersionQuery(client);
  const version = versionQuery.data?.version ?? null;

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
  // Per-session notices (a pending permission request driving the row's badge)
  // are read inside each SessionNode with a narrow selector, not subscribed to
  // here: a notice arriving on any session must not re-render the whole pane —
  // and thus every visible row — when only the affected row's badge changes.
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
  const startNewSession = useNavStore((state) => state.startNewSession);
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
              {/*
                Label semantics: in the steady state (connection is `open` AND
                the version has resolved) the label reads `Delta <version>`,
                turning the always-visible connection row into a passive
                build-identity readout. The status dot on its left still
                encodes the live connection state, so a disconnect is not
                silenced by the label swap. Non-`open` states (`connecting` /
                `closed`) keep the previous connection wording so a dropped
                socket still surfaces the "Disconnected" text; `open` with a
                pending or failed version query also falls back to the
                previous `Connected` copy so the row never renders blank or
                broken. `data-testid="connection-label"` gates the unit test
                without depending on the text.

                `font-mono` is applied only in the version-showing branch —
                the version string is a code-like identifier (sha suffix,
                dot-separated build metadata) and the rest of the codebase
                renders such values in mono (paths, sha short refs, PR head
                refs — grep `font-mono` under `packages/apps/web/src`), so
                the steady-state label matches that convention. The connection
                fallback copy stays in the ambient sans typography.
              */}
              <span
                className={cn(
                  'text-caption text-fg-muted',
                  connection === 'open' &&
                    version !== null &&
                    'font-mono text-code',
                )}
                data-testid="connection-label"
              >
                {connection === 'open' && version !== null
                  ? `Delta ${version}`
                  : CONNECTION_LABEL[connection]}
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
          className="mx-2 mb-1.5 mt-1.5 rounded-lg border border-accent-disabled bg-accent/10 px-2 py-2 text-caption text-accent shadow-sm ring-1 ring-accent-disabled"
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
          // Props are kept memo-friendly (stable/primitive): `rowRef` is the
          // virtualizer's stable `measureElement`, `start` is the raw offset the
          // row turns into its own memoized style, and focus/close/permission
          // are handled inside the row rather than via fresh per-render
          // closures. Combined with `memo(SessionNode)`, a scroll commit — or an
          // unrelated pane-level store update — no longer re-renders every
          // visible row, only the rows whose own inputs changed.
          return (
            <SessionNode
              key={item.session.id}
              rowRef={virtualizer.measureElement}
              index={virtualRow.index}
              start={virtualRow.start}
              item={item}
              isFocused={focusedSessionId === item.session.id}
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
          className="flex justify-center px-3 pb-3 pt-1 text-caption text-fg-subtle"
        >
          <Spinner label="loading more" />
        </div>
      )}
    </Panel>
  );
}
