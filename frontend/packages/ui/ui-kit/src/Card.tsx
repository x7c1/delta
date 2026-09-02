import type { ReactNode } from 'react';
import { CARD_BODY_CLASS, CARD_CAPTION_CLASS, CARD_FRAME_CLASS } from './cardStyles';
import { cn } from './cn';

export interface CardProps {
  /** One-line caption shown above the body. */
  summary: ReactNode;
  className?: string;
  /** Optional `data-testid` for the card's outer element. */
  testId?: string;
  children: ReactNode;
}

/**
 * The static counterpart of {@link Collapsible}: the same bordered frame and
 * caption line, but the body is always shown and there is nothing to click.
 * Use it when the content is meant to be read in place rather than opened.
 */
export function Card({ summary, className, testId, children }: CardProps) {
  return (
    <div className={cn(CARD_FRAME_CLASS, className)} data-testid={testId}>
      <div className={CARD_CAPTION_CLASS}>
        <span className="min-w-0 flex-1 truncate">{summary}</span>
      </div>
      <div className={CARD_BODY_CLASS}>{children}</div>
    </div>
  );
}
