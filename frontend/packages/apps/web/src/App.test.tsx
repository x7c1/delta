import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { App } from './App';
import { MACOS_SHELL } from './shell';

// The workspace itself is irrelevant here; only the root layout around it is.
vi.mock('./features/workspace/WorkspaceScreen', () => ({
  WorkspaceScreen: () => <div data-testid="workspace" />,
}));

describe('App title-bar inset', () => {
  afterEach(() => {
    delete document.documentElement.dataset.shell;
  });

  it('reserves the title-bar strip above the workspace in the macOS shell', () => {
    document.documentElement.dataset.shell = MACOS_SHELL;
    render(<App />);

    const strip = screen.getByTestId('shell-title-bar');
    expect(strip).toHaveClass('h-[var(--shell-top-inset)]');
    expect(strip).toHaveAttribute('aria-hidden', 'true');
    // The strip comes first in the root column, so the workspace is laid out
    // below it rather than under the native title bar.
    expect(
      strip.compareDocumentPosition(screen.getByTestId('workspace')) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it('reserves nothing outside the macOS shell', () => {
    render(<App />);

    expect(screen.getByTestId('workspace')).toBeInTheDocument();
    expect(screen.queryByTestId('shell-title-bar')).not.toBeInTheDocument();
  });
});
