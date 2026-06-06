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
              <span className="text-slate-400" aria-hidden>
                ›
              </span>
            )}
            {item.onClick && !isLast ? (
              <button
                type="button"
                onClick={item.onClick}
                className="rounded px-1 text-indigo-600 hover:bg-indigo-50 hover:underline"
              >
                {item.label}
              </button>
            ) : (
              <span
                aria-current={isLast ? 'page' : undefined}
                className={cn(
                  'px-1',
                  isLast ? 'font-semibold text-slate-800' : 'text-slate-500',
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
