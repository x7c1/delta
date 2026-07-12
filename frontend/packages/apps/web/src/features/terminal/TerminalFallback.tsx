import { Button, Panel } from '@delta/ui-kit';

export interface TerminalFallbackProps {
  /** Collapse the terminal column (mirrors the live pane's close button). */
  onClose: () => void;
}

/**
 * Shown in place of {@link TerminalPane} when its attach throws and the
 * surrounding error boundary trips. It keeps the panel chrome — including the
 * close control — so the embedded terminal failing never strands the user, and
 * the rest of the workspace (navigator, transcript, composer) keeps working.
 */
export function TerminalFallback({ onClose }: TerminalFallbackProps) {
  return (
    <Panel
      className="border-l border-border-default"
      header={
        <div className="flex items-center justify-between">
          <span className="text-secondary font-semibold text-fg">Terminal</span>
          <Button
            size="sm"
            variant="ghost"
            onClick={onClose}
            aria-label="Close terminal"
          >
            ✕
          </Button>
        </div>
      }
      bodyClassName="bg-terminal-bg"
    >
      <p className="p-3 text-caption text-terminal-fg">
        The terminal could not be displayed. It was isolated so it would not
        affect the rest of the app — reload the page to try again.
      </p>
    </Panel>
  );
}
