import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type MouseEvent,
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
   * Notified whenever the panel opens or closes. Lets an enclosing row react to
   * the open state — e.g. lift its stacking order so the dropdown is not painted
   * under a sibling row (needed inside a windowed list, where each row is a
   * `transform`ed stacking context and the panel cannot otherwise escape it).
   */
  onOpenChange?: (open: boolean) => void;
}

const ITEM_TONE_CLASSES: Record<MenuItemTone, string> = {
  default: 'text-slate-700 hover:bg-slate-100',
  danger: 'text-red-600 hover:bg-red-50',
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
 * clicking it toggles a right-aligned panel rendered below the trigger.
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
  onOpenChange,
}: MenuProps) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const firstItemRef = useRef<HTMLButtonElement>(null);

  const isDisabled = disabled || items.length === 0;

  const close = useCallback(() => setOpen(false), []);

  // Surface open/close to the parent. `setState` setters are identity-stable, so
  // a parent passing one directly keeps this effect from re-firing each render.
  useEffect(() => {
    onOpenChange?.(open);
  }, [open, onOpenChange]);

  // Move focus into the panel on open; restore it to the trigger on close.
  //
  // Only *restore* focus when closing after having been open — never on the
  // initial mount. Without the `wasOpen` guard, the `else` branch fires on mount
  // (open starts false) and every freshly rendered Menu grabs focus, so right
  // after a page load a kebab trigger shows a focus ring it never earned. With
  // a windowed list that mounts many Menus at once, this is very visible.
  const wasOpen = useRef(false);
  useEffect(() => {
    if (open) {
      firstItemRef.current?.focus();
    } else if (wasOpen.current) {
      triggerRef.current?.focus();
    }
    wasOpen.current = open;
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
      setOpen((prev) => !prev);
    },
    [isDisabled],
  );

  const handleSelect = useCallback(
    (item: MenuItem) => {
      close();
      item.onSelect();
    },
    [close],
  );

  return (
    <div ref={containerRef} className={cn('relative', className)}>
      <button
        ref={triggerRef}
        type="button"
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={isDisabled}
        onClick={handleTriggerClick}
        className={cn(
          'inline-flex h-6 w-6 items-center justify-center rounded text-slate-400 transition-colors',
          'hover:bg-slate-200 hover:text-slate-700',
          'disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-slate-400',
        )}
      >
        <KebabIcon />
      </button>

      {open && (
        <div
          role="menu"
          aria-label={label}
          className="absolute right-0 top-full z-10 mt-1 min-w-[8rem] overflow-hidden rounded border border-slate-200 bg-white py-1 shadow-md"
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
                'block w-full px-3 py-1.5 text-left text-sm transition-colors',
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
