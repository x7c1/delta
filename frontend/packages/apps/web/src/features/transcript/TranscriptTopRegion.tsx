import type { ComponentProps, MutableRefObject, ReactNode } from 'react';
import type { Thread } from '@delta/wire-gen';
import { Breadcrumb } from '@delta/ui-kit';
import { ThreadTimelineOverlay } from './ThreadTimelineOverlay';
import { FLOATING_CARD_CLASS } from './floatingCard';

interface TranscriptTopRegionProps {
  /** Whether the timeline is expanded, which flips the top-region layout. */
  timelineExpanded: boolean;
  /** True for the new-session composer state. */
  newSession: boolean;
  /** All threads of the focused session. */
  threads: Thread[];
  /** The active thread, or null. */
  activeThread: Thread | null;
  /** Whether the active thread is a sub-thread (the breadcrumb shows). */
  isOnSubThread: boolean;
  /** The right pane's reopen button, or null. */
  paneToggleButton: ReactNode;
  /** The breadcrumb's crumbs, main thread first. */
  breadcrumbItems: ComponentProps<typeof Breadcrumb>['items'];
  /** The scrolling transcript body the timeline jumps within. */
  bodyRef: MutableRefObject<HTMLDivElement | null>;
  /** The collapsed breadcrumb card, measured for the top reserve. */
  breadcrumbOverlayRef: MutableRefObject<HTMLDivElement | null>;
  /** The collapsed right-side cluster, measured for the top reserve. */
  rightClusterOverlayRef: MutableRefObject<HTMLDivElement | null>;
  /** The expanded container, measured for the top reserve. */
  expandedContainerRef: MutableRefObject<HTMLDivElement | null>;
}

/**
 * The transcript's top region: the breadcrumb, the timeline and the right
 * pane's reopen button, as two floating cards (collapsed) or one pinned
 * container (expanded).
 */
