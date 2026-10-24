import {
  useCallback,
  useLayoutEffect,
  useState,
  type RefObject,
} from 'react';

interface TimelineAxisViewport {
  /**
   * Callback ref for the timeline's horizontal-scroll container. Mirrors the
   * node into the caller's ref (which the scroll and navigation hooks read)
   * and into state, so the width observer re-attaches whenever the container
   * mounts or unmounts (expand / collapse, empty / non-empty lane list).
   */
  setAxisScrollNode: (node: HTMLDivElement | null) => void;
  /** The container's `clientWidth` (px); 0 while it is unmounted. */
  axisViewportWidth: number;
}

/**
 * Track the visible width of the timeline's horizontal-scroll container with
 * a `ResizeObserver`, so layout that depends on it (the lane label cap) and
 * the playhead catch-up follow the card width as the window is resized.
 */
export function useTimelineAxisViewport(
  axisScrollRef: RefObject<HTMLDivElement | null>,
): TimelineAxisViewport {
  const [axisScrollEl, setAxisScrollEl] = useState<HTMLDivElement | null>(
    null,
  );
  const [axisViewportWidth, setAxisViewportWidth] = useState(0);

  const setAxisScrollNode = useCallback(
    (node: HTMLDivElement | null) => {
      axisScrollRef.current = node;
      setAxisScrollEl(node);
    },
    [axisScrollRef],
  );

  // Layout phase so the first measurement lands before paint: the label cap
  // derived from it then applies on the first visible frame instead of
  // flashing an uncapped label column.
  useLayoutEffect(() => {
    if (axisScrollEl === null) {
      setAxisViewportWidth(0);
      return;
    }
    const measure = () => setAxisViewportWidth(axisScrollEl.clientWidth);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(axisScrollEl);
    return () => observer.disconnect();
  }, [axisScrollEl]);

  return { setAxisScrollNode, axisViewportWidth };
}
