import { useState } from 'react';
import { Button } from '@delta/ui-kit';
import type { QuestionNotice as Notice } from '../../store/liveStore';

/**
 * One option of an `AskUserQuestion` question: a short `label` the user picks
 * and a longer `description` explaining it.
 */
interface QuestionOption {
  label: string;
  description?: string;
}

/** One question of an `AskUserQuestion` tool call. */
interface ParsedQuestion {
  /** A short heading for the question (e.g. "Framework"). */
  header?: string;
  /** The full question prompt. */
  question?: string;
  options: QuestionOption[];
  /** Whether more than one option may be selected. */
  multiSelect: boolean;
}

/**
 * Parse the raw `{questions:[…]}` tool input into the questions to render.
 *
 * Defensive by design: the payload comes straight off the wire, so anything
 * unparsable or shaped unexpectedly yields an empty list rather than throwing —
 * the card then shows its terminal fallback without interactive options.
 */
export function parseQuestions(toolInput: string): ParsedQuestion[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(toolInput);
  } catch {
    return [];
  }
  if (typeof parsed !== 'object' || parsed === null) {
    return [];
  }
  const questions = (parsed as Record<string, unknown>).questions;
  if (!Array.isArray(questions)) {
    return [];
  }
  return questions.map((raw): ParsedQuestion => {
    const record =
      typeof raw === 'object' && raw !== null
        ? (raw as Record<string, unknown>)
        : {};
    const rawOptions = Array.isArray(record.options) ? record.options : [];
    const options = rawOptions.flatMap((opt): QuestionOption[] => {
      if (typeof opt !== 'object' || opt === null) {
        return [];
      }
      const o = opt as Record<string, unknown>;
      if (typeof o.label !== 'string') {
        return [];
      }
      return [
        {
          label: o.label,
          description:
            typeof o.description === 'string' ? o.description : undefined,
        },
      ];
    });
    return {
      header: typeof record.header === 'string' ? record.header : undefined,
      question:
        typeof record.question === 'string' ? record.question : undefined,
      options,
      multiSelect:
        typeof record.multiSelect === 'boolean' ? record.multiSelect : false,
    };
  });
}

export interface QuestionCardProps {
  notice: Notice;
  /**
   * Submit the answer: the chosen 0-based option index(es) per question, in
   * question order. The parent POSTs it to the answer endpoint, which injects
   * the selection keystrokes into the session's TUI pane.
   */
  onAnswer: (selections: number[][]) => void;
  /** Open the embedded terminal (the fallback if injection misfires). */
  onOpenTerminal: () => void;
  /** Dismiss the card without answering (the TUI prompt stays up). */
  onDismiss: () => void;
}

/**
 * The interactive question card for Claude Code's `AskUserQuestion` tool.
 *
 * It renders each question's header and prompt and turns its options into
 * controls the user actually answers from Delta:
 *
 * - A single-select question's options are clickable rows — clicking one is the
 *   answer (it submits immediately for a single-question call, or records that
 *   question's choice in a multi-question call).
 * - A multi-select question's options are checkboxes; a per-card Submit sends
 *   the toggled set.
 * - A multi-question call collects one selection per question, with a single
 *   Submit enabled once every question has a choice.
 *
 * On submit the parent injects the selection keystrokes into the session's TUI
 * pane (a CLI hook cannot return the pick). The authoritative clear is still the
 * existing resolution path (the `tool_result` resolving the question's request
 * row), not this component. An "Open terminal" link stays as a fallback so a
 * misfired injection never strands the user.
 *
 * It renders inline at the conversation tail (not in a floating overlay), so the
 * choices sit in the flow right after the assistant's live-streamed preamble.
 * Capped in height with internal scrolling so a large multi-question card never
 * blankets the transcript.
 */
