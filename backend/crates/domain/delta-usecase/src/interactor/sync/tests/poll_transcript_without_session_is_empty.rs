use crate::interactor::testing::*;

/// `poll_transcript` is a no-op before any session is registered.
#[tokio::test]
async fn poll_transcript_without_session_is_empty() {
    let ix = interactor();
    let (polled, _events) = ix.poll_transcript().await.unwrap();
    assert!(polled.is_empty());
}
