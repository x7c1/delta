import { useEffect, useId, useRef, type ReactNode, type RefObject } from 'react';
import { Button, Spinner } from '@delta/ui-kit';

interface ConfirmPanelProps {
  /** What is about to happen, and what it costs. */
  children: ReactNode;
  /** The confirming button's label, naming the action (`Remove 12 sessions`). */
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  /** Whether the confirming button is held back (a typed confirmation not yet matched). */
  confirmDisabled?: boolean;
  /** Whether the confirmed request is in flight. */
  pending?: boolean;
  /** The refusal or failure of the confirmed request, said under the buttons. */
  error?: string | null;
  testId?: string;
  /**
   * An element whose first enabled control opened the panel, to take the
   * focus back when the panel is cancelled. Read
   * once the panel is gone, so it may name the control mounted again in the
   * panel's place. Required so that no panel leaves the keyboard user's focus
   * on the page body.
   */
  returnFocusTo: RefObject<HTMLElement | null>;
}

/** The confirmation panels open now, in the order they opened. */
const openPanels: HTMLElement[] = [];

/**
 * The open panel an Escape pressed on `target` cancels: the one holding the
 * focus, otherwise the last one opened.
 */
function escapeOwner(target: EventTarget | null): HTMLElement | undefined {
  const holder =
    target instanceof Node ? openPanels.find((panel) => panel.contains(target)) : undefined;
  return holder ?? openPanels[openPanels.length - 1];
}

/**
 * An inline confirmation for a destructive Storage action, opened in place of
 * the control that asked for it.
 *
 * Inline rather than a modal: these actions live inside the Settings dialog,
 * and a second dialog over it would compete with it for Escape and focus. The
 * panel is an `alertdialog` named by its own message, so it is announced and
 * reachable as the question it is; Cancel puts the original control back.
 *
 * Opening it unmounts the control that had focus, so the panel takes focus
 * itself: the typed-name field when it has one, otherwise Cancel — never the
 * destructive button, so a second Enter does not confirm. Cancelled, by Cancel
 * or Escape, it hands the focus to `returnFocusTo` if the focus went with
 * the panel, so the keyboard user keeps their place.
 *
 * Escape cancels an open panel, as Cancel does, before it reaches the
 * Settings dialog, which closes on Escape only once no panel is open. The
 * panel listens on the document in the capture phase, ahead of the dialog's
 * own listener, so it wins wherever the focus is. While the confirmed request
 * is in flight, Cancel is held back and Escape does nothing.
 */
export function ConfirmPanel({
  children,
  confirmLabel,
  onConfirm,
  onCancel,
  confirmDisabled = false,
  pending = false,
  error = null,
  testId,
  returnFocusTo,
}: ConfirmPanelProps) {
  const messageId = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // Inputs sit in the message, ahead of the buttons, and Cancel is the first button.
    panelRef.current?.querySelector<HTMLElement>('input, button')?.focus();
  }, []);
  // Read by the Escape listener and the cleanup, both registered once for the panel's lifetime.
  const cancelRef = useRef({ onCancel, pending, returnFocusTo });
  useEffect(() => {
    cancelRef.current = { onCancel, pending, returnFocusTo };
  });
  const cancelledRef = useRef(false);
  const cancel = () => {
    cancelledRef.current = true;
    cancelRef.current.onCancel();
  };
  useEffect(() => {
    const panel = panelRef.current;
    if (panel === null) {
      return;
    }
    openPanels.push(panel);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.isComposing || escapeOwner(event.target) !== panel) {
        return;
      }
      event.stopPropagation();
      if (!cancelRef.current.pending) {
        cancel();
      }
    };
    document.addEventListener('keydown', onKeyDown, true);
    return () => {
      document.removeEventListener('keydown', onKeyDown, true);
      openPanels.splice(openPanels.indexOf(panel), 1);
      // Runs after the commit that removed the panel, so the control put back
      // in its place is mounted. Focus that moved elsewhere stays there.
      const opener = cancelRef.current.returnFocusTo.current;
      const dropped = document.activeElement === null || document.activeElement === document.body;
      if (cancelledRef.current && dropped && opener) {
        opener.querySelector<HTMLElement>('button:not(:disabled), input:not(:disabled)')?.focus();
      }
    };
  }, []);
  return (
    <div
      ref={panelRef}
      role="alertdialog"
      aria-labelledby={messageId}
      className="flex flex-col gap-2 rounded-sm border border-border-default bg-surface-sunken px-3 py-2"
      data-testid={testId}
    >
      <div id={messageId} className="flex flex-col gap-2 text-caption text-fg">
        {children}
      </div>
      <div className="flex items-center justify-end gap-2">
        {pending && <Spinner aria-label="working" />}
        <Button size="sm" variant="ghost" onClick={cancel} disabled={pending}>
          Cancel
        </Button>
        <Button
          size="sm"
          variant="primary"
          onClick={onConfirm}
          disabled={confirmDisabled || pending}
        >
          {confirmLabel}
        </Button>
      </div>
      {error && (
        <p className="text-caption text-danger" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
