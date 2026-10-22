import { isLinuxShell } from './shell';

/**
 * Ctrl-Z undo and Ctrl-Shift-Z redo in text fields, inside the Linux desktop
 * shell.
 *
 * WebKitGTK binds no key to undo, so without this the Linux app has none;
 * macOS gets Cmd-Z from the default Edit menu, and a browser has its own. The
 * history is WebKit's page-wide one, the same the macOS app's Edit menu acts
 * on, so undoing past one field's edits moves into another field's.
 *
 * WebKit groups a whole run of uninterrupted typing, Backspace included, into
 * one undo step, so Ctrl-Z after deleting a few characters cannot bring just
 * those back. {@link TypingSteps} splits that run where an editor would.
 *
 * Z is the key labelled Z, or the key in QWERTY's Z position under a layout
 * without a Latin Z (Russian, Greek, Hebrew, ...); see {@link isZKey}.
 *
 * The terminal is left alone: Ctrl-Z there goes to the tmux pane (SIGTSTP).
 */

/** The `input` types whose value is edited as text. */
const TEXT_INPUT_TYPES = new Set(['text', 'search', 'url', 'tel', 'email', 'password', 'number']);

/** The selector of xterm.js's container, which holds its hidden helper textarea. */
const TERMINAL_SELECTOR = '.xterm';

/** Runs an editing command; `document.execCommand` in the page. */
export type ExecCommand = (command: 'undo' | 'redo') => void;

/** Whether `target` is a text field the user can type into, outside the terminal. */
function isEditableTextField(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement) || target.closest(TERMINAL_SELECTOR) !== null) {
    return false;
  }
  if (target instanceof HTMLTextAreaElement) {
    return !target.readOnly && !target.disabled;
  }
  if (target instanceof HTMLInputElement) {
    return TEXT_INPUT_TYPES.has(target.type) && !target.readOnly && !target.disabled;
  }
  // The nearest `contenteditable` attribute decides, as editing is inherited.
  const editingHost = target.closest('[contenteditable]');
  return editingHost !== null && editingHost.getAttribute('contenteditable') !== 'false';
}

/** A single printable ASCII character, as `KeyboardEvent.key` reports one. */
const ASCII_CHARACTER = /^[\x20-\x7E]$/;

/**
 * Whether a key press is the Z key. An ASCII `event.key` decides, so a layout
 * that moves Z (Dvorak, AZERTY, QWERTZ) matches on the key labelled Z, and the
 * key in QWERTY's Z position does not undo when it types something else there
 * (`;` under Dvorak, `y` under QWERTZ). Any other `event.key` — a Cyrillic,
 * Greek or Hebrew letter under a layout without a Latin Z — falls back to the
 * physical key, `event.code`, as editors and browsers do for shortcuts.
 */
function isZKey(event: KeyboardEvent): boolean {
  if (ASCII_CHARACTER.test(event.key)) {
    return event.key.toLowerCase() === 'z';
  }
  return event.code === 'KeyZ';
}

/** The command a key press asks for, or `null` when it is not ours to handle. */
function commandFor(event: KeyboardEvent): 'undo' | 'redo' | null {
  if (
    event.defaultPrevented ||
    event.isComposing ||
    !event.ctrlKey ||
    event.altKey ||
    event.metaKey ||
    !isZKey(event) ||
    !isEditableTextField(event.target)
  ) {
    return null;
  }
  return event.shiftKey ? 'redo' : 'undo';
}

/**
 * Run undo or redo for a Ctrl-Z / Ctrl-Shift-Z key press in a text field, and
 * keep the key from going further. Any other key press is left alone.
 */
export function handleUndoKey(event: KeyboardEvent, exec: ExecCommand): void {
  const command = commandFor(event);
  if (command === null) {
    return;
  }
  exec(command);
  event.preventDefault();
}

