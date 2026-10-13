import type {
  CommsDirection,
  CommsFrame,
  CommsFrameKind,
} from '@delta/wire-gen';

/**
 * A scripted comms log for the mock-mode Codex session: one launch plus one turn
 * of the frames Delta and a `codex app-server` actually exchange.
 *
 * MSW cannot mock WebSockets, so mock mode has no `/comms` stream to read (the
 * same reason `/ws` is replaced by {@link FakeEventSource}). This is the
 * equivalent stand-in, which is what makes the comms pane demonstrable with no
 * backend — in `make mock` and in the end-to-end suite alike.
 *
 * Deliberately covers every (direction, kind) pair the real stream carries, so a
 * rendering bug in any one of them shows up: Delta's requests and the responses
 * to them, the server's pushed notifications, its own approval request, and
 * Delta's answer to that request. Sequence numbers are contiguous from 0 and
 * timestamps increase, exactly as the server mints them.
 */
export function mockCommsFrames(): CommsFrame[] {
  const startedAt = Date.UTC(2026, 0, 1, 9, 30, 0);
  const frames: [CommsDirection, CommsFrameKind, string | null, unknown][] = [
    ['to_agent', 'request', 'thread/start', { id: 1, method: 'thread/start', params: { cwd: '/home/dev/projects/delta' } }],
    ['from_agent', 'response', 'thread/start', { id: 1, result: { thread: { id: 'thr_mock_0001' }, model: 'gpt-5.6-sol' } }],
    ['to_agent', 'request', 'turn/start', { id: 2, method: 'turn/start', params: { threadId: 'thr_mock_0001', input: [{ type: 'text', text: 'add a test' }] } }],
    ['from_agent', 'response', 'turn/start', { id: 2, result: { turn: { id: 'turn_mock_0001' } } }],
    ['from_agent', 'notification', 'turn/started', { method: 'turn/started', params: { threadId: 'thr_mock_0001', turnId: 'turn_mock_0001' } }],
    // A streaming burst — long enough for the pane to fold it into a group row.
    ['from_agent', 'notification', 'item/agentMessage/delta', { method: 'item/agentMessage/delta', params: { threadId: 'thr_mock_0001', itemId: 'm1', delta: 'Adding' } }],
    ['from_agent', 'notification', 'item/agentMessage/delta', { method: 'item/agentMessage/delta', params: { threadId: 'thr_mock_0001', itemId: 'm1', delta: ' the' } }],
    ['from_agent', 'notification', 'item/agentMessage/delta', { method: 'item/agentMessage/delta', params: { threadId: 'thr_mock_0001', itemId: 'm1', delta: ' test' } }],
    ['from_agent', 'notification', 'item/agentMessage/delta', { method: 'item/agentMessage/delta', params: { threadId: 'thr_mock_0001', itemId: 'm1', delta: ' now.' } }],
    ['from_agent', 'request', 'item/commandExecution/requestApproval', { id: 'srv-1', method: 'item/commandExecution/requestApproval', params: { threadId: 'thr_mock_0001', itemId: 'exec_1', command: 'cargo test' } }],
    ['to_agent', 'response', null, { id: 'srv-1', result: { decision: 'accept' } }],
    ['from_agent', 'notification', 'item/completed', { method: 'item/completed', params: { threadId: 'thr_mock_0001', item: { id: 'm1', type: 'agentMessage', text: 'Added the test.' } } }],
    ['from_agent', 'notification', 'turn/completed', { method: 'turn/completed', params: { threadId: 'thr_mock_0001', turnId: 'turn_mock_0001', status: 'completed' } }],
  ];
  return frames.map(([direction, kind, method, payload], index) => ({
    seq: index,
    at_ms: startedAt + index * 120,
    direction,
    kind,
    method,
    payload_json: JSON.stringify(payload),
  }));
}
