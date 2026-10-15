//! The secrets the server always holds, minted when the environment hands in
//! none.

use std::ffi::OsString;

/// The environment variable that fixes the per-run bearer token in advance.
const AUTH_TOKEN_VAR: &str = "DELTA_AUTH_TOKEN";

/// The environment variable that fixes the hook secret in advance.
const HOOK_SECRET_VAR: &str = "DELTA_HOOK_SECRET";

/// The secrets a shell hands the configuration: the values of
/// `DELTA_AUTH_TOKEN` and `DELTA_HOOK_SECRET`, each `None` when the variable
/// is unset or not valid Unicode.
///
/// Read apart from the rest of the environment because reading them also
/// removes them from it ([`Secrets::take_from_env`]); [`Secrets::default`]
/// names neither.
#[derive(Default)]
pub struct Secrets {
    /// The value of `DELTA_AUTH_TOKEN`; empty or `None` mints one.
    pub(super) auth_token: Option<String>,
    /// The value of `DELTA_HOOK_SECRET`; empty or `None` leaves it to the
    /// data directory's hook state.
    pub(super) hook_secret: Option<String>,
}

impl Secrets {
    /// Read both secrets from the process environment, and remove them from
    /// it.
    ///
    /// Every command the server starts — tmux, and through it each agent,
    /// git, gh, the editor opener — inherits the process environment. Both
    /// secrets are held in the configuration once they are read
    /// ([`delta_bootstrap::Config::hook_secret_from_env`] records whether the
    /// hook secret came from the variable), so nothing the server starts needs
    /// them, and none of them should be able to read them: an agent runs
    /// whatever its session asks, and its environment is visible to anything
    /// it runs in turn. The hook secret is what lets a local process post as
    /// one of Delta's sessions, so it is withheld like the token.
    ///
    /// Must be called before the process starts any other thread: changing
    /// the environment while another thread may read it is undefined
    /// behaviour in libc. The `delta-server` binary calls it (through
    /// [`super::config_from_env`]) before its runtime starts, the desktop
    /// shell as the first statement of its `main`, before Tauri, logging or
    /// any thread. No other code removes these variables.
    pub fn take_from_env() -> Self {
        let secrets = Self::from_vars(|name| std::env::var_os(name));
        std::env::remove_var(AUTH_TOKEN_VAR);
        std::env::remove_var(HOOK_SECRET_VAR);
        secrets
    }

    /// The secrets `var` names, answering like [`std::env::var_os`]. Removes
    /// nothing.
    pub(super) fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let text = |name: &str| var(name).and_then(|value| value.into_string().ok());
        Self {
            auth_token: text(AUTH_TOKEN_VAR),
            hook_secret: text(HOOK_SECRET_VAR),
        }
    }
}

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
