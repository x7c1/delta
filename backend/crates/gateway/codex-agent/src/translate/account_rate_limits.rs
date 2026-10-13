//! The account-scoped rate-limit state of one `codex app-server` connection.

use delta_usecase::{AgentEvent, RateLimitWindow};
use serde_json::Value;

use crate::wire::Notification;

use super::{int_field, METHOD_ACCOUNT_RATE_LIMITS};

/// The account-scoped rate-limit state observed on one `codex app-server`
/// connection, and the merge the vendored schema requires of its clients.
///
/// `account/rateLimits/updated` is a **sparse rolling update**: the schema says
/// in as many words that a client must merge the values it carries into the
/// snapshot it last observed, and that a null field "does not clear a previously
/// observed value" (see [`merge_window`] for what that means field by field).
/// This type is where that merge lives — at the provider's own edge, since the
/// merge rule is the provider's, not the core's.
///
/// It is connection-scoped rather than session-scoped because the account is:
/// one shared app-server connection hosts every Codex session, and these limits
/// describe the account behind it.
#[derive(Debug, Default)]
pub struct AccountRateLimits {
    /// The most significant window the account reports, as last observed.
    primary: Option<RateLimitWindow>,
    /// The secondary window, as last observed.
    secondary: Option<RateLimitWindow>,
}

impl AccountRateLimits {
    /// Fold one account-scoped notification into the observed state and project
    /// it into neutral events.
    ///
    /// Yields nothing for any notification this build does not model — including
    /// a rate-limit frame with no `rateLimits` object at all, which states
    /// nothing and must therefore not be turned into an empty window list that
    /// would wipe the display.
    pub fn translate(&mut self, n: &Notification) -> Vec<AgentEvent> {
        match n.method.as_str() {
            METHOD_ACCOUNT_RATE_LIMITS => match n.params.get("rateLimits") {
                Some(snapshot) => vec![AgentEvent::RateLimitsUpdated {
                    windows: self.merge(snapshot),
                }],
                None => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// Merge a sparse `RateLimitSnapshot` into the observed windows and return
    /// them, most significant first. A window the account does not report at all
    /// is simply absent from the list (its row disappears rather than showing
    /// zero).
    fn merge(&mut self, snapshot: &Value) -> Vec<RateLimitWindow> {
        self.primary = merge_window(self.primary.take(), snapshot.get("primary"));
        self.secondary = merge_window(self.secondary.take(), snapshot.get("secondary"));
        [self.primary.clone(), self.secondary.clone()]
            .into_iter()
            .flatten()
            .collect()
    }
}

/// Merge one sparse window onto the last observed one.
///
/// An absent or null window leaves the previous observation untouched. A present
/// one overwrites only the fields it actually carries: `usedPercent` is required
/// by the schema, while `resetsAt` and `windowDurationMins` are nullable and a
/// null there means "unavailable in this rolling update", not "cleared".
fn merge_window(
    previous: Option<RateLimitWindow>,
    update: Option<&Value>,
) -> Option<RateLimitWindow> {
    let Some(update) = update.filter(|value| !value.is_null()) else {
        // Nothing said about this window: keep exactly what was last observed
        // (including having observed nothing).
        return previous;
    };
    let previous = previous.unwrap_or_default();
    Some(RateLimitWindow {
        // Minutes on the wire, seconds in the neutral model — the unit the
        // browser labels and paces the row with.
        duration_seconds: int_field(update, "windowDurationMins")
            .map(|minutes| minutes * 60)
            .or(previous.duration_seconds),
        used_percentage: update
            .get("usedPercent")
            .and_then(Value::as_f64)
            .or(previous.used_percentage),
        resets_at: int_field(update, "resetsAt").or(previous.resets_at),
    })
}
