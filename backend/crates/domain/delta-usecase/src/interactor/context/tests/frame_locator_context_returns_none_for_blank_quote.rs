use crate::interactor::context::frame_locator_context;

/// An empty or whitespace-only quote yields `None`, so nothing is injected.
#[test]
fn frame_locator_context_returns_none_for_blank_quote() {
    assert!(frame_locator_context("").is_none());
    assert!(frame_locator_context("   \n\t ").is_none());
}
