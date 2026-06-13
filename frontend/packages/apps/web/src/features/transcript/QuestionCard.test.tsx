import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import type { QuestionNotice } from '../../store/liveStore';
import { QuestionCard } from './QuestionCard';

function notice(toolInput: string): QuestionNotice {
  return { kind: 'question', requestId: 1, toolInput, dismissed: false };
}

/** A resolved-promise stub for `onAnswer` in tests that only assert the call. */
const answerOk = () => Promise.resolve();

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

const MULTI_SELECT = JSON.stringify({
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
  ],
});

const MULTI_QUESTION = JSON.stringify({
  questions: [
    {
      question: 'Pick a language',
      header: 'Language',
      options: [
        { label: 'Rust', description: 'systems' },
        { label: 'TypeScript', description: 'web' },
      ],
      multiSelect: false,
    },
    {
      question: 'Pick a database',
      header: 'Database',
      options: [
        { label: 'SQLite', description: 'embedded' },
        { label: 'Postgres', description: 'server' },
      ],
      multiSelect: false,
    },
  ],
});

// A box-drawing preview exercises the verbatim, monospace rendering: it must
// survive untouched (no Markdown mangling of `|`/`+`/`-`, newlines preserved).
const PREVIEW_TEXT = ['+------+', '| Card |', '+------+'].join('\n');

const SINGLE_WITH_PREVIEW = JSON.stringify({
  questions: [
    {
      question: 'Which layout?',
      header: 'Layout',
      options: [
        { label: 'Boxed', description: 'A bordered card', preview: PREVIEW_TEXT },
        { label: 'Plain', description: 'No border' },
      ],
      multiSelect: false,
    },
  ],
});

const noop = () => {};

