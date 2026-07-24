import type { ThreadId } from '@delta/model';

/**
 * CSS class added to a transcript message article right after a wheel/click
 * jump scrolls it into view, then removed after the highlight fades so the
 * eye spots where the navigation landed. The class drives a one-shot,
 * compositor-friendly `::after` wash overlay on the inner message bubble
 * whose `opacity` fades to zero — only `opacity` is animated, so the flash
 * does not repaint the message body every frame. Matches the rule under
 * `.delta-timeline-jump-highlight` in index.css.
 *
 * Exported so a test can assert the class is applied without depending on
 * the literal string in two places.
 */
export const TIMELINE_JUMP_HIGHLIGHT_CLASS = 'delta-timeline-jump-highlight';

/**
 * Duration (ms) the {@link TIMELINE_JUMP_HIGHLIGHT_CLASS} stays applied. The
 * CSS animation under that class fades the wash overlay's `opacity` to zero
 * over this window, exposing the bubble's resting color; once the class is
 * removed the overlay is torn down and a subsequent jump to the same message
 * can re-apply the highlight cleanly.
 * Tuned to be brief but still legible: long enough to register as a fade
 * rather than a blink, short enough not to linger after the jump has landed.
 * MUST stay in lock-step with the matching CSS `animation` duration in
 * index.css — a shorter constant snaps the bubble back mid-fade, a longer
 * one leaves the class on stale.
 */
export const TIMELINE_JUMP_HIGHLIGHT_MS = 450;

/**
 * CSS selector matching a transcript message article by uuid. The selector
 * is anchored to the `<article>` tag so it never matches the timeline's own
 * dots or clusters, which stamp the SAME `data-message-uuid` value on a
 * `<span>` (see {@link TimelineDotMark} / {@link TimelineClusterMark}).
 *
 * Without the tag anchor a `[data-message-uuid="X"]` query rooted at the
 * conversation pane's scroll container hits the timeline span first
 * (DOM-pre-order — the top-region floating cards render before the
 * message list), so `scrollIntoView` lands on the already-visible dot
 * (no-op) and the pane-scroll IntersectionObserver observes the dot
 * instead of the article. Both regressions show up the moment the
 * timeline starts living inside the conversation pane's scroll
 * container — see TranscriptPane's `topRegion`.
 *
 * Exported so a regression test can pin the selector shape.
 */
export function articleMessageSelector(uuid: string): string {
  return `article[data-message-uuid="${CSS.escape(uuid)}"]`;
}

/**
 * CSS selector matching every transcript message article. The pane-scroll
 * IntersectionObserver iterates this set to track which article the user is
 * reading; the `<article>` tag anchor keeps the timeline's own dots — which
 * share the `data-message-uuid` attribute and (in the expanded state)
 * live in the same scroll container as the message articles — out of the
 * observation set.
 */
export const ALL_ARTICLES_SELECTOR = 'article[data-message-uuid]';

/**
 * Scroll the matching transcript message into view, aligned to the top of
 * the scrollable body. Scoped to the given container by uuid AND tag (see
 * {@link articleMessageSelector}), so neither a duplicate `data-message-uuid`
 * outside the transcript (e.g. in a portaled preview) nor the timeline's
 * own dots can misdirect the jump.
 *
 * Using `block: 'start'` rather than the v6 `block: 'center'` means the
 * destination message becomes the first line the eye reads on the next
 * paint — a centred message wastes half the viewport above the line the
 * user just asked to jump to. The transcript's top region overlay (the
 * collapsed-state breadcrumb and {Thread + Terminal} floating cards)
 * would otherwise hide the top of the article; the `scroll-margin-top`
 * rule on `article[data-message-uuid]` (driven by the live overlay
 * height via `--delta-top-region-reserve` — see index.css and
 * TranscriptPane) shifts the landing position down by that height so
 * the article lands just below the overlay row.
 *
 * The `scrollIntoView` call is guarded against environments where it is
 * unavailable (jsdom does not implement it on every element by default), so
 * an automatic jump driven by the playhead settle cannot crash unrelated
 * tests that render the overlay but never opted into a `scrollIntoView` stub.
 */
