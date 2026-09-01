//! The one place a route is mounted.
//!
//! An axum `Router` cannot be asked what it routes once it is built, so a
//! router assembled by hand can quietly serve a path nobody declared, or miss
//! one that is declared — which is how an API surface drifts away from its
//! written contract. The binder closes that gap by mounting only what
//! [`delta_wire::endpoint`] declares: every registration names a declaration
//! (so its method and path come from the contract, not from a string typed
//! here) and is recorded as it happens. Before handing the router over,
//! [`RouteBinder::finish`] compares the recorded set with [`ENDPOINTS`] in both
//! directions. A disagreement panics during construction — in every `router()`
//! call, test or real boot alike — rather than being discovered by a client.

use std::collections::BTreeSet;

use axum::handler::Handler;
use axum::routing::{delete, get, patch, post};
use axum::Router;

use delta_wire::endpoint::{route_label, Endpoint, Method, ENDPOINTS};

use crate::state::AppState;

/// Builds the router, mounting one declared endpoint per [`bind`](Self::bind).
pub(crate) struct RouteBinder {
    router: Router<AppState>,
    /// The routes mounted so far, as [`route_label`] spells them.
    mounted: Vec<String>,
}

impl RouteBinder {
    pub(crate) fn new() -> Self {
        Self {
            router: Router::new(),
            mounted: Vec::new(),
        }
    }

    /// Mounts `handler` at the route its endpoint declaration names.
    ///
    /// The marker is taken by value purely so the call site reads as the
    /// binding it is (`bind(endpoint::CreateSend, api::create_send)`);
    /// everything the binder needs comes from the trait's constants.
    /// Registering one endpoint per call is safe even where two methods share a
    /// path, since axum merges method routers per path.
    pub(crate) fn bind<E, H, T>(mut self, _endpoint: E, handler: H) -> Self
    where
        E: Endpoint,
        H: Handler<T, AppState>,
        T: 'static,
    {
        let method_router = match E::METHOD {
            Method::Get => get(handler),
            Method::Post => post(handler),
            Method::Patch => patch(handler),
            Method::Delete => delete(handler),
        };
        self.router = self.router.route(E::PATH, method_router);
        self.mounted.push(route_label(E::METHOD, E::PATH));
        self
    }

    /// Checks the mounted surface against the declared one and returns the
    /// finished router.
    ///
    /// # Panics
    ///
    /// If the two disagree in either direction — see
    /// [`assert_mounts_the_declared_surface`].
    pub(crate) fn finish(self, state: AppState) -> Router {
        assert_mounts_the_declared_surface(&self.mounted);
        // Three guards wrap the whole surface. Layers apply outermost-last, so a
        // request flows through the bearer-token guard first, then the hook
        // guard, then the origin/host guard, then the handler:
        //
        //   auth_guard (bearer token) → hook_auth_guard (hs secret)
        //     → origin_guard (Origin/Host) → handler
        //
        // The bearer guard is outermost deliberately: it is the coarser gate (a
        // local non-browser caller with no token is refused before its origin is
        // even inspected), and it keeps the origin guard independently testable —
        // the origin tests present a valid token, so a foreign origin still fails
        // at the origin check with 403, not 401. The two auth guards partition
        // the surface by path: the bearer guard covers everything except
        // `/hooks/*` and `/health`, while the hook guard covers `/hooks/*` (and
        // passes everything else through), so a hook needs the `hs` secret but no
        // bearer token, and an `/api/*` request the reverse. The origin/host
        // guard covers every path. See [`crate::auth_guard`],
        // [`crate::hook_auth_guard`], and [`crate::origin_guard`].
        let auth = axum::middleware::from_fn_with_state(state.clone(), crate::auth_guard::guard);
        let hook_auth =
            axum::middleware::from_fn_with_state(state.clone(), crate::hook_auth_guard::guard);
        self.router
            .with_state(state)
            .layer(axum::middleware::from_fn(crate::origin_guard::guard))
            .layer(hook_auth)
            .layer(auth)
    }
}

/// Asserts that `mounted` is exactly the surface [`ENDPOINTS`] declares.
///
/// Comparing the two as sets is enough: mounting one route twice never reaches
/// here, because axum rejects the second `route` call with an overlapping-method
/// panic that already names the route.
///
/// # Panics
///
/// If a declared endpoint was never mounted, or if a route was mounted that no
/// declaration covers — naming the offending routes in each case.
fn assert_mounts_the_declared_surface(mounted: &[String]) {
    let declared_labels: Vec<String> = ENDPOINTS
        .iter()
        .map(|spec| route_label(spec.method, spec.path))
        .collect();
    let declared: BTreeSet<&str> = declared_labels.iter().map(String::as_str).collect();
    let mounted: BTreeSet<&str> = mounted.iter().map(String::as_str).collect();

    let unmounted: BTreeSet<&str> = declared.difference(&mounted).copied().collect();
    assert!(
        unmounted.is_empty(),
        "declared in delta_wire::endpoint::ENDPOINTS but not mounted: {}",
        join(&unmounted),
    );

    let undeclared: BTreeSet<&str> = mounted.difference(&declared).copied().collect();
    assert!(
        undeclared.is_empty(),
        "mounted but not declared in delta_wire::endpoint::ENDPOINTS: {}",
        join(&undeclared),
    );
}

/// Renders the offending routes of a failed check into one readable list.
fn join(labels: &BTreeSet<&str>) -> String {
    labels.iter().copied().collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared_labels() -> Vec<String> {
        ENDPOINTS
            .iter()
            .map(|spec| route_label(spec.method, spec.path))
            .collect()
    }

    /// Runs the check over `mounted` and returns the message it panicked with.
    ///
    /// The panic is the behaviour under test, so the "thread panicked at …"
    /// line these tests print to stderr is expected output, not a failure.
    fn rejection_of(mounted: Vec<String>) -> String {
        let panic = std::panic::catch_unwind(|| assert_mounts_the_declared_surface(&mounted))
            .expect_err("the check accepted a surface it should have rejected");
        panic
            .downcast_ref::<String>()
            .expect("an assert! panics with a formatted String")
            .clone()
    }

    #[test]
    fn accepts_exactly_the_declared_surface() {
        assert_mounts_the_declared_surface(&declared_labels());
    }

    #[test]
    fn rejects_a_declared_endpoint_that_was_never_mounted() {
        let mut mounted = declared_labels();
        let forgotten = mounted.pop().expect("the table is not empty");

        let message = rejection_of(mounted);
        assert!(
            message.contains("not mounted") && message.contains(&forgotten),
            "the panic must name the forgotten route, got: {message}",
        );
    }

    #[test]
    fn rejects_a_route_no_declaration_covers() {
        let mut mounted = declared_labels();
        mounted.push("GET /api/undeclared".to_string());

        let message = rejection_of(mounted);
        assert!(
            message.contains("not declared") && message.contains("GET /api/undeclared"),
            "the panic must name the undeclared route, got: {message}",
        );
    }
}
