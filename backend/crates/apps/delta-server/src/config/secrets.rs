//! The secrets the server always holds, minted when the environment hands in
//! none.

/// The per-run bearer token the API and live sockets require.
///
/// `scripts/dev.sh` mints one token and exports `DELTA_AUTH_TOKEN` into both the
/// backend and the Vite dev server, so the two agree without a startup race. A
/// bare `cargo run -p delta-server` sets nothing, so mint a random fallback here
/// — the server must always hold a token, and an empty one would let every
/// request through.
pub(super) fn auth_token(from_env: Option<String>) -> String {
    from_env
        .filter(|token| !token.is_empty())
        .unwrap_or_else(mint_secret)
}

/// The hook secret every `/hooks/*` callback must carry as `?hs=`, as far as
/// the environment alone can settle it.
///
/// Rendered into the session's hook URLs, so genuine Claude Code callbacks
/// present it and a forged local POST is refused. `DELTA_HOOK_SECRET` overrides
/// it (kept as a seam symmetrical with `DELTA_AUTH_TOKEN`); otherwise a random
/// one is minted — the server must always hold a non-empty secret, since an
/// empty one would authenticate every hook request. The binaries then replace a
/// minted one with the secret kept in the data directory (see
/// [`super::adopt_persisted_hook_secret`]), so it survives restarts.
pub(super) fn hook_secret(from_env: Option<String>) -> String {
    from_env
        .filter(|secret| !secret.is_empty())
        .unwrap_or_else(mint_secret)
}

/// Two concatenated random (v4) UUIDs: 244 bits of entropy rendered as 64 hex
/// characters.
pub(super) fn mint_secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
