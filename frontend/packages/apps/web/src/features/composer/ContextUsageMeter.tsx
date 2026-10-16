import { Meter, cn } from '@delta/ui-kit';
import { COMPOSER_RAIL_ITEM_CLASS } from './ComposerRail';

const LABEL = 'Context window usage';

export interface ContextUsageMeterProps {
  /**
   * The focused session's context-window usage in percent, as the latest
   * `status_updated` snapshot reported it. Clamped to 0–100 here, so a
   * provider's out-of-range figure never overflows the track or the readout.
   */
  usedPercentage: number;
}

/**
 * The composer rail's right-end item: the focused session's context-window
 * usage as a small fixed-width bar with the percentage beside it — right where
 * the user is about to send.
 *
 * It speaks the same visual language as the account rate-limit rows in the
 * navigator footer (`RateLimitRow`): the ui-kit `Meter` with the neutral
 * `bg-fg-muted` fill, anchored to the right (`justify-end`) so the bar grows
 * leftward from the readout, and a monospace tabular percentage.
 */
export function ContextUsageMeter({ usedPercentage }: ContextUsageMeterProps) {
  const clamped = Number.isFinite(usedPercentage)
    ? Math.min(100, Math.max(0, usedPercentage))
    : 0;
  return (
    <div
      className={cn(COMPOSER_RAIL_ITEM_CLASS, 'group/ctx relative')}
      data-testid="composer-context-meter"
    >
      {/* `py-1` around a 14px (`h-3.5`) row gives the template button's 22px
          item height, so the two items share one baseline on the rail and the
          rail is no taller with the meter than without it. Focusable so the
          tooltip below is reachable from the keyboard too. */}
      <div
        className="flex cursor-help items-center px-2 py-1"
        tabIndex={0}
      >
        <div className="flex h-3.5 items-center gap-1.5 font-mono text-code leading-none text-fg-muted">
          <Meter
            value={clamped}
            fillClassName="bg-fg-muted"
            className="flex w-12 justify-end"
            // Accessible name only: the custom tooltip below already shows the
            // same words, and a native `title` would add a second one.
            label={LABEL}
          />
          <span
            className="inline-block min-w-[3ch] text-right tabular-nums"
            data-testid="composer-context-pct"
          >
            {Math.round(clamped)}%
          </span>
        </div>
      </div>
      {/* Opens upward, so it never covers the composer card below. */}
      <span
        role="note"
        data-testid="composer-context-popover"
        className="pointer-events-none absolute bottom-full right-0 z-10 mb-1 hidden w-max max-w-xs rounded-md border border-border-default bg-surface px-2.5 py-1.5 text-caption text-fg-muted shadow-lg group-hover/ctx:block group-focus-within/ctx:block"
      >
        {LABEL}
      </span>
    </div>
  );
}
