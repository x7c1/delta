//! The live sockets' bounds, driven over real connections: the events (`/ws`)
//! and comms (`/comms`) streams refuse an inbound message over their bound and
//! cap how many sockets may be open at once. The terminal bridge's message
//! bound is covered next to the bridge, in `crate::pty`, since driving it
//! needs a running PTY rather than a tmux pane.

use tokio_tungstenite::tungstenite::Message;

use super::{router, test_state, TEST_AUTH_TOKEN};
use crate::socket_limits::test_client::{
    assert_alive, assert_closed, assert_huge_frame_header_is_refused, connect, connect_eventually,
    refused_status, send_regardless, serve,
};
use crate::socket_limits::{EVENT_SOCKETS_CAP, IDLE_INBOUND_LIMIT, SESSION_SOCKETS_CAP};

fn events_path() -> String {
    format!("/ws?token={TEST_AUTH_TOKEN}")
}

fn comms_path(session_id: &str) -> String {
    format!("/comms?session_id={session_id}&token={TEST_AUTH_TOKEN}")
}

async fn serve_router() -> u16 {
    serve(router(test_state().await)).await
}

/// A message at the bound is accepted (the socket stays open and served); one
/// byte over it ends the socket.
async fn assert_inbound_bound(port: u16, path: &str) {
    let mut client = connect(port, path).await.unwrap();
    send_regardless(&mut client, Message::text("x".repeat(IDLE_INBOUND_LIMIT))).await;
    assert_alive(&mut client).await;

    send_regardless(
        &mut client,
        Message::text("x".repeat(IDLE_INBOUND_LIMIT + 1)),
    )
    .await;
    assert_closed(&mut client).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_event_stream_bounds_inbound_messages() {
    let port = serve_router().await;
    assert_inbound_bound(port, &events_path()).await;
    assert_huge_frame_header_is_refused(port, &events_path()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_comms_stream_bounds_inbound_messages() {
    let port = serve_router().await;
    assert_inbound_bound(port, &comms_path("s-1")).await;
    assert_huge_frame_header_is_refused(port, &comms_path("s-1")).await;
}

/// Opening one socket past the session's cap is refused with `429`; the cap
/// is per session, and closing one socket makes room for the next.
#[tokio::test(flavor = "multi_thread")]
async fn the_comms_stream_caps_sockets_per_session() {
    let port = serve_router().await;
    let mut open = Vec::new();
    for _ in 0..SESSION_SOCKETS_CAP {
        open.push(connect(port, &comms_path("s-cap")).await.unwrap());
    }

    assert_eq!(refused_status(port, &comms_path("s-cap")).await, 429);
    // Another session has a cap of its own.
    let _other = connect(port, &comms_path("s-other")).await.unwrap();

    let mut closing = open.pop().unwrap();
    closing.close(None).await.unwrap();
    assert_closed(&mut closing).await;
    let mut reopened = connect_eventually(port, &comms_path("s-cap")).await;
    assert_alive(&mut reopened).await;
}

/// A socket whose pump ends on a receive error — here an inbound message over
/// its bound — gives its slot back, exactly as a clean close does.
#[tokio::test(flavor = "multi_thread")]
async fn a_socket_ended_by_an_error_releases_its_slot() {
    let port = serve_router().await;
    let mut open = Vec::new();
    for _ in 0..SESSION_SOCKETS_CAP {
        open.push(connect(port, &comms_path("s-err")).await.unwrap());
    }
    assert_eq!(refused_status(port, &comms_path("s-err")).await, 429);

    let mut failing = open.pop().unwrap();
    send_regardless(
        &mut failing,
        Message::text("x".repeat(IDLE_INBOUND_LIMIT + 1)),
    )
    .await;
    assert_closed(&mut failing).await;

    let mut reopened = connect_eventually(port, &comms_path("s-err")).await;
    assert_alive(&mut reopened).await;
}

/// The event stream is capped across all clients, not per session.
#[tokio::test(flavor = "multi_thread")]
async fn the_event_stream_caps_sockets_globally() {
    let port = serve_router().await;
    let mut open = Vec::new();
    for _ in 0..EVENT_SOCKETS_CAP {
        open.push(connect(port, &events_path()).await.unwrap());
    }

    assert_eq!(refused_status(port, &events_path()).await, 429);

    let mut closing = open.pop().unwrap();
    closing.close(None).await.unwrap();
    assert_closed(&mut closing).await;
    let mut reopened = connect_eventually(port, &events_path()).await;
    assert_alive(&mut reopened).await;
}