export function QuestionCard({
  notice,
  onAnswer,
  onOpenTerminal,
  onDismiss,
}: QuestionCardProps) {
  const questions = parseQuestions(notice.toolInput);
  const answerable = questions.length > 0;
  const multiQuestion = questions.length > 1;

  // One selection set per question (a Set of chosen option indices). A
  // single-select question keeps at most one; a multi-select question any
  // number. Seeded empty and built up as the user clicks/toggles.
  const [selections, setSelections] = useState<Set<number>[]>(() =>
    questions.map(() => new Set<number>()),
  );
  // True once the user has submitted, to disable the controls and avoid a
  // double-send while the authoritative clear (the resolution path) lands.
  const [submitted, setSubmitted] = useState(false);

  const submit = (sets: Set<number>[]) => {
    if (submitted) {
      return;
    }
    setSubmitted(true);
    onAnswer(sets.map((set) => [...set].sort((a, b) => a - b)));
  };

  const toggleMulti = (qi: number, oi: number) => {
    setSelections((prev) => {
      const next = prev.map((set, i) => (i === qi ? new Set(set) : set));
      const set = next[qi];
      if (set.has(oi)) {
        set.delete(oi);
      } else {
        set.add(oi);
      }
      return next;
    });
  };

  const chooseSingle = (qi: number, oi: number) => {
    // A single-question single-select call answers on click. A multi-question
    // call records the choice and waits for the overall Submit.
    if (!multiQuestion) {
      submit([new Set([oi])]);
      return;
    }
    setSelections((prev) => prev.map((set, i) => (i === qi ? new Set([oi]) : set)));
  };

  // Every question must have at least one selection before the overall Submit
  // (multi-question, or a single multi-select question) is enabled.
  const allAnswered = selections.every((set) => set.size > 0);

  return (
    <div
      className="flex max-h-[40vh] flex-col gap-2 overflow-y-auto rounded-md border border-indigo-200 bg-indigo-50 px-3 py-2 text-xs"
      data-testid="question-card"
      role="group"
      aria-label="Question from Claude Code"
    >
      <p className="font-medium text-indigo-900">Claude is asking a question</p>

      {!answerable ? (
        <p className="text-slate-600">
          Claude is asking a multiple-choice question. Answer it in the terminal.
        </p>
      ) : (
        <ul className="space-y-2">
          {questions.map((q, qi) => (
            <li key={qi} className="space-y-1">
              {q.header && (
                <p className="font-semibold text-indigo-800">{q.header}</p>
              )}
              {q.question && <p className="text-slate-700">{q.question}</p>}
              {q.multiSelect && (
                <p className="text-slate-500">Select all that apply.</p>
              )}
              <ul className="space-y-1">
                {q.options.map((opt, oi) => {
                  const selected = selections[qi]?.has(oi) ?? false;
                  return (
                    <li key={oi}>
                      <button
                        type="button"
                        disabled={submitted}
                        aria-pressed={selected}
                        data-testid={`question-option-${qi}-${oi}`}
                        onClick={() =>
                          q.multiSelect
                            ? toggleMulti(qi, oi)
                            : chooseSingle(qi, oi)
                        }
                        className={`flex w-full items-start gap-2 rounded border px-2 py-1 text-left transition-colors disabled:opacity-60 ${
                          selected
                            ? 'border-indigo-400 bg-indigo-100'
                            : 'border-indigo-100 bg-white hover:border-indigo-300'
                        }`}
                      >
                        {q.multiSelect && (
                          <span
                            aria-hidden="true"
                            className="mt-0.5 font-mono text-indigo-700"
                          >
                            {selected ? '[x]' : '[ ]'}
                          </span>
                        )}
                        <span>
                          <span className="font-semibold text-slate-800">
                            {opt.label}
                          </span>
                          {opt.description && (
                            <span className="block text-slate-600">
                              {opt.description}
                            </span>
                          )}
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </li>
          ))}
        </ul>
      )}

      <div className="flex flex-wrap items-center gap-2">
        {/* A Submit is shown whenever a click alone is not the answer: a
            multi-question call, or any multi-select question. A lone
            single-select question answers on the option click, so it needs no
            Submit. */}
        {answerable && (multiQuestion || questions.some((q) => q.multiSelect)) && (
          <Button
            size="sm"
            disabled={submitted || !allAnswered}
            data-testid="question-submit"
            onClick={() => submit(selections)}
          >
            Submit
          </Button>
        )}
        {/* The terminal stays available as a fallback if injection misfires. */}
        <Button size="sm" variant="ghost" onClick={onOpenTerminal}>
          Open terminal
        </Button>
        <Button size="sm" variant="ghost" onClick={onDismiss}>
          Dismiss
        </Button>
      </div>
    </div>
  );
}
