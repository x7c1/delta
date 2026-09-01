# Security & trust model

## Overview

delta-server runs on your machine and binds the loopback interface only. That
keeps it off the network, but loopback binding is *not* an authentication
boundary: any process or web page on the same host can still reach the port.
Delta therefore treats **reaching the loopback port as the trust boundary**
("unauthenticated-by-port") and layers explicit guards on top of it. This
document states what each guard covers, the one deliberate trade-off in how
Delta pre-accepts Claude Code's workspace-trust dialog, and how Delta handles the
launch options that switch an agent's own safety mechanisms off.

This is a living document: it describes the guards in place today, and later
hardening work will extend it.

## The loopback trust boundary

delta-server listens on loopback only, so a remote host cannot connect. Within
the local machine, four guards defend the surface:

- **Origin/Host guard** — rejects a request whose `Host` is not a loopback host
  (blocking DNS rebinding), and one that carries a *present* but non-loopback
  `Origin` (blocking a foreign web page from driving the API through the
  browser). A same-host non-browser client (e.g. `curl`) sends no `Origin` and a
  loopback `Host`, so it passes this guard — which is why the next one exists.
- **Per-run bearer token** — a secret minted once per server run and required on
  every REST call (`Authorization: Bearer <token>`) and live socket
  (`token=<token>` query parameter). Anything without the valid token gets
  `401`. This closes the gap the Origin/Host guard leaves open for a local
  non-browser process. Two paths are exempt from *this* guard: `/health` (an
  unauthenticated liveness probe) and `/hooks/*`, which carries the per-run hook
  secret instead of a bearer token (next guard) rather than being left open.
- **Per-run hook secret** — the `/hooks/*` control plane is called by Claude
  Code (not the browser), so it cannot carry a bearer token. Instead Delta
  renders a per-run secret into the hook URLs (`?hs=<secret>`) and requires it
  on every hook request, giving that path its own per-run authentication.
- **Scoped trust seeding** — the subject of the next section: Delta pre-accepts
  Claude Code's workspace-trust dialog only for directories it created itself.

## The Claude Code trust-seeding trade-off

Claude Code shows a blocking "Do you trust the files in this folder?" dialog the
first time it launches in a directory, and records the answer per absolute path
in the user's **global** `~/.claude.json`. A non-interactive launch (Delta's
spawn flow) cannot answer that dialog, so Delta pre-seeds the acceptance for the
directory it is about to launch in.

Writing that acceptance is consequential: it is global, so it also suppresses
the dialog in your own plain `claude` sessions in that directory — which means
any automation checked into that directory (a `.claude/settings.json` with
hooks) would then run without ever asking you.

Delta therefore scopes trust seeding narrowly:

- **Delta's own worktrees are auto-trusted.** When Delta creates a git worktree
  under its own worktree base, it pre-accepts the dialog for that worktree path.
  Delta made the directory, so nothing checked into it is a surprise.
- **A directory you point Delta at is not.** If you launch a session in an
  existing repository you selected yourself (or reuse a working tree outside the
  worktree base), Delta does **not** pre-accept the dialog. Claude Code shows its
  normal one-time trust dialog instead; once you accept, it is remembered. Delta
  stays out of that decision, because pre-accepting it would also silently trust
  that repository's checked-in automation in your plain `claude` sessions.

The gate is a strict "is this path under Delta's worktree base?" check:
directories are canonicalized (so a `/tmp` vs `/private/tmp` symlink or a `..`
cannot disguise a path) and compared by path components (so a sibling like
`<base>-evil` is not mistaken for a child of `<base>`).

## Safety-bypassing launch options

A launch option is otherwise a pass-through — Delta does not read the names or
values, because the agent that receives them owns that vocabulary. A handful of
them, though, do not configure the agent so much as disable its guardrails:
Claude's `--dangerously-skip-permissions` (and `--permission-mode
bypassPermissions`), Codex's `sandbox = danger-full-access` and `approvalPolicy =
never`, and the same two settings written inside a Codex `config` value.

Delta's position is **marking, not prohibition**: those options stay registrable
and stay selectable for an individual session, because there are legitimate uses
and a tool that quietly removes the capability just gets worked around. What they
may never be is *silent*:

- **They cannot be enabled by default.** Setting `default_enabled` on such an
  option is refused (`400 launch_option_rejected`) on both the create and the
  update path, because a pre-checked bypass would disarm every new session
  without the user saying so even once. Turning the flag off is always allowed,
  so a row registered before this rule can be disarmed.
- **No shipped built-in is dangerous.** Startup reconciliation preserves each
  shipped row's `default_enabled`, which makes it the one writer that could
  reintroduce a pre-checked bypass past those refusals; a guard test in the
  composition root fails the build if a dangerous preset is ever added to a
  catalog.
- **They are marked, and never pre-checked, in the UI.** The registry badges such
  a row and disables its default control — except on a row that already carries
  the flag from before this rule, where the control stays live so the stored
  default can be cleared; the session-start picker badges it, refuses to
  pre-check it even if a stored row still says `default_enabled`, and reveals an
  inline warning naming the option once it is selected.

Recognising which `(name, value)` pairs qualify needs the provider's own
vocabulary, so the predicate lives in each agent gateway and is exposed to the
rest of Delta through one port. The verdict is **derived, never stored**: a row
registered before a spelling was recognised starts being flagged as soon as the
predicate learns it, with no migration. It is deliberately a closed list of
spellings that mean "stop asking" — an option that merely widens what is
auto-approved (`--permission-mode acceptEdits`) is not on it, because the
permission system is still running.
