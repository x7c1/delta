import { Fragment } from 'react';
import { cn } from './cn';

export interface BreadcrumbItem {
  key: string | number;
  label: string;
  /** When omitted the item renders as the non-clickable current location. */
  onClick?: () => void;
}

export interface BreadcrumbProps {
  items: BreadcrumbItem[];
  className?: string;
}

/** A `a › b › c` trail. The last item is styled as the current location. */
export function Breadcrumb({ items, className }: BreadcrumbProps) {
  return (
    <nav
      aria-label="Breadcrumb"
      className={cn('flex flex-wrap items-center gap-1 text-xs', className)}
    >
      {items.map((item, index) => {
        const isLast = index === items.length - 1;
        return (
          <Fragment key={item.key}>
            {index > 0 && (
              <span className="text-fg-subtle" aria-hidden>
                ›
              </span>
            )}
            {item.onClick && !isLast ? (
              <button
                type="button"
                onClick={item.onClick}
                // The hover background uses a low-alpha wash of `accent`
                // (previously the hardcoded indigo-50 soft tint), matching the
                // Chip/Badge soft-tone pattern. `text-accent` is one step
                // lighter than the old indigo-600 (the accent token is
                // indigo-500 in light), which is acceptable since navigation
                // links read by hue rather than exact shade.
                className="rounded px-1 text-accent hover:bg-accent/10 hover:underline"
              >
                {item.label}
              </button>
            ) : (
              <span
                aria-current={isLast ? 'page' : undefined}
                className={cn(
                  'px-1',
                  // The current-page label uses `text-fg` (slate-900 light,
                  // one step darker than the old slate-800). Non-current
                  // crumbs use `text-fg-muted` (slate-600 light), shifting
                  // from the previous slate-500 by one step toward muted —
                  // slate-500 sits exactly between fg-muted and fg-subtle and
                  // the muted token reads as the correct "secondary trail"
                  // intent.
                  isLast ? 'font-semibold text-fg' : 'text-fg-muted',
                )}
              >
                {item.label}
              </span>
            )}
          </Fragment>
        );
      })}
    </nav>
  );
}
