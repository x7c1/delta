import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type MouseEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react';
import { cn } from './cn';

export type MenuItemTone = 'default' | 'danger';

export interface MenuItem {
  /** Visible, accessible label for the item. */
  label: string;
  /** Invoked when the item is selected; the menu then closes. */
  onSelect: () => void;
  /** `danger` styles destructive actions; defaults to `default`. */
  tone?: MenuItemTone;
}

export interface MenuProps {
  /** Accessible name for the trigger button (not shown visually). */
  label: string;
  /** Menu entries. With no enabled items the trigger renders disabled. */
  items: MenuItem[];
  /** Force the trigger disabled regardless of items. */
  disabled?: boolean;
  className?: string;
  /**
   * Extra classes for the trigger button itself (the outer `className` lands
   * on the positioning wrapper, which the trigger's own text color does not
   * inherit through). Merged last, so a consumer's utility — e.g. a text
   * color replacing the resting `text-fg-subtle` — wins per the {@link cn}
   * contract.
   */
  triggerClassName?: string;
  /**
   * Notified whenever the panel opens or closes. Lets an enclosing row react to
   * the open state — e.g. lift its stacking order so the dropdown is not painted
   * under a sibling row (needed inside a windowed list, where each row is a
   * `transform`ed stacking context and the panel cannot otherwise escape it).
   */
  onOpenChange?: (open: boolean) => void;
  /**
   * Open the panel when a mouse rests on the trigger, and close it shortly
   * after the pointer leaves the trigger and the panel, instead of waiting for
   * a click. A panel opened this way leaves focus where it was — hovering past
   * a row must not pull focus out of whatever the user is typing in. Touch
   * input ignores hover and keeps tap-to-open, and the keyboard keeps
   * Enter/Space; a panel opened either of those ways still moves focus into
   * itself.
   */
  openOnHover?: boolean;
}

/**
 * How long a hover-opened panel stays open after the pointer leaves it. Long
 * enough to cross the gap between the trigger and the panel, short enough that
 * a pointer moving on reads as closing the menu.
 */
const HOVER_CLOSE_DELAY_MS = 150;

const ITEM_TONE_CLASSES: Record<MenuItemTone, string> = {
  // The default item text uses `text-fg`: slate-700 (the original literal)
  // sat between fg-muted (slate-600) and fg (slate-900); fg matches the
  // "interactive content" convention used by Button. The hover background
  // uses `surface-elevated-hover` (light = slate-100, dark = slate-700),
  // one step darker than the panel's `surface-elevated` base — the
  // dropdown-on-elevated-surface mirror of `surface-sunken-hover`. The
  // danger row uses `text-danger` (rose-600 vs the original red-600 — one
  // hue step but the same intent) with a low-alpha wash of the same token
  // for the hover, matching the Chip/Badge soft-tone pattern.
  default: 'text-fg hover:bg-surface-elevated-hover',
  danger: 'text-danger hover:bg-danger/10',
};

/** A vertical three-dot ("kebab") glyph drawn as an inline SVG. */
function KebabIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="currentColor"
      aria-hidden="true"
    >
      <circle cx="8" cy="3" r="1.5" />
      <circle cx="8" cy="8" r="1.5" />
      <circle cx="8" cy="13" r="1.5" />
    </svg>
  );
}

/**
 * A small, reusable kebab dropdown menu. The trigger is a three-dot button;
 * clicking it toggles a right-aligned panel rendered below the trigger (or,
 * with {@link MenuProps.openOnHover}, hovering it opens the panel).
 * Click-outside and Escape close the panel, and selecting an item runs its
 * `onSelect` then closes. The trigger stops click propagation so it never
 * activates an enclosing row's handler. When disabled (explicitly or because
 * no items are enabled) the trigger is greyed and never opens a panel.
 */
