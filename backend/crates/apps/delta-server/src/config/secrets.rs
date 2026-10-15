//! The secrets the server always holds, minted when the environment hands in
//! none.

/// The environment variable that fixes the per-run bearer token in advance.
pub(super) const AUTH_TOKEN_VAR: &str = "DELTA_AUTH_TOKEN";

/// The environment variable that fixes the hook secret in advance.
pub(super) const HOOK_SECRET_VAR: &str = "DELTA_HOOK_SECRET";

/// How many characters a hook secret has: the 64 hexadecimal digits
/// [`mint_secret`] renders.
const HOOK_SECRET_LEN: usize = 64;

/// The per-run bearer token the API and live sockets require.
///
/// `DELTA_AUTH_TOKEN` fixes it in advance, which is what the test suites and
/// scripts that drive the server directly use. Otherwise — a plain run, `make
/// dev`, the desktop app — a random one is minted here: the server must always
/// hold a token, and an empty one would let every request through. A page
/// obtains it through a one-time code (`crate::one_time_codes`).
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
/// it (kept as a seam symmetrical with [`AUTH_TOKEN_VAR`]); otherwise a random
/// one is minted — the server must always hold a non-empty secret, since an
/// empty one would authenticate every hook request. The binaries then replace a
/// minted one with the secret kept in the data directory (see
/// [`super::adopt_persisted_hook_secret`]), so it survives restarts.
pub(super) fn hook_secret(from_env: Option<String>) -> String {
    from_env
        .filter(|secret| !secret.is_empty())
        .unwrap_or_else(mint_secret)
}

/// Whether `secret` has the form [`mint_secret`] gives a hook secret: 64
/// lowercase hexadecimal characters.
///
/// The hook secret is rendered into hook URLs and `curl` configuration files,
/// so only this form is adopted: it needs no escaping anywhere it goes, and a
/// short or hand-typed value cannot weaken what authenticates hook calls.
pub(crate) fn is_well_formed_hook_secret(secret: &str) -> bool {
    secret.len() == HOOK_SECRET_LEN
        && secret
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Two concatenated random (v4) UUIDs: 244 bits of entropy rendered as 64 hex
/// characters.
pub(crate) fn mint_secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