/** How long a pause in typing ends an undo step, in milliseconds. */
export const TYPING_PAUSE_MS = 1000;

/** What a key press does to a field's text, as far as undo steps go. */
type TypingKind = 'insert' | 'delete';

/** The typing kind of a plain key press, or `null` for any other key. */
function typingKindOf(event: KeyboardEvent): TypingKind | null {
  if (event.ctrlKey || event.altKey || event.metaKey) {
    return null;
  }
  if (event.key === 'Backspace' || event.key === 'Delete') {
    return 'delete';
  }
  return event.key.length === 1 || event.key === 'Enter' ? 'insert' : null;
}

/**
 * Splits WebKit's run of typing into undo steps: a new step starts when the
 * typing switches between inserting and deleting, at a space or a line break,
 * after a pause of {@link TYPING_PAUSE_MS}, in a different field, and when an
 * input method commits its text.
 *
 * It works on keydown, before WebKit applies the key: by the time `beforeinput`
 * fires, WebKit has already added the edit to the open step. Setting the
 * selection to where it already is closes that step, and the next edit opens a
 * new one. Only fields with a text selection (`textarea`, text-like `input`)
 * are split; key presses during IME composition are left to the input method.
 */
export class TypingSteps {
  private lastKind: TypingKind | null = null;
  private lastAt = 0;
  private lastTarget: EventTarget | null = null;

  /** Record a key press, closing the open undo step first when a new one starts. */
  onKeyDown(event: KeyboardEvent, now: number): void {
    const kind = event.isComposing ? null : typingKindOf(event);
    const field = kind === null ? null : selectableField(event.target);
    if (kind === null || field === null) {
      this.lastKind = null;
      return;
    }
    const startsStep =
      this.lastKind !== null &&
      (event.target !== this.lastTarget ||
        kind !== this.lastKind ||
        now - this.lastAt > TYPING_PAUSE_MS ||
        (kind === 'insert' && (event.key === ' ' || event.key === 'Enter')));
    if (startsStep) {
      closeStep(field);
    }
    this.lastKind = kind;
    this.lastAt = now;
    this.lastTarget = event.target;
  }

  /**
   * Close the undo step when the input method commits its text, so each
   * committed conversion is its own step. Its key presses arrive as
   * composing, which {@link onKeyDown} leaves alone, so without this a
   * Backspace right after the commit would join the committed text's step.
   */
  onCompositionEnd(event: CompositionEvent): void {
    const field = selectableField(event.target);
    if (field !== null) {
      closeStep(field);
    }
    this.lastKind = null;
  }
}

/** Close WebKit's open typing step by setting the selection to where it already is. */
function closeStep(field: HTMLTextAreaElement | HTMLInputElement): void {
  field.setSelectionRange(field.selectionStart, field.selectionEnd, field.selectionDirection ?? undefined);
}

/** `target` when it is a text field with a text selection, outside the terminal. */
function selectableField(target: EventTarget | null): HTMLTextAreaElement | HTMLInputElement | null {
  if (!isEditableTextField(target)) {
    return null;
  }
  if (target instanceof HTMLTextAreaElement) {
    return target;
  }
  // A `number` input has no text selection: setting one throws.
  if (target instanceof HTMLInputElement && target.selectionStart !== null) {
    return target;
  }
  return null;
}

/**
 * Install the undo keys on the whole document, inside the Linux shell only.
 * Returns whether they were installed.
 */
export function installUndoKeys(): boolean {
  if (!isLinuxShell()) {
    return false;
  }
  const exec: ExecCommand = (command) => {
    // Deprecated, but the only way to reach the engine's own undo history.
    document.execCommand(command);
  };
  const steps = new TypingSteps();
  document.addEventListener('keydown', (event) => {
    handleUndoKey(event, exec);
    steps.onKeyDown(event, performance.now());
  });
  document.addEventListener('compositionend', (event) => steps.onCompositionEnd(event));
  return true;
}