export function scrollMessageIntoView(
  container: HTMLElement | null,
  uuid: string,
): void {
  if (!container) {
    return;
  }
  const target = container.querySelector(articleMessageSelector(uuid));
  if (target && typeof target.scrollIntoView === 'function') {
    target.scrollIntoView({ block: 'start' });
  }
}

/**
 * Find the uuid of the message nearest {@link targetUuid} (by timeline order,
 * within the same lane {@link threadId}) whose `<article>` is currently
 * rendered in {@link container}.
 *
 * A cross-lane jump can target a message that renders NO article — an axis
 * click on a renders-nothing carrier (e.g. a `tool_result`, filtered out of
 * the transcript by `messageRendersNothing`). `scheduleScrollAfterRender` then
 * polls to its DOM-ready timeout and never scrolls, leaving the pane wherever
 * the thread switch parked it (the tail). This resolves the deterministic
 * fallback the timeout path scrolls to instead: the closest lane neighbor that
 * actually renders, so the pane lands NEAR the target's timeline position
 * rather than at the tail. Expands outward from the target and, on a distance
 * tie, prefers the later (tail-ward) neighbor so the pane sits just past the
 * carrier. Returns `null` when no lane message is rendered, so the caller can
 * fall back to the lane top.
 */
export function nearestRenderedNeighborUuid(
  container: HTMLElement | null,
  sorted: ReadonlyArray<{ uuid: string; threadId: ThreadId }>,
  targetUuid: string,
  threadId: ThreadId,
): string | null {
  if (!container) {
    return null;
  }
  const targetIndex = sorted.findIndex((m) => m.uuid === targetUuid);
  if (targetIndex < 0) {
    return null;
  }
  const isRendered = (uuid: string): boolean =>
    container.querySelector(articleMessageSelector(uuid)) !== null;
  for (let distance = 1; distance < sorted.length; distance += 1) {
    const after = sorted[targetIndex + distance];
    if (after && after.threadId === threadId && isRendered(after.uuid)) {
      return after.uuid;
    }
    const before = sorted[targetIndex - distance];
    if (before && before.threadId === threadId && isRendered(before.uuid)) {
      return before.uuid;
    }
  }
  return null;
}

/**
 * Briefly mark the matching transcript message with the jump-highlight class
 * so the eye spots where the navigation landed. The class attaches a
 * compositor-friendly `::after` wash overlay to the bubble whose `opacity`
 * fades from the wash color to transparent (see the
 * `.delta-timeline-jump-highlight` rule in index.css) — no paint-triggering
 * property is animated, so the flash stays smooth even over a long message.
 * Scoped to the given container AND the `<article>` tag (see
 * {@link articleMessageSelector}) so neither a duplicate uuid in a portaled
 * preview nor the timeline's own dots steal the highlight (a dot
 * highlighting amber would be confusing and would mask the missing
 * article-level highlight).
 *
 * Removing the class after {@link TIMELINE_JUMP_HIGHLIGHT_MS} tears the
 * overlay back down, so a subsequent jump to the same message re-applies the
 * highlight from rest. When the class is ALREADY present (a repeat jump to a
 * message still mid-fade), the overlay is restarted by removing the class and
 * re-adding it across a `requestAnimationFrame` boundary — the pseudo-element
 * is destroyed on removal and recreated on the next frame's add, replaying
 * the opacity keyframe from the start. This replaces the old forced
 * synchronous reflow (`void offsetWidth`): a fresh overlay node does not need
 * a reflow to restart. The common first-jump case (class not yet present)
 * adds the class synchronously, so the flash lands on the same frame as the
 * scroll.
 *
 * The removal timer uses `window.setTimeout` so the call is no-op safe under
 * SSR or jsdom without native timers (a missing `setTimeout` would just skip
 * the removal).
 *
 * Returns a cancel handle the caller can fire to clear the class early if
 * the component unmounts or a superseding jump arrives before the timer.
 */
