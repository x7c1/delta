/**
 * The AskUserQuestion notices `applyEvent` raises and clears.
 *
 * Shared helpers live in liveStore.testkit.ts.
 */
import { beforeEach, describe, expect, it } from 'vitest';
import { noticeOf, useLiveStore } from './liveStore';
import { notices, reset } from './liveStore.testkit';

describe('liveStore.applyEvent question notices', () => {
  beforeEach(reset);

  const QUESTION_INPUT = '{"questions":[{"header":"Pick"}]}';
  const QUESTION_THREAD = 3;
  const QUESTION_NOTICE = {
    kind: 'question',
    requestId: 5,
    threadId: QUESTION_THREAD,
    toolInput: QUESTION_INPUT,
    dismissed: false,
  } as const;

  function askQuestion(sessionId = 'sess-1', requestId = 5) {
    useLiveStore.getState().applyEvent({
      kind: 'question_asked',
      session_id: sessionId,
      request_id: requestId,
      thread_id: QUESTION_THREAD,
      tool_input: QUESTION_INPUT,
    });
  }

  it('records a question notice on question_asked and flags it dismissed on dismiss', () => {
    askQuestion();
    expect(noticeOf(notices(), 'sess-1', 'question')).toEqual(QUESTION_NOTICE);

    useLiveStore.getState().dismissQuestion('sess-1');
    expect(noticeOf(notices(), 'sess-1', 'question')).toEqual({
      ...QUESTION_NOTICE,
      dismissed: true,
    });
  });

  it('clears a question notice when its request resolves (the user answered)', () => {
    askQuestion();

    // The correlated tool_result was ingested, so the same `permission_resolved`
    // event that clears a permission dialog clears the matching question.
    useLiveStore.getState().applyEvent({
      kind: 'permission_resolved',
      session_id: 'sess-1',
      request_id: 5,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toBeNull();
  });

  it('ignores a resolution for a different request than the current question', () => {
    askQuestion('sess-1', 6);

    useLiveStore.getState().applyEvent({
      kind: 'permission_resolved',
      session_id: 'sess-1',
      request_id: 5,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toEqual({
      ...QUESTION_NOTICE,
      requestId: 6,
    });
  });

  it('clears a question notice when its turn completes', () => {
    askQuestion();

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toBeNull();
  });

  it('clears a question notice when the session closes', () => {
    askQuestion();

    useLiveStore.getState().applyEvent({
      kind: 'session_closed',
      session_id: 'sess-1',
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toBeNull();
  });

  it('seedQuestion re-creates the notice the missed event would have set', () => {
    useLiveStore.getState().seedQuestion('sess-1', {
      request_id: 5,
      thread_id: QUESTION_THREAD,
      tool_input: QUESTION_INPUT,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toEqual(QUESTION_NOTICE);
  });

  it('seedQuestion refuses a question permission_resolved already resolved', () => {
    // The stale-envelope race: the sends response still carrying the question
    // resolves after the resolution event cleared the card.
    askQuestion();
    useLiveStore.getState().applyEvent({
      kind: 'permission_resolved',
      session_id: 'sess-1',
      request_id: 5,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toBeNull();

    useLiveStore.getState().seedQuestion('sess-1', {
      request_id: 5,
      thread_id: QUESTION_THREAD,
      tool_input: QUESTION_INPUT,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toBeNull();

    // A genuinely new question has a new id and still seeds.
    useLiveStore.getState().seedQuestion('sess-1', {
      request_id: 6,
      thread_id: QUESTION_THREAD,
      tool_input: QUESTION_INPUT,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toEqual({
      ...QUESTION_NOTICE,
      requestId: 6,
    });
  });

  it('seedQuestion never clears, and never un-dismisses the shown question', () => {
    askQuestion();
    useLiveStore.getState().dismissQuestion('sess-1');

    // A report of the SAME request must not resurrect the dismissed card.
    useLiveStore.getState().seedQuestion('sess-1', {
      request_id: 5,
      thread_id: QUESTION_THREAD,
      tool_input: QUESTION_INPUT,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')?.dismissed).toBe(true);

    // A null report clears nothing (clearing is owned by the events/sweeps).
    useLiveStore.getState().seedQuestion('sess-1', null);
    expect(noticeOf(notices(), 'sess-1', 'question')).not.toBeNull();

    // A DIFFERENT pending question replaces the entry, un-dismissed.
    useLiveStore.getState().seedQuestion('sess-1', {
      request_id: 9,
      thread_id: 7,
      tool_input: '{"questions":[]}',
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toEqual({
      kind: 'question',
      requestId: 9,
      threadId: 7,
      toolInput: '{"questions":[]}',
      dismissed: false,
    });
  });
});