export function Menu({
  label,
  items,
  disabled = false,
  className,
  triggerClassName,
  onOpenChange,
  openOnHover = false,
}: MenuProps) {
  const [open, setOpen] = useState(false);
  // The open state as of the latest change, readable before React re-renders.
  // A mouse click follows the pointer's arrival within the same burst of input,
  // and a hover open is not necessarily rendered by the time the click arrives,
  // so the event handlers below read this instead of the rendered `open` —
  // otherwise that click would see the panel as shut and toggle it straight
  // back.
  const openRef = useRef(false);
  // Whether the current panel was opened by hovering, which decides both the
  // focus handling below and what a click on the trigger does while it is open.
  const openedByHover = useRef(false);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  );
  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const firstItemRef = useRef<HTMLButtonElement>(null);

  const isDisabled = disabled || items.length === 0;

  const cancelPendingClose = useCallback(() => {
    if (closeTimer.current !== undefined) {
      clearTimeout(closeTimer.current);
      closeTimer.current = undefined;
    }
  }, []);

  const changeOpen = useCallback((next: boolean) => {
    openRef.current = next;
    setOpen(next);
  }, []);

  const close = useCallback(() => {
    cancelPendingClose();
    changeOpen(false);
  }, [cancelPendingClose, changeOpen]);

  useEffect(() => cancelPendingClose, [cancelPendingClose]);

  // Surface open/close to the parent. `setState` setters are identity-stable, so
  // a parent passing one directly keeps this effect from re-firing each render.
  useEffect(() => {
    onOpenChange?.(open);
  }, [open, onOpenChange]);

  // Move focus into the panel on open; restore it to the trigger on close.
  //
  // Only *restore* focus when this panel moved it in — never on the initial
  // mount. Without the `movedFocus` guard, the `else` branch fires on mount
  // (open starts false) and every freshly rendered Menu grabs focus, so right
  // after a page load a kebab trigger shows a focus ring it never earned. With
  // a windowed list that mounts many Menus at once, this is very visible.
  //
  // A hover-opened panel moves no focus, so its close restores none either.
  const movedFocus = useRef(false);
  useEffect(() => {
    if (open) {
      if (!openedByHover.current) {
        firstItemRef.current?.focus();
        movedFocus.current = true;
      }
    } else if (movedFocus.current) {
      triggerRef.current?.focus();
      movedFocus.current = false;
    }
  }, [open]);

  // Escape closes; click-outside closes.
  useEffect(() => {
    if (!open) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        close();
      }
    };
    const onPointerDown = (event: PointerEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) {
        close();
      }
    };
    document.addEventListener('keydown', onKeyDown);
    document.addEventListener('pointerdown', onPointerDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      document.removeEventListener('pointerdown', onPointerDown);
    };
  }, [open, close]);

  const handleTriggerClick = useCallback(
    (event: MouseEvent<HTMLButtonElement>) => {
      // Never let the click reach an enclosing row's onClick.
      event.stopPropagation();
      if (isDisabled) {
        return;
      }
      // A mouse click lands on a trigger it is already hovering, so in hover
      // mode the panel is open by then; the click keeps it open rather than
      // toggling it shut under the pointer.
      if (openRef.current && openedByHover.current) {
        return;
      }
      openedByHover.current = false;
      changeOpen(!openRef.current);
    },
    [isDisabled, changeOpen],
  );

  const handlePointerEnter = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (!openOnHover || isDisabled || event.pointerType === 'touch') {
        return;
      }
      cancelPendingClose();
      if (!openRef.current) {
        openedByHover.current = true;
        changeOpen(true);
      }
    },
    [openOnHover, isDisabled, cancelPendingClose, changeOpen],
  );

  const handlePointerLeave = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (!openOnHover || !openRef.current || event.pointerType === 'touch') {
        return;
      }
      cancelPendingClose();
      closeTimer.current = setTimeout(close, HOVER_CLOSE_DELAY_MS);
    },
    [openOnHover, close, cancelPendingClose],
  );

  const handleSelect = useCallback(
    (item: MenuItem) => {
      close();
      item.onSelect();
    },
    [close],
  );

  return (
    <div
      ref={containerRef}
      className={cn('relative', className)}
      // The panel is a child of this wrapper, so the pointer moving from the
      // trigger onto the panel stays "inside" and only the gap between them
      // is covered by the close delay.
      onPointerEnter={handlePointerEnter}
      onPointerLeave={handlePointerLeave}
    >
      <button
        ref={triggerRef}
        type="button"
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={isDisabled}
        onClick={handleTriggerClick}
        className={cn(
          // The trigger's resting and disabled-hover text use `text-fg-subtle`
          // (slate-400 exact). The hover background uses `bg-surface-sunken`
          // (slate-200 exact). The hover text shifts to `text-fg`: slate-700
          // sat between fg-muted (slate-600) and fg (slate-900); fg matches
          // the interactive-content convention.
          'inline-flex h-6 w-6 items-center justify-center rounded-sm text-fg-subtle transition-colors',
          'hover:bg-surface-sunken hover:text-fg',
          'disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-fg-subtle',
          triggerClassName,
        )}
      >
        <KebabIcon />
      </button>

      {open && (
        <div
          role="menu"
          aria-label={label}
          // The popover panel uses `bg-surface-elevated` (light = slate-50)
          // rather than `bg-surface` (light = white) to follow the Dialog
          // convention: a dropdown is by definition an elevated surface and
          // should read as raised over the page in every theme. The light
          // shift from white to slate-50 is imperceptible at the panel's
          // actual size.
          className="absolute right-0 top-full z-10 mt-1 min-w-32 overflow-hidden rounded-sm border border-border-default bg-surface-elevated py-1 shadow-md"
        >
          {items.map((item, index) => (
            <button
              key={item.label}
              ref={index === 0 ? firstItemRef : undefined}
              type="button"
              role="menuitem"
              onClick={(event) => {
                event.stopPropagation();
                handleSelect(item);
              }}
              className={cn(
                // `whitespace-nowrap` keeps each label on a single line; without
                // it a label longer than the panel's `min-w-32` (e.g. `Copy
                // session ID`) wrapped to a second line. The panel itself has no
                // max width, so it now grows to fit the longest label naturally.
                'block w-full whitespace-nowrap px-3 py-1.5 text-left text-secondary transition-colors',
                ITEM_TONE_CLASSES[item.tone ?? 'default'],
              )}
            >
              {item.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
