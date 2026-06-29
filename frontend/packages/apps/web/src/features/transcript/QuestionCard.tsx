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
  /**
   * An optional pre-formatted preview (a multi-line mockup, code snippet, or
   * ASCII/box-drawing art) rendered verbatim so the user can compare options.
   * Shown in a monospace, whitespace-preserving block — never through the
   * Markdown renderer, which would mangle box-drawing characters.
   */
  preview?: string;
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
          preview: typeof o.preview === 'string' ? o.preview : undefined,
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
   * the selection keystrokes into the session's TUI pane. The returned promise
   * rejects when the POST fails (a `400`/`409` or a network error); the card
   * awaits it to surface an inline error and re-enable its controls for a retry.
   */
  onAnswer: (selections: number[][]) => Promise<void>;
  /**
   * Cancel the question in the TUI itself: the parent POSTs to the cancel
   * endpoint, which injects a single `Escape` into the session's pane (one key
   * cancels the whole call). The returned promise rejects when the POST fails (a
   * `409` or a network error); the card awaits it to surface an inline error and
   * re-enable its controls for a retry. Distinct from {@link onDismiss}, which
   * only hides the card and leaves the TUI prompt up.
   */
  onCancel: () => Promise<void>;
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
 * If the answer POST itself fails (a `400`/`409` or a network error), the card
 * shows an inline error, re-enables its controls so the user can retry, and
 * emphasizes the terminal fallback — it never leaves a dead Submit behind.
 *
 * A "Cancel" action does the terminal's `Esc`: the parent POSTs to the cancel
 * endpoint, which injects a single `Escape` into the pane and cancels the whole
 * question in the TUI (the `is_error` `tool_result` then clears the card through
 * the same resolution path). It is distinct from "Dismiss", which only hides the
 * card locally and leaves the TUI prompt open — both are kept because they do
 * genuinely different things. A failed cancel POST shares the answer's failure
 * UX (inline error, terminal emphasized, controls re-enabled).
 *
 * It renders inline at the conversation tail (not in a floating overlay), so the
 * choices sit in the flow right after the assistant's live-streamed preamble.
 * It grows with its content (no height cap or internal scroll) so every option
 * is visible in the conversation pane without scrolling within the card.
 */
