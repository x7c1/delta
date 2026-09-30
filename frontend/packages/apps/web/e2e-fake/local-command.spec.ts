import { test, expect } from "./support/fixtures";
import { startNewSession, sendMessage } from "./support/app";
import { fetchSends, latestSession } from "./support/rest";

/**
 * A local slash command that Claude Code records nothing for still frees the
 * session, runs once, and lets the queue behind it move.
 *
 * From Claude Code 2.1.286 a local command such as `/cost` fires no
 * `UserPromptSubmit`, no `Stop`, and writes nothing to the transcript (earlier
 * versions wrote a caveat / command-name / stdout group Delta resolved the
 * send against). Silence is the only signal, so a slash-command send has a
 * short echo deadline of its own, and reaching it means the command ran: the
 * send settles as delivered with no `Escape` and no re-type — re-typing would
 * run the command a second time.
 *
 * Scenario `local-command`: the fake answers the positional first prompt
 * (`reply` + `stop`), then `local_command` consumes the `/cost` keystrokes and
 * fires nothing. The next `await_prompt` answers whatever is typed next; if the
 * command were typed again it would take that step, and the follow-up would be
 * left stuck in the pending strip.
 *
 * The slash-command deadline is server-wide, so this spec runs its own server
 * generation with a short one and restores the suite's value afterwards (see
 * `ServerHandle.restart`).
 */

/** The shortened slash-command echo deadline this spec's server runs with. */
const SLASH_COMMAND_ECHO_DEADLINE_MS = "3000";

test.afterEach(async ({ server }) => {
  // Restore the shared configuration even when the test failed, so the short
  // deadline cannot leak into the specs that follow.
  await server.restart();
});

test("a local slash command frees the session and the queued follow-up dispatches, without the command being typed twice", async ({
  page,
  server,
}) => {
  await server.restart({
    DELTA_SLASH_COMMAND_ECHO_DEADLINE_MS: SLASH_COMMAND_ECHO_DEADLINE_MS,
  });

  await page.goto("/");
  await startNewSession(page, "local-command opening prompt");

  // The positional first prompt is auto-submitted; the fake replies and stops,
  // so the session is idle before the command.
  await expect(page.getByText("session opened")).toBeVisible({
    timeout: 15_000,
  });
  const session = await latestSession(page);

  // Run the command. Nothing will ever be heard about it, so it stays in
  // progress while its short deadline runs.
  await sendMessage(page, "/cost");
  const pending = page.getByTestId("pending-item");
  await expect(pending.filter({ hasText: "/cost" })).toHaveCount(1, {
    timeout: 15_000,
  });

  // Composed while the command is outstanding: queued behind it.
  await sendMessage(page, "follow-up after the command");
  await expect(
    pending.filter({ hasText: "follow-up after the command" }),
  ).toHaveCount(1, { timeout: 15_000 });

  // The deadline settles the command and dispatches the follow-up, which the
  // fake answers — proof the command was not typed a second time, since a
  // re-typed `/cost` would have taken the fake's only remaining prompt.
  await expect(page.getByText("follow-up answered")).toBeVisible({
    timeout: 20_000,
  });
  await expect(pending).toHaveCount(0, { timeout: 20_000 });
  await expect(page.getByTestId("send-parked-notice")).toHaveCount(0);

  // Server-side truth: nothing is left open (the command settled rather than
  // being parked or requeued) and the session is idle.
  await expect(async () => {
    const sends = await fetchSends(page, session.id);
    expect(sends.sends).toHaveLength(0);
    expect(sends.turn.state).toBe("idle");
  }).toPass({ timeout: 20_000 });
  await expect(page.getByTestId("session-running")).toHaveCount(0, {
    timeout: 20_000,
  });
});
