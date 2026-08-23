import {
  createContext,
  useContext,
  useMemo,
  useRef,
  type MutableRefObject,
  type ReactNode,
} from 'react';
import type { ThreadId } from '@delta/model';

/**
 * What a control OUTSIDE the composer card needs in order to edit the draft the
 * user is typing: which draft it is, the textarea showing it, and whether that
 * textarea has ever held the caret.
 */
export interface ComposerDraftTarget {
  /** The `composerStore.drafts` key the visible textarea is bound to. */
  draftKey: ThreadId;
  /** The live textarea node, or `null` while the composer is unmounted. */
  textareaRef: MutableRefObject<HTMLTextAreaElement | null>;
  /**
   * Whether the textarea has been focused at least once since the composer
   * mounted.
   *
   * A textarea that has never been focused reports `selectionStart === 0`,
   * which is indistinguishable from a caret deliberately parked at the very
   * beginning — so an inserter reading the selection blindly would prepend to a
   * draft the user has only pasted or restored, never clicked into. This flag
   * is what lets that case fall back to "insert at the end" instead.
   */
  everFocusedRef: MutableRefObject<boolean>;
}

const ComposerDraftTargetContext = createContext<ComposerDraftTarget | null>(
  null,
);

export interface ComposerDraftTargetProviderProps {
  /** The draft key of the composer rendered inside — see {@link composerDraftKey}. */
  draftKey: ThreadId;
  children: ReactNode;
}

/**
 * Shares the composer's textarea with the controls stacked AROUND it — today
 * the prompt-template button, which rides the rail above the card and so cannot
 * reach the textarea through props.
 *
 * The rail and the card are siblings by construction (the rail must stay in
 * normal flow above the card so the bottom overlay measures it — see
 * {@link ComposerRail}), so the only place that can hold something both of them
 * see is a provider wrapping the pair. It owns the refs and the composer fills
 * them in; a composer rendered with no provider above it (component tests that
 * exercise the input alone) simply keeps its own local ref.
 */
export function ComposerDraftTargetProvider({
  draftKey,
  children,
}: ComposerDraftTargetProviderProps) {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const everFocusedRef = useRef(false);
  const value = useMemo<ComposerDraftTarget>(
    () => ({ draftKey, textareaRef, everFocusedRef }),
    [draftKey],
  );
  return (
    <ComposerDraftTargetContext.Provider value={value}>
      {children}
    </ComposerDraftTargetContext.Provider>
  );
}

/**
 * The enclosing {@link ComposerDraftTargetProvider}'s target, or `null` when
 * there is none. Used by the composer itself, which must work either way.
 */
export function useOptionalComposerDraftTarget(): ComposerDraftTarget | null {
  return useContext(ComposerDraftTargetContext);
}

/**
 * The enclosing {@link ComposerDraftTargetProvider}'s target. For controls that
 * exist only to edit the composer's draft and are meaningless without it, so a
 * missing provider is a wiring bug rather than a state to render around.
 */
export function useComposerDraftTarget(): ComposerDraftTarget {
  const target = useContext(ComposerDraftTargetContext);
  if (!target) {
    throw new Error(
      'useComposerDraftTarget must be used within a ComposerDraftTargetProvider',
    );
  }
  return target;
}
