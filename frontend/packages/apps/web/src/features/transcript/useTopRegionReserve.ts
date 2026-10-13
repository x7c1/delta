import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import type { Thread } from '@delta/wire-gen';

interface TopRegionReserveArgs {
  /** Whether the timeline is expanded, which flips the top-region layout. */
  timelineExpanded: boolean;
  /** True for the new-session composer state. */
  newSession: boolean;
  /** The active thread, or null. */
  activeThread: Thread | null;
  /** Whether the active thread is a sub-thread (the breadcrumb shows). */
  isOnSubThread: boolean;
  /** The right pane's reopen button, or null. */
  paneToggleButton: ReactNode;
}

/**
 * Measure the top region (the floating breadcrumb and timeline cards, or the
 * pinned expanded container) and derive the `--delta-top-region-reserve` the
 * body's top padding and the articles' scroll margin read. Returns the refs
 * the top-region surfaces bind and the measured reserve.
 */
export function useTopRegionReserve({
  timelineExpanded,
  newSession,
  activeThread,
  isOnSubThread,
  paneToggleButton,
}: TopRegionReserveArgs) {
  // In the COLLAPSED state the top row is rendered as two independent
  // absolute floating cards — the breadcrumb at top-left and the
  // {Thread + pane toggle} cluster at top-right — so the conversation shows
  // through the gap between them rather than being hidden under a full-
  // width white bar. Each card pins itself with `top`/`left` (or
  // `top`/`right`) at the shared `overlay-inset` so they read as one row
  // even though they are two separate boxes. Two things compensate for
  // those cards not occupying layout space: the body reserves
  // `padding-top` equal to the visual row height (the taller of the two
  // cards) so the first message is not hidden under them on initial
  // paint, and `article[data-message-uuid]` carries a matching
  // `scroll-margin-top` (index.css) so a timeline-jump
  // `scrollIntoView({ block: 'start' })` lands the destination article
  // just BELOW the floating row rather than hidden underneath it. Both
  // feeds share one source: `Math.max(breadcrumbHeight,
  // rightClusterHeight)`, measured via the ResizeObservers below and
  // exposed as the `--delta-top-region-reserve` CSS variable.
  //
  // In the EXPANDED state the entire top region — the expanded timeline
  // card AND the row carrying the breadcrumb + pane toggle underneath it —
  // sits inside a SINGLE absolute container pinned to the top of the
  // Panel's body region (`absolute top-0 left-0 right-0 z-20`). The
  // container does NOT scroll with the conversation: it anchors to the
  // Panel's relative wrapper (outside the scrolling body), not to the
  // body's scrolling content. Pinning the container — not its children
  // — is what fixes v18's regression where scrubbing the expanded
  // timeline scrolled the conversation, dragging the timeline itself
  // off-screen so the user could not scrub again after the first jump.
  // Inside the container the children use normal flow (the timeline
  // card on top, the breadcrumb + pane-toggle row underneath); no child
  // carries its own absolute positioning.
  //
  // Like the collapsed state, the expanded container does not occupy
  // layout space inside the body, so the body reserves matching
  // `padding-top` equal to the container's measured height. The
  // ResizeObserver effect below switches its observation targets when
  // `timelineExpanded` flips: collapsed observes the two floating
  // cards' refs and writes their max, expanded observes the single
  // container's ref and writes its height. Both feeds share the same
  // `--delta-top-region-reserve` CSS variable so the body padding-top
  // and the `scroll-margin-top` on `article[data-message-uuid]` (see
  // index.css) track whichever state is live.
  const breadcrumbOverlayRef = useRef<HTMLDivElement | null>(null);
  const rightClusterOverlayRef = useRef<HTMLDivElement | null>(null);
  const expandedContainerRef = useRef<HTMLDivElement | null>(null);
  const [topRegionReserve, setTopRegionReserve] = useState<number | null>(null);

  // Track whichever top-region surface is currently mounted and drive
  // the body's `--delta-top-region-reserve` CSS variable from its
  // measured height. The variable feeds both the body's `padding-top`
  // (so the first message does not render under the pinned region) and
  // the `scroll-margin-top` rule on `article[data-message-uuid]` (see
  // index.css), so a timeline-jump `scrollIntoView({ block: 'start' })`
  // lands the destination article just below the pinned region rather
  // than hidden underneath it.
  //
  // The observation targets switch with `timelineExpanded`:
  //   - collapsed: observe BOTH `breadcrumbOverlayRef` and
  //     `rightClusterOverlayRef` (the two independent absolute floating
  //     cards) and write `max(breadcrumbHeight, rightClusterHeight)` —
  //     the visual row height those two cards form together.
  //   - expanded: observe the single `expandedContainerRef` (the
  //     absolute container pinned to the top of the Panel's body region,
  //     holding the timeline card and the breadcrumb+pane-toggle under-row
  //     in normal flow) and write the container's total height.
  //
  // Re-running the effect on the `timelineExpanded` flip disconnects
  // the previous observer and binds a fresh one to the new state's
  // node(s). The single ResizeObserver instance per render handles its
  // own cleanup; observing nodes that no longer exist is impossible
  // because the JSX for the unmounted state is not rendered. Re-bind
  // also fires when the collapsed cards' presence may have flipped (a
  // new-session ↔ active-thread swap, or the breadcrumb appearing for
  // a sub-thread navigation) so the initial measurement still lands on
  // the new nodes. The observer reads the cards/container size only
  // and writes the body's CSS variable, never the observed nodes' own
  // size, so it cannot feed back into a loop.
  useLayoutEffect(() => {
    if (timelineExpanded) {
      const container = expandedContainerRef.current;
      if (!container) {
        setTopRegionReserve(null);
        return;
      }
      const apply = () => {
        setTopRegionReserve(container.getBoundingClientRect().height);
      };
      apply();
      const observer = new ResizeObserver(apply);
      observer.observe(container);
      return () => observer.disconnect();
    }
    const breadcrumb = breadcrumbOverlayRef.current;
    const cluster = rightClusterOverlayRef.current;
    if (!breadcrumb && !cluster) {
      setTopRegionReserve(null);
      return;
    }
    const apply = () => {
      const breadcrumbHeight = breadcrumb
        ? breadcrumb.getBoundingClientRect().height
        : 0;
      const clusterHeight = cluster
        ? cluster.getBoundingClientRect().height
        : 0;
      setTopRegionReserve(Math.max(breadcrumbHeight, clusterHeight));
    };
    apply();
    const observer = new ResizeObserver(apply);
    if (breadcrumb) {
      observer.observe(breadcrumb);
    }
    if (cluster) {
      observer.observe(cluster);
    }
    return () => observer.disconnect();
  }, [
    // Re-bind when the observed surface flips (collapsed ↔ expanded) or
    // when the collapsed cards' presence may have changed, so the
    // initial measurement still lands on the new node(s) (or the
    // reserve is cleared when no node is present).
    timelineExpanded,
    newSession,
    activeThread?.id,
    isOnSubThread,
    paneToggleButton,
  ]);

  return {
    breadcrumbOverlayRef,
    rightClusterOverlayRef,
    expandedContainerRef,
    topRegionReserve,
  };
}
