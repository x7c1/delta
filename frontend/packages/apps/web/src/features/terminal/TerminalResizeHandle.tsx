import { useCallback, type PointerEvent as ReactPointerEvent } from 'react';
import { useNavStore } from '../../store/navStore';

/**
 * A thin draggable divider on the left edge of the persistent right pane (large
 * screens only). Dragging left/right sets the pane width via the nav store,
 * which clamps it to the allowed range.
 *
 * Shared by both right-pane kinds — the terminal and the comms log — since they
 * are one column the user sizes once, and at most one of them is ever shown. Its
 * accessible name stays "Resize terminal": that is the control users know it as,
 * and the store field it drives (`terminalWidth`) is likewise the right column's
 * width under its original name.
 *
 * The pane sits on the right, so the new width is the distance from the pointer
 * to the right viewport edge: `window.innerWidth - clientX`.
 */
export function TerminalResizeHandle() {
  const setTerminalWidth = useNavStore((state) => state.setTerminalWidth);

  const onPointerDown = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      // Only react to the primary button / touch.
      if (event.button !== 0) {
        return;
      }
      event.preventDefault();
      const handle = event.currentTarget;
      handle.setPointerCapture(event.pointerId);
      // Suppress text selection across the whole document while dragging.
      document.body.classList.add('select-none');

      const onPointerMove = (moveEvent: PointerEvent) => {
        setTerminalWidth(window.innerWidth - moveEvent.clientX);
      };
      const onPointerUp = (upEvent: PointerEvent) => {
        handle.releasePointerCapture(upEvent.pointerId);
        document.body.classList.remove('select-none');
        handle.removeEventListener('pointermove', onPointerMove);
        handle.removeEventListener('pointerup', onPointerUp);
        handle.removeEventListener('pointercancel', onPointerUp);
      };

      handle.addEventListener('pointermove', onPointerMove);
      handle.addEventListener('pointerup', onPointerUp);
      handle.addEventListener('pointercancel', onPointerUp);
    },
    [setTerminalWidth],
  );

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize terminal"
      onPointerDown={onPointerDown}
      className="absolute inset-y-0 left-0 z-30 flex w-1.5 -translate-x-1/2 cursor-col-resize touch-none items-stretch"
    >
      {/* Visible hairline centered in the wider hit area. */}
      <div className="mx-auto w-px bg-border-default transition-colors hover:bg-border-strong" />
    </div>
  );
}