describe('QuestionCard', () => {
  it('renders a single question with its header, prompt, and options', () => {
    render(
      <QuestionCard
        notice={notice(SINGLE)}
        onAnswer={answerOk}
        onOpenTerminal={noop}
        onDismiss={noop}
      />,
    );

    expect(screen.getByTestId('question-card')).toBeTruthy();
    expect(screen.getByText('Framework')).toBeTruthy();
    expect(screen.getByText('Which framework should we use?')).toBeTruthy();
    expect(screen.getByText('React')).toBeTruthy();
    expect(screen.getByText('A UI library')).toBeTruthy();
    expect(screen.getByText('Svelte')).toBeTruthy();
  });

  it('answers a single-select question on option click (no Submit needed)', () => {
    const onAnswer = vi.fn().mockResolvedValue(undefined);
    render(
      <QuestionCard
        notice={notice(SINGLE)}
        onAnswer={onAnswer}
        onOpenTerminal={noop}
        onDismiss={noop}
      />,
    );

    // A lone single-select question has no Submit button — the click is the
    // answer.
    expect(screen.queryByTestId('question-submit')).toBeNull();
    fireEvent.click(screen.getByTestId('question-option-0-1'));
    expect(onAnswer).toHaveBeenCalledTimes(1);
    expect(onAnswer).toHaveBeenCalledWith([[1]]);
  });

  it('toggles multi-select options and submits the chosen set', () => {
    const onAnswer = vi.fn().mockResolvedValue(undefined);
    render(
      <QuestionCard
        notice={notice(MULTI_SELECT)}
        onAnswer={onAnswer}
        onOpenTerminal={noop}
        onDismiss={noop}
      />,
    );

    expect(screen.getByText('Select all that apply.')).toBeTruthy();
    const submit = screen.getByTestId('question-submit');
    // Submit is disabled until at least one option is toggled.
    expect((submit as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(screen.getByTestId('question-option-0-0'));
    fireEvent.click(screen.getByTestId('question-option-0-1'));
    expect((submit as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(submit);
    expect(onAnswer).toHaveBeenCalledWith([[0, 1]]);
  });

  it('collects one selection per question and submits when all answered', () => {
    const onAnswer = vi.fn().mockResolvedValue(undefined);
    render(
      <QuestionCard
        notice={notice(MULTI_QUESTION)}
        onAnswer={onAnswer}
        onOpenTerminal={noop}
        onDismiss={noop}
      />,
    );

    const submit = screen.getByTestId('question-submit');
    // A multi-question call needs every question chosen before Submit enables;
    // clicking one option does NOT submit immediately.
    fireEvent.click(screen.getByTestId('question-option-0-1'));
    expect(onAnswer).not.toHaveBeenCalled();
    expect((submit as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(screen.getByTestId('question-option-1-0'));
    expect((submit as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(submit);
    expect(onAnswer).toHaveBeenCalledWith([[1], [0]]);
  });

  it('renders an option preview verbatim and shows none for options without one', () => {
    render(
      <QuestionCard
        notice={notice(SINGLE_WITH_PREVIEW)}
        onAnswer={answerOk}
        onOpenTerminal={noop}
        onDismiss={noop}
      />,
    );

    // The option WITH a preview renders its block, preserving the exact text
    // (box-drawing characters and newlines intact, not run through Markdown).
    const preview = screen.getByTestId('question-option-preview-0-0');
    expect(preview).toBeTruthy();
    expect(preview.textContent).toBe(PREVIEW_TEXT);

    // The option WITHOUT a preview renders no preview block (no empty box).
    expect(screen.queryByTestId('question-option-preview-0-1')).toBeNull();
  });

  it('still answers on click when an option carries a preview', () => {
    const onAnswer = vi.fn().mockResolvedValue(undefined);
    render(
      <QuestionCard
        notice={notice(SINGLE_WITH_PREVIEW)}
        onAnswer={onAnswer}
        onOpenTerminal={noop}
        onDismiss={noop}
      />,
    );

    // Selecting the previewed option still works: the click target is the
    // button, with the preview rendered as a sibling below it.
    fireEvent.click(screen.getByTestId('question-option-0-0'));
    expect(onAnswer).toHaveBeenCalledTimes(1);
    expect(onAnswer).toHaveBeenCalledWith([[0]]);
  });

  it('shows NO Allow/Deny buttons', () => {
    render(
      <QuestionCard
        notice={notice(SINGLE)}
        onAnswer={answerOk}
        onOpenTerminal={noop}
        onDismiss={noop}
      />,
    );
    expect(screen.queryByText('Allow')).toBeNull();
    expect(screen.queryByText('Deny')).toBeNull();
  });

  it('keeps an Open terminal fallback and a Dismiss action', () => {
    const onOpenTerminal = vi.fn();
    const onDismiss = vi.fn();
    render(
      <QuestionCard
        notice={notice(SINGLE)}
        onAnswer={answerOk}
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
        onAnswer={answerOk}
        onOpenTerminal={noop}
        onDismiss={noop}
      />,
    );
    // The card still renders with its terminal fallback, just no options.
    expect(screen.getByTestId('question-card')).toBeTruthy();
    expect(
      screen.getByText(
        'Claude is asking a multiple-choice question. Answer it in the terminal.',
      ),
    ).toBeTruthy();
    expect(screen.queryByTestId('question-submit')).toBeNull();
  });

  it('surfaces an error and re-enables the controls when the answer POST fails', async () => {
    // The POST rejects (a 400/409 or a network failure): the card must not be
    // left with a dead Submit. It shows an inline error, keeps the terminal
    // fallback, and re-enables Submit so the user can retry.
    const onAnswer = vi.fn().mockRejectedValue(new Error('boom'));
    render(
      <QuestionCard
        notice={notice(MULTI_SELECT)}
        onAnswer={onAnswer}
        onOpenTerminal={noop}
        onDismiss={noop}
      />,
    );

    fireEvent.click(screen.getByTestId('question-option-0-0'));
    const submit = screen.getByTestId('question-submit') as HTMLButtonElement;
    fireEvent.click(submit);
    expect(onAnswer).toHaveBeenCalledTimes(1);

    // The inline error appears once the rejected promise settles (findBy* polls
    // inside act, so the post-reject state update is awaited cleanly).
    expect(await screen.findByTestId('question-error')).toBeTruthy();
    expect(screen.getByText('Open terminal')).toBeTruthy();

    // Submit is usable again (not left disabled), so a retry is possible.
    expect(submit.disabled).toBe(false);
    fireEvent.click(submit);
    expect(onAnswer).toHaveBeenCalledTimes(2);
  });
});
