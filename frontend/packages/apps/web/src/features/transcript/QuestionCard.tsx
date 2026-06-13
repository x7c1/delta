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
  multiSelect?: boolean;
}

/**
 * Parse the raw `{questions:[…]}` tool input into the questions to render.
 *
 * Defensive by design: the payload comes straight off the wire, so anything
 * unparsable or shaped unexpectedly yields an empty list rather than throwing —
 * the card then shows its "answer in the terminal" guidance without options.
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
        typeof record.multiSelect === 'boolean' ? record.multiSelect : undefined,
    };
  });
}

export interface QuestionCardProps {
  notice: Notice;
  /** Open the embedded terminal (where the user actually answers). */
  onOpenTerminal: () => void;
  /** Dismiss the card without answering (the TUI prompt stays up). */
  onDismiss: () => void;
}

/**
 * The floating question card for Claude Code's `AskUserQuestion` tool: it
 * renders each question's header and prompt, and its options as a readable
 * list (label in bold, description beneath), so the user can read the choice in
 * Delta. Answering happens in the embedded terminal — there is deliberately NO
 * Allow/Deny and NO decision POST: a CLI hook cannot return the selected
 * option, so the TUI is the only answer path.
 *
 * Capped in height with internal scrolling so a large multi-question card never
 * blankets the whole transcript.
 */
export function QuestionCard({
  notice,
  onOpenTerminal,
  onDismiss,
}: QuestionCardProps) {
  const questions = parseQuestions(notice.toolInput);

  return (
    <div
      className="pointer-events-auto absolute right-overlay-inset top-overlay-inset flex max-h-[60%] max-w-sm flex-col gap-2 overflow-y-auto rounded border border-indigo-200 bg-indigo-50 px-3 py-2 text-xs shadow-md"
      data-testid="question-card"
      role="dialog"
      aria-label="Question from Claude Code"
    >
      <p className="font-medium text-indigo-900">Claude is asking a question</p>

      {questions.length === 0 ? (
        <p className="text-slate-600">
          Claude is asking a multiple-choice question.
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
                {q.options.map((opt, oi) => (
                  <li
                    key={oi}
                    className="rounded border border-indigo-100 bg-white px-2 py-1"
                  >
                    <span className="font-semibold text-slate-800">
                      {opt.label}
                    </span>
                    {opt.description && (
                      <span className="block text-slate-600">
                        {opt.description}
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            </li>
          ))}
        </ul>
      )}

      <p className="text-slate-600">Pick an option in the terminal.</p>
      <div className="flex gap-2">
        <Button size="sm" onClick={onOpenTerminal}>
          Open terminal
        </Button>
        <Button size="sm" variant="ghost" onClick={onDismiss}>
          Dismiss
        </Button>
      </div>
    </div>
  );
}
