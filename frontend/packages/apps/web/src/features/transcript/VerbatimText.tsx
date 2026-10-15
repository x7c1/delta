import { useState } from 'react';
import { visibleText, type TextSegment } from './permissionText';

/** How each kind of segment is styled inside the block. */
const SEGMENT_CLASS: Record<TextSegment['kind'], string | undefined> = {
  text: undefined,
  // An escape stands for a character the user could not otherwise see, so it
  // is marked in the warning colour rather than read as part of the value.
  escape: 'rounded-sm bg-warning/15 text-warning',
  // A marker is not part of the value at all: it describes what was collapsed
  // or left out, so it is dimmed and set apart.
  marker: 'rounded-sm bg-surface-sunken px-1 text-fg-subtle',
};

export interface VerbatimTextProps {
  /** The text exactly as the request carries it. */
  text: string;
  'data-testid'?: string;
}

/**
 * Request text shown as it is: whitespace and line breaks preserved
 * (`pre-wrap`), padding collapsed to a counted marker, invisible characters
 * shown as escapes, and a value past the elision limit shown as head and tail
 * with a control to show the rest. See `permissionText.ts` for the rules.
 */
export function VerbatimText({
  text,
  'data-testid': testId,
}: VerbatimTextProps) {
  const [full, setFull] = useState(false);
  const visible = visibleText(text, full);
  return (
    <div className="flex flex-col items-start gap-1">
      <pre
        className="whitespace-pre-wrap break-all font-mono text-code text-fg-muted"
        data-testid={testId}
      >
        {visible.segments.map((segment, index) => (
          <span
            // Segments are positional and rebuilt together, so the index is
            // a stable key for one render.
            key={index}
            className={SEGMENT_CLASS[segment.kind]}
            data-segment={segment.kind === 'text' ? undefined : segment.kind}
          >
            {segment.text}
          </span>
        ))}
      </pre>
      {visible.elided > 0 && (
        <button
          type="button"
          className="text-caption text-fg-subtle underline hover:text-fg"
          onClick={() => setFull(true)}
        >
          Show all {Array.from(text).length} characters
        </button>
      )}
    </div>
  );
}
