import type {
  LaneCluster,
  TimelineDot,
  TimelineDotSize,
} from './timelineLanes';

/**
 * Mark diameter in pixels, by size class.
 *
 * Round marks read more naturally as "speech turns" than rectangles, but a
 * single uniform size let dense lanes overlap into an illegible blur. Two
 * deliberate sizes solve both problems: the main-conversation turns (user +
 * assistant prose) are the larger circle, and the auxiliary marks (tool
 * calls, meta lines, question cards) are the smaller circle. The size delta
 * is small (just visible) so the lane still reads as one timeline rather than
 * two layers, while the smaller dot helps the eye filter out auxiliary marks
 * at a glance. Overlap is prevented at the layout level by the shared global
 * x map (see {@link buildGlobalXMap}), which pushes any neighbour that would
 * collide to the right by at least the sum of the two radii — so the marks
 * can stay solid-fill without any alpha or ring workaround.
 */
export const MARK_LARGE_PX = 6;
export const MARK_SMALL_PX = 4;
/**
 * Diameter (px) of a cluster mark — pinned to {@link MARK_SMALL_PX} so a
 * cluster is the same VISUAL size as a lone auxiliary dot, never the larger
 * headline-turn size. v10 nudged the inner fill a hair larger (5 px) to
 * make a cluster "stand out"; v11 reverted the fill to 4 px but layered a
 * 1 px outline outside the box for the same purpose. In dogfooding the
 * outline turned out to occupy the FULL 1 px on each side OUTSIDE the
 * disc (an `outline` is painted strictly outside the element's box, so the
 * 4 px disc became a 6 px outer footprint — identical in TOTAL width to a
 * 6 px main-role dot, and the user reported clusters as "large outlined
 * circles". The outline was therefore the same regression the size bump
 * had been, just dressed differently: it bumped the cluster's visible
 * footprint back into headline-turn territory.
 *
 * The cluster now renders as a plain small dot — same fill colour, same
 * outer extent — and "cluster-ness" is purely positional / interactive:
 * the dot still occupies its representative's x, the data attributes
 * still expose `data-cluster-member-count` for diagnostics, and a click
 * still snaps the playhead to the representative member. Losing the
 * visual distinction is a deliberate trade: a cluster that reads as a
 * normal small dot is honest about being one mark on the timeline; the
 * user cares about WHERE on the time axis it sits, not whether it
 * collapses 2 or 6 underlying messages.
 */
export const MARK_CLUSTER_PX = MARK_SMALL_PX;

/**
 * Pixel diameter for a mark of the given size class. Fed to
 * {@link buildGlobalXMap} so the minimum spacing between two adjacent marks
 * is the average of their diameters — i.e. their summed radii — and the two
 * circles never paint into each other.
 */
export function markDiameterPx(size: TimelineDotSize): number {
  return size === 'large' ? MARK_LARGE_PX : MARK_SMALL_PX;
}

interface TimelineDotMarkProps {
  dot: TimelineDot;
  /**
   * Absolute x in pixels for this mark, resolved through the global x map
   * shared across every lane. The mark renders centred on this x so a
   * cross-lane playhead lands on the same column as the mark.
   */
  xPx: number;
}

/**
 * One mark within a lane. Rendered as a round speech-turn marker, colored by
 * author kind — user turns in blue, everything else in slate — and sized by
 * its role in the conversation: the main-conversation turns (user + Claude
 * prose) are the larger circle, auxiliary turns (tool calls, meta lines,
 * question cards) are the smaller circle. The tokens mirror `MessageItem`'s
 * bubble palette family so the timeline reads as the same conversation, just
 * compressed.
 *
 * Overlap is prevented at the layout level: the shared global x map (see
 * {@link buildGlobalXMap}) pushes any neighbour whose ideal time-axis x
 * would collide with the previous mark, so adjacent circles always clear
 * each other by at least the sum of their radii. The fill can therefore stay
 * solid — no alpha, no ring — and each mark reads as one disc.
 *
 * The mark is non-interactive: hover and click navigation flow through the
 * playhead alone, so a mark is purely a visual anchor.
 */
export function TimelineDotMark({ dot, xPx }: TimelineDotMarkProps) {
  // Two-color scheme: user vs everything else. Mirrors `MessageItem`'s
  // bubble palette family — the `info` accent for user, a muted foreground
  // tone for the assistant side.
  const colorClasses =
    dot.kind === 'user' ? 'bg-info' : 'bg-fg-subtle';
  const diameter = dot.size === 'large' ? MARK_LARGE_PX : MARK_SMALL_PX;
  return (
    <span
      data-testid="thread-timeline-dot"
      data-message-uuid={dot.uuid}
      data-thread-id={dot.threadId}
      data-message-kind={dot.kind}
      data-message-size={dot.size}
      aria-hidden="true"
      className={`pointer-events-none absolute top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-full ${colorClasses}`}
      style={{
        left: xPx,
        width: diameter,
        height: diameter,
      }}
    />
  );
}

interface TimelineClusterMarkProps {
  cluster: LaneCluster;
  /**
   * Absolute x in pixels for the cluster's representative (first member),
   * resolved through the global x map shared across every lane. The cluster
   * renders centred on this x; clicking near it snaps the playhead to the
   * representative message via the global nearest-message lookup.
   */
  xPx: number;
}

/**
 * A run of 2+ consecutive auxiliary marks (tool calls, meta lines, question
 * cards) collapsed into one visible disc. The cluster sits at the leftmost
 * member's x (its representative), so a left-to-right read of the lane stays
 * chronological, and a click that lands closest to it snaps the playhead to
 * the representative message via the global nearest-message lookup.
 *
 * The cluster renders at exactly the same diameter AND with no extra outline
 * vs. a lone small dot, so its total visual footprint equals
 * {@link MARK_CLUSTER_PX} px end-to-end — never the larger main-role
 * footprint. Earlier revisions tried a 5 px fill (v10) and then a 4 px fill
 * with a 1 px outline (v11) to make a cluster "stand out"; both produced a
 * 6 px outer disc indistinguishable from a 6 px main-role dot when the user
 * eyeballed the lane. The visual distinction is dropped on purpose: a
 * cluster behaves like a normal small dot to the eye, and the cluster
 * concept stays meaningful through (a) the representative x and (b) the
 * `data-cluster-member-count` attribute for downstream diagnostics and
 * tests. Click navigation keeps snapping to the representative member via
 * the global nearest-message lookup, with or without a visual cue.
 */
export function TimelineClusterMark({ cluster, xPx }: TimelineClusterMarkProps) {
  return (
    <span
      data-testid="thread-timeline-cluster"
      data-message-uuid={cluster.representativeUuid}
      data-thread-id={cluster.threadId}
      data-cluster-member-count={cluster.memberCount}
      aria-hidden="true"
      // No outline, no ring, no border — just the `fg-subtle` fill a lone
      // small assistant dot uses. The cluster's footprint equals a small
      // dot's exactly, never larger.
      className="pointer-events-none absolute top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-full bg-fg-subtle"
      style={{
        left: xPx,
        width: MARK_CLUSTER_PX,
        height: MARK_CLUSTER_PX,
      }}
    />
  );
}
