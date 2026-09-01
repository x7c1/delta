# Security & trust model

## Overview

delta-server runs on your machine and binds the loopback interface only. That
keeps it off the network, but loopback binding is *not* an authentication
boundary: any process or web page on the same host can still reach the port.
Delta therefore treats **reaching the loopback port as the trust boundary**
("unauthenticated-by-port") and layers explicit guards on top of it. This
document states what each guard covers and the one deliberate trade-off in how
Delta pre-accepts Claude Code's workspace-trust dialog.

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