export function highlightMessageJump(
  container: HTMLElement | null,
  uuid: string,
): () => void {
  if (!container) {
    return () => undefined;
  }
  const target = container.querySelector(articleMessageSelector(uuid));
  if (!target) {
    return () => undefined;
  }
  const canUseTimers =
    typeof window !== 'undefined' && typeof window.setTimeout === 'function';
  const canUseRaf =
    typeof window !== 'undefined' &&
    typeof window.requestAnimationFrame === 'function';

  let removalHandle: number | null = null;
  let restartRafHandle: number | null = null;

  const scheduleRemoval = () => {
    if (!canUseTimers) {
      return;
    }
    removalHandle = window.setTimeout(() => {
      target.classList.remove(TIMELINE_JUMP_HIGHLIGHT_CLASS);
    }, TIMELINE_JUMP_HIGHLIGHT_MS);
  };

  const applyHighlight = () => {
    target.classList.add(TIMELINE_JUMP_HIGHLIGHT_CLASS);
    scheduleRemoval();
  };

  if (target.classList.contains(TIMELINE_JUMP_HIGHLIGHT_CLASS) && canUseRaf) {
    // Repeat jump to a message still mid-fade: drop the overlay now and
    // re-create it on the next frame so the opacity keyframe replays from
    // the start (no forced reflow needed — the pseudo-element is a fresh
    // node once the class is re-added).
    target.classList.remove(TIMELINE_JUMP_HIGHLIGHT_CLASS);
    restartRafHandle = window.requestAnimationFrame(() => {
      restartRafHandle = null;
      applyHighlight();
    });
  } else {
    applyHighlight();
  }

  return () => {
    if (removalHandle !== null && canUseTimers) {
      window.clearTimeout(removalHandle);
    }
    if (restartRafHandle !== null && canUseRaf) {
      window.cancelAnimationFrame(restartRafHandle);
    }
    target.classList.remove(TIMELINE_JUMP_HIGHLIGHT_CLASS);
  };
}

/**
 * Maximum time (ms) {@link scheduleScrollAfterRender} polls for the target
 * message's element to appear in the DOM before giving up. The cross-lane
 * jump path switches the active thread first, then has to wait for the
 * conversation pane to re-render with the target thread's messages — which
 * can take several paint frames depending on the data layer (query refetch,
 * Suspense boundary, etc.). v10's single-rAF deferral was a no-op the
 * moment the re-render took more than one frame: `querySelector` returned
 * `null` and the scroll silently dropped. Polling across rAFs absorbs the
 * variable delay; the timeout caps the wait so a deleted message (or a
 * pane that genuinely never renders the uuid) cannot keep the loop running
 * forever.
 *
 * 1000 ms is roughly an order of magnitude above the worst observed
 * re-render delay in dogfooding — comfortable margin without feeling
 * stuck — and well below any "did the click do anything?" threshold a
 * human would notice. Exported so tests can assert the cap explicitly.
 */
export const SCROLL_DOM_READY_TIMEOUT_MS = 1000;

/**
 * Schedule {@link scrollMessageIntoView} to run as soon as the target uuid's
 * element appears in the DOM, so a preceding active-thread switch has time
 * to render the target thread's messages before the scroll fires. Polls
 * once per `requestAnimationFrame` until the element is present (or
 * {@link SCROLL_DOM_READY_TIMEOUT_MS} elapses), then scrolls and applies
 * the jump highlight in the same tick the element became visible.
 *
 * When the element never appears within the timeout the scroll is skipped
 * silently — the prior behaviour was a no-op `querySelector(null)` anyway,
 * so dropping the scroll on a missing target is not a behaviour change;
 * what we gain is the common case (re-render takes 2–N frames) actually
 * landing the scroll.
 *
 * Falls back to a zero-delay `setTimeout` when rAF is unavailable (older
 * test runners); in that fallback the wait is a single tick rather than
 * polled, matching the v10 deferral.
 *
 * The optional `onScroll` callback fires immediately before the
 * `scrollIntoView` call — i.e. ONLY when the target element actually
 * rendered and the scroll lands. Cross-lane callers use this to stamp the
 * time-based programmatic-scroll guard right before the scroll, so it covers
 * the remaining IO ripple window.
 *
 * The optional `onSettled` callback fires exactly once, whichever way the
 * schedule terminates: the scroll fired (success), the DOM-ready poll timed
 * out, or the returned cancel handle was invoked (superseding jump /
 * unmount). Cross-lane callers use this to release the in-flight guard
 * counter so it can never latch — every increment has exactly one matching
 * decrement regardless of which path the schedule takes.
 *
 * The optional `onTimeout` callback fires ONLY on the DOM-ready timeout leg —
 * the target's article never rendered within the budget — immediately before
 * `onSettled`. Cross-lane callers use it to run a deterministic fallback
 * scroll (to the nearest rendering neighbor of a renders-nothing target)
 * instead of silently leaving the pane wherever the thread switch parked it.
 * It does NOT fire on the success leg (the scroll landed) or the cancel leg (a
 * superseding jump / unmount).
 *
 * Returns a cancel handle the caller can fire to abort the wait if the
 * component unmounts or another jump supersedes this one before the element
 * lands. Invoking it also drives `onSettled` (once).
 */