export function TranscriptTopRegion({
  timelineExpanded,
  newSession,
  threads,
  activeThread,
  isOnSubThread,
  paneToggleButton,
  breadcrumbItems,
  bodyRef,
  breadcrumbOverlayRef,
  rightClusterOverlayRef,
  expandedContainerRef,
}: TranscriptTopRegionProps) {
  // The top region layout splits into two distinct shapes by
  // `timelineExpanded`:
  //
  //   1. COLLAPSED (default): two independent absolute floating cards
  //      sit at the top of the conversation panel —
  //         [breadcrumb]                    [{Thread} {pane toggle}]
  //      The breadcrumb pins to top-left; the {Thread + pane toggle}
  //      cluster pins to top-right. Both share the same `overlay-inset`
  //      top/side offsets so they read as one row even though they are
  //      two boxes, and the conversation shows through the gap between
  //      them — there is NO full-width white bar. Each piece keeps its
  //      own card chrome (a breadcrumb card; the Thread/pane-toggle
  //      pills already carry `bg-surface shadow-md` via
  //      `TIMELINE_TOGGLE_BUTTON_CLASS` / `PANE_TOGGLE_BUTTON_CLASS`)
  //      so the floating elements stay legible against any conversation
  //      content scrolling underneath. The body reserves
  //      `padding-top: var(--delta-top-region-reserve)` equal to the
  //      taller of the two cards, so the first message clears them on
  //      initial paint.
  //
  //   2. EXPANDED: a SINGLE absolute container pinned to the top of
  //      the Panel's body region holds the entire top region —
  //         [expanded timeline card                                  ]
  //         [breadcrumb] [flex-1 spacer]               [{pane toggle}]
  //      The container is `absolute top-0 left-0 right-0 z-20`, so it
  //      anchors to the Panel's relative wrapper (outside the
  //      scrolling body) and STAYS PINNED across conversation scroll.
  //      Inside the container the children use normal flow — the
  //      timeline card on top, the breadcrumb + pane-toggle row directly
  //      underneath — no child carries its own absolute positioning.
  //      Pinning the container — not its children — is what fixes the
  //      v18 regression where the expanded timeline scrolled away with
  //      the conversation after the first scrub, breaking subsequent
  //      scrubs. The body reserves `padding-top` equal to the
  //      container's measured height (same `--delta-top-region-reserve`
  //      mechanism as collapsed) so the first message clears it. No
  //      Thread icon in the under-row — the expanded card itself
  //      replaces it.
  //
  // The Panel's body wrapper (`Panel.tsx`) already carries
  // `position: relative`, so the absolute floating cards / container
  // anchor to the scroll viewport rather than to the scrolling content
  // itself — they stay glued to the top edge while the conversation
  // scrolls underneath. A high z-index (`z-20`) keeps them above any
  // other in-flow content (streaming bubble, chip rows).
  const showTimeline = !newSession && activeThread !== null;
  const showBreadcrumb = isOnSubThread;
  // The single floating breadcrumb card (collapsed state only) — pinned
  // top-left via `top-overlay-inset` / `left-overlay-inset`, with the
  // same card chrome as the in-flow breadcrumb used in the expanded
  // state. The ref drives one half of the body's measured reserve.
  const collapsedBreadcrumbCard = showBreadcrumb && (
    <div
      ref={breadcrumbOverlayRef}
      data-testid="transcript-breadcrumb-overlay"
      className={`${FLOATING_CARD_CLASS} pointer-events-auto absolute left-overlay-inset top-overlay-inset z-20 self-start px-3 py-1.5`}
    >
      <Breadcrumb items={breadcrumbItems} />
    </div>
  );
  // The single floating right-side cluster (collapsed state only) —
  // pinned top-right via `top-overlay-inset` / `right-overlay-inset`,
  // with the Thread toggle and the pane-toggle pill side-by-side inside it.
  // The cluster wrapper carries NO shared background or border on
  // purpose: each pill already has its own white card chrome via
  // `TIMELINE_TOGGLE_BUTTON_CLASS` / `PANE_TOGGLE_BUTTON_CLASS`, so
  // wrapping them in another white bar would re-introduce the v17
  // top-bar look the v18 design retracts.
  const showRightCluster = showTimeline || paneToggleButton;
  const collapsedRightCluster = showRightCluster && (
    <div
      ref={rightClusterOverlayRef}
      data-testid="transcript-top-row"
      data-expanded="false"
      className="pointer-events-auto absolute right-overlay-inset top-overlay-inset z-20 flex items-start gap-2"
    >
      {showTimeline && activeThread !== null && (
        <ThreadTimelineOverlay
          threads={threads}
          activeThreadId={activeThread.id}
          conversationBodyRef={bodyRef}
        />
      )}
      {paneToggleButton}
    </div>
  );
  // The top region itself. Two completely different shapes by
  // `timelineExpanded` (see the comment above for the full rationale):
  //
  //   - collapsed: a transparent, layout-less wrapper
  //     (`display: contents`) hosting two independent absolute floating
  //     cards (each pinned to its own corner of the Panel body region).
  //
  //   - expanded: a SINGLE absolute container pinned to the top of the
  //     Panel body region (`absolute top-0 left-0 right-0 z-20`)
  //     holding the expanded timeline card on top and a single
  //     normal-flow row of breadcrumb + pane toggle underneath it. The
  //     container itself takes no layout space inside the scrolling
  //     body — the body reserves matching `padding-top` from the
  //     ResizeObserver-driven `--delta-top-region-reserve`, mirroring
  //     the collapsed state's mechanism.
  //
  // Neither shape paints a shared white bar across the full top edge,
  // so the v17 look the v18 design retracted is not re-introduced. The
  // outer `transcript-top-region` wrapper carries the testid in both
  // states for tests to locate.
  return timelineExpanded ? (
    showTimeline ? (
      <div
        ref={expandedContainerRef}
        data-testid="transcript-top-region"
        data-expanded="true"
        // Single absolute container pinned to the top of the Panel
        // body region; anchored to Panel's relative wrapper (outside
        // the scrolling body), so it STAYS PINNED across conversation
        // scroll — the v19 fix for the v18 regression. Children use
        // normal flow inside.
        className="pointer-events-auto absolute left-0 right-0 top-0 z-20 flex flex-col gap-2 px-3 pt-3"
      >
        {activeThread !== null && (
          <ThreadTimelineOverlay
            threads={threads}
            activeThreadId={activeThread.id}
            conversationBodyRef={bodyRef}
          />
        )}
        {(showBreadcrumb || paneToggleButton) && (
          <div
            data-testid="transcript-top-row"
            data-expanded="true"
            className="flex items-center gap-2 pb-2"
          >
            {showBreadcrumb ? (
              <div className={`${FLOATING_CARD_CLASS} px-3 py-1.5`}>
                <Breadcrumb items={breadcrumbItems} />
              </div>
            ) : (
              <span />
            )}
            <span className="flex-1" />
            {paneToggleButton}
          </div>
        )}
      </div>
    ) : null
  ) : (
    (showBreadcrumb || showRightCluster) && (
      <div
        data-testid="transcript-top-region"
        data-expanded="false"
        // Layout-less wrapper: zero height in the document flow, just a
        // host for the two absolute children below. The body's measured
        // padding-top reserve (driven by Math.max of the two children's
        // heights) clears space for them above the first message.
        className="contents"
      >
        {collapsedBreadcrumbCard}
        {collapsedRightCluster}
      </div>
    )
  );
}
