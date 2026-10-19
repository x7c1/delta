import { isLinuxShell } from './shell';

/**
 * Ctrl-Z undo and Ctrl-Shift-Z redo in text fields, inside the Linux desktop
 * shell.
 *
 * WebKitGTK binds no key to undo, so without this the Linux app has none;
 * macOS gets Cmd-Z from the default Edit menu, and a browser has its own. The
 * history is WebKit's page-wide one, the same the macOS app's Edit menu acts
 * on: one step undoes a run of uninterrupted typing, and undoing past one
 * field's edits moves into another field's.
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

/** The command a key press asks for, or `null` when it is not ours to handle. */
function commandFor(event: KeyboardEvent): 'undo' | 'redo' | null {
  if (
    event.defaultPrevented ||
    event.isComposing ||
    !event.ctrlKey ||
    event.altKey ||
    event.metaKey ||
    event.key.toLowerCase() !== 'z' ||
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
  document.addEventListener('keydown', (event) => handleUndoKey(event, exec));
  return true;
}