export function QuestionCard({
  notice,
  onAnswer,
  onCancel,
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
  // double-send while the authoritative clear (the resolution path) lands. A
  // failed POST flips it back so the user can retry.
  const [submitted, setSubmitted] = useState(false);
  // Set to the action ('answer' or 'cancel') whose POST rejected, so the card
  // shows an action-specific inline error and emphasizes the terminal fallback
  // instead of silently doing nothing; null while nothing has failed.
  const [failedAction, setFailedAction] = useState<'answer' | 'cancel' | null>(
    null,
  );
  const failed = failedAction !== null;

  const submit = (sets: Set<number>[]) => {
    if (submitted) {
      return;
    }
    setSubmitted(true);
    setFailedAction(null);
    onAnswer(sets.map((set) => [...set].sort((a, b) => a - b))).catch(() => {
      // Re-enable the controls and surface the failure so the Submit is never
      // left dead; the terminal fallback is emphasized below.
      setSubmitted(false);
      setFailedAction('answer');
    });
  };

  const cancel = () => {
    if (submitted) {
      return;
    }
    // Reuse `submitted` to disable the controls while the cancel is in flight,
    // so an answer and a cancel cannot race. A failure flips it back and shows
    // the shared inline error, exactly like a failed answer.
    setSubmitted(true);
    setFailedAction(null);
    onCancel().catch(() => {
      setSubmitted(false);
      setFailedAction('cancel');
    });
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
      className="flex flex-col gap-2 rounded-md border border-accent/30 bg-accent/10 px-3 py-2 text-sm"
      data-testid="question-card"
      role="group"
      aria-label="Question from Claude Code"
    >
      <p className="text-xs font-medium text-accent">
        Claude is asking a question
      </p>

      {!answerable ? (
        <p className="text-fg-muted">
          Claude is asking a multiple-choice question. Answer it in the terminal.
        </p>
      ) : (
        <ul className="space-y-2">
          {questions.map((q, qi) => (
            <li key={qi} className="space-y-1">
              {q.header && (
                <p className="font-semibold text-accent">{q.header}</p>
              )}
              {q.question && <p className="text-fg-muted">{q.question}</p>}
              {q.multiSelect && (
                <p className="text-fg-subtle">Select all that apply.</p>
              )}
              <ul className="space-y-1">
                {q.options.map((opt, oi) => {
                  const selected = selections[qi]?.has(oi) ?? false;
                  return (
                    <li key={oi} className="flex items-start gap-2">
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
                        className={`flex min-w-0 items-start gap-2 rounded border px-2 py-1 text-left transition-colors disabled:opacity-60 ${
                          opt.preview ? 'w-80 shrink-0' : 'w-full'
                        } ${
                          selected
                            ? 'border-accent bg-accent/20'
                            : 'border-accent/20 bg-surface hover:border-accent-disabled'
                        }`}
                      >
                        {q.multiSelect && (
                          <span
                            aria-hidden="true"
                            className="mt-0.5 font-mono text-accent"
                          >
                            {selected ? '[x]' : '[ ]'}
                          </span>
                        )}
                        <span className="min-w-0">
                          <span className="font-semibold text-fg">
                            {opt.label}
                          </span>
                          {opt.description && (
                            <span className="block text-xs text-fg-muted">
                              {opt.description}
                            </span>
                          )}
                        </span>
                      </button>
                      {/* The preview sits to the RIGHT of the option, side by
                          side (label/description left, preview right). It is a
                          sibling — not nested in the button — so a wide,
                          scrollable monospace block never intercepts the
                          option's click/selection. The label column is a fixed
                          width (`w-80` + `shrink-0`) so every preview starts on
                          the same vertical line instead of jittering with each
                          label's length; the preview takes the remaining width
                          (`flex-1` + `min-w-0` so long lines scroll inside the
                          block instead of stretching the row). Shown verbatim —
                          never through Markdown — to keep box drawing, code, and
                          ASCII art exact. */}
                      {opt.preview && (
                        <pre
                          data-testid={`question-option-preview-${qi}-${oi}`}
                          className="min-w-0 flex-1 overflow-x-auto whitespace-pre rounded border border-border-default bg-surface-elevated px-2 py-1 font-mono text-xs text-fg-muted"
                        >
                          {opt.preview}
                        </pre>
                      )}
                    </li>
                  );
                })}
              </ul>
            </li>
          ))}
        </ul>
      )}

      {failed && (
        <p className="text-danger" role="alert" data-testid="question-error">
          {failedAction === 'cancel'
            ? "Couldn't cancel the question — cancel it in the terminal, or try again."
            : "Couldn't submit your answer — answer in the terminal, or try again."}
        </p>
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
        {/* The terminal stays available as a fallback if injection misfires;
            a failed POST emphasizes it (solid, not ghost) as the way forward. */}
        <Button
          size="sm"
          variant={failed ? 'primary' : 'ghost'}
          onClick={onOpenTerminal}
        >
          Open terminal
        </Button>
        {/* Cancel does the terminal's Esc: it cancels the question in the TUI
            itself (a single Escape cancels the whole call), unlike Dismiss which
            only hides this card and leaves the TUI prompt open. Disabled while a
            submit/cancel is in flight so the two cannot race. */}
        <Button
          size="sm"
          variant="ghost"
          disabled={submitted}
          data-testid="question-cancel"
          onClick={cancel}
        >
          Cancel
        </Button>
        <Button size="sm" variant="ghost" onClick={onDismiss}>
          Dismiss
        </Button>
      </div>
    </div>
  );
}
