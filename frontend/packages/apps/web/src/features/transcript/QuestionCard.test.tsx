import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import type { QuestionNotice } from '../../store/liveStore';
import { QuestionCard } from './QuestionCard';

function notice(toolInput: string): QuestionNotice {
  return { kind: 'question', requestId: 1, toolInput, dismissed: false };
}

const SINGLE = JSON.stringify({
  questions: [
    {
      question: 'Which framework should we use?',
      header: 'Framework',
      options: [
        { label: 'React', description: 'A UI library' },
        { label: 'Svelte', description: 'A compiler' },
      ],
      multiSelect: false,
    },
  ],
});

const MULTI = JSON.stringify({
  questions: [
    {
      question: 'Pick languages',
      header: 'Languages',
      options: [
        { label: 'Rust', description: 'systems' },
        { label: 'TypeScript', description: 'web' },
      ],
      multiSelect: true,
    },
    {
      question: 'Pick a database',
      header: 'Database',
      options: [{ label: 'SQLite', description: 'embedded' }],
      multiSelect: false,
    },
  ],
});

describe('QuestionCard', () => {
  const noop = () => {};

  it('renders a single question with its header, prompt, and options', () => {
    render(
      <QuestionCard notice={notice(SINGLE)} onOpenTerminal={noop} onDismiss={noop} />,
    );

    const card = screen.getByTestId('question-card');
    expect(card).toBeTruthy();
    expect(screen.getByText('Framework')).toBeTruthy();
    expect(screen.getByText('Which framework should we use?')).toBeTruthy();
    expect(screen.getByText('React')).toBeTruthy();
    expect(screen.getByText('A UI library')).toBeTruthy();
    expect(screen.getByText('Svelte')).toBeTruthy();
  });

  it('renders multiple questions and the multi-select hint', () => {
    render(
      <QuestionCard notice={notice(MULTI)} onOpenTerminal={noop} onDismiss={noop} />,
    );

    expect(screen.getByText('Languages')).toBeTruthy();
    expect(screen.getByText('Database')).toBeTruthy();
    expect(screen.getByText('Rust')).toBeTruthy();
    expect(screen.getByText('SQLite')).toBeTruthy();
    // The multiSelect question surfaces the "select all" hint.
    expect(screen.getByText('Select all that apply.')).toBeTruthy();
  });

  it('shows NO Allow/Deny buttons (answering happens in the terminal)', () => {
    render(
      <QuestionCard notice={notice(SINGLE)} onOpenTerminal={noop} onDismiss={noop} />,
    );
    expect(screen.queryByText('Allow')).toBeNull();
    expect(screen.queryByText('Deny')).toBeNull();
  });

  it('fires onOpenTerminal and onDismiss', () => {
    const onOpenTerminal = vi.fn();
    const onDismiss = vi.fn();
    render(
      <QuestionCard
        notice={notice(SINGLE)}
        onOpenTerminal={onOpenTerminal}
        onDismiss={onDismiss}
      />,
    );

    fireEvent.click(screen.getByText('Open terminal'));
    expect(onOpenTerminal).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByText('Dismiss'));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('degrades gracefully when the tool input is unparsable', () => {
    render(
      <QuestionCard
        notice={notice('not json')}
        onOpenTerminal={noop}
        onDismiss={noop}
      />,
    );
    // The card still renders with its terminal guidance, just no options.
    expect(screen.getByTestId('question-card')).toBeTruthy();
    expect(screen.getByText('Pick an option in the terminal.')).toBeTruthy();
  });
});