export function scheduleScrollAfterRender(
  container: HTMLElement | null,
  uuid: string,
  onScroll?: () => void,
  onSettled?: () => void,
  onTimeout?: () => void,
): () => void {
  let settled = false;
  const settle = () => {
    if (settled) {
      return;
    }
    settled = true;
    onSettled?.();
  };
  let highlightCancel: (() => void) | null = null;
  let reflowRafHandle: number | null = null;
  const run = () => {
    onScroll?.();
    // The scroll is committing — settle now (before scrollIntoView, matching
    // the historical order where the guard-release ran inside `onScroll`).
    settle();
    scrollMessageIntoView(container, uuid);
    highlightCancel = highlightMessageJump(container, uuid);
    // Re-call scrollIntoView on the next animation frame so the browser has
    // had a chance to resolve the article's computed `scroll-margin-top`
    // (driven by the CSS variable `--delta-top-region-reserve` inherited
    // from the body). A cross-lane jump mounts a freshly-rendered article
    // whose computed scroll-margin-top is still 0 at first paint, so the
    // initial scrollIntoView aligns the article with the viewport top —
    // behind the overlay. After one frame, layout has resolved the margin
    // and the second call positions the article just below the overlay.
    // Same-lane jumps are unaffected: the second call is a no-op against an
    // already correctly-positioned element.
    if (
      typeof window !== 'undefined' &&
      typeof window.requestAnimationFrame === 'function'
    ) {
      reflowRafHandle = window.requestAnimationFrame(() => {
        reflowRafHandle = null;
        scrollMessageIntoView(container, uuid);
      });
    }
  };
  if (
    typeof window !== 'undefined' &&
    typeof window.requestAnimationFrame === 'function' &&
    typeof window.performance !== 'undefined' &&
    typeof window.performance.now === 'function'
  ) {
    let cancelled = false;
    let rafHandle = 0;
    const start = window.performance.now();
    const tick = () => {
      if (cancelled) {
        return;
      }
      // Re-query each frame so a re-render that swapped the target node's
      // identity (or appended it for the first time) is picked up at the
      // earliest possible paint. The selector is article-anchored (see
      // {@link articleMessageSelector}) so the timeline's own dots — which
      // share the uuid attribute and may already be present in the same
      // container — never satisfy the wait early and cause a no-op scroll.
      const present =
        container !== null &&
        container.querySelector(articleMessageSelector(uuid)) !== null;
      if (present) {
        run();
        return;
      }
      if (window.performance.now() - start >= SCROLL_DOM_READY_TIMEOUT_MS) {
        // Target never rendered within the budget (e.g. a renders-nothing
        // carrier message). Run the caller's deterministic fallback scroll
        // BEFORE settling — it parks the pane on the nearest rendering
        // neighbor instead of the tail — then settle so the caller's guard
        // counter is released instead of latching forever.
        onTimeout?.();
        settle();
        return;
      }
      rafHandle = window.requestAnimationFrame(tick);
    };
    rafHandle = window.requestAnimationFrame(tick);
    return () => {
      cancelled = true;
      window.cancelAnimationFrame(rafHandle);
      if (reflowRafHandle !== null) {
        window.cancelAnimationFrame(reflowRafHandle);
        reflowRafHandle = null;
      }
      highlightCancel?.();
      settle();
    };
  }
  const handle = setTimeout(run, 0);
  return () => {
    clearTimeout(handle);
    if (
      reflowRafHandle !== null &&
      typeof window !== 'undefined' &&
      typeof window.cancelAnimationFrame === 'function'
    ) {
      window.cancelAnimationFrame(reflowRafHandle);
      reflowRafHandle = null;
    }
    highlightCancel?.();
    settle();
  };
}
