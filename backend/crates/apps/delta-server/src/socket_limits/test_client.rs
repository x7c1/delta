//! A WebSocket client for the live-socket tests: connect over a real loopback
//! connection, and observe whether the server keeps a socket open or ends it.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// A connected test client.
pub(crate) type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How long a test waits for the server to answer or to end a socket.
pub(crate) const PATIENCE: Duration = Duration::from_secs(10);

/// Serve `app` on an ephemeral loopback port, returning the port.
pub(crate) async fn serve(app: axum::Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await });
    port
}

/// Open a socket at `path_and_query` (which carries any query the endpoint
/// needs, the bearer token included).
pub(crate) async fn connect(port: u16, path_and_query: &str) -> Result<Client, tungstenite::Error> {
    tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}{path_and_query}"))
        .await
        .map(|(client, _response)| client)
}

/// The HTTP status an upgrade was refused with, panicking if it was accepted.
pub(crate) async fn refused_status(port: u16, path_and_query: &str) -> u16 {
    match connect(port, path_and_query).await {
        Err(tungstenite::Error::Http(response)) => response.status().as_u16(),
        Err(err) => panic!("the upgrade failed without an HTTP refusal: {err}"),
        Ok(_) => panic!("the upgrade to {path_and_query} was accepted"),
    }
}

/// Keep retrying an upgrade until it is accepted, panicking after
/// [`PATIENCE`]. A slot is released by the server's socket task as it ends,
/// which happens shortly after — not at the moment — the client sees the
/// socket close.
pub(crate) async fn connect_eventually(port: u16, path_and_query: &str) -> Client {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        match connect(port, path_and_query).await {
            Ok(client) => return client,
            Err(err) if tokio::time::Instant::now() >= deadline => {
                panic!("the upgrade to {path_and_query} was never accepted again: {err}")
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
}

/// Assert the socket is still open and served: a ping is answered with its
/// pong.
pub(crate) async fn assert_alive(client: &mut Client) {
    client
        .send(Message::Ping(b"alive".to_vec().into()))
        .await
        .expect("the ping is sent");
    loop {
        match tokio::time::timeout(PATIENCE, client.next()).await {
            Ok(Some(Ok(Message::Pong(payload)))) => {
                assert_eq!(&payload[..], b"alive");
                return;
            }
            Ok(Some(Ok(_))) => continue,
            Ok(other) => panic!("the socket ended instead of answering the ping: {other:?}"),
            Err(_) => panic!("the ping was never answered"),
        }
    }
}

/// Assert the server ends the socket: a close frame, an error, or the stream's
/// end arrives within [`PATIENCE`]. Data the server sent before ending it is
/// skipped.
pub(crate) async fn assert_closed(client: &mut Client) {
    loop {
        match tokio::time::timeout(PATIENCE, client.next()).await {
            Ok(Some(Ok(Message::Close(_)))) | Ok(Some(Err(_))) | Ok(None) => return,
            Ok(Some(Ok(_))) => continue,
            Err(_) => panic!("the server kept the socket open"),
        }
    }
}

/// Send `message`, ignoring a failed write: the server may end the socket as
/// soon as it has read the frame header, before the client finishes writing.
pub(crate) async fn send_regardless(client: &mut Client, message: Message) {
    let _ = client.send(message).await;
}

/// Upgrade a raw connection to `path_and_query`, send only the header of a
/// binary frame that announces 1 GiB, and assert the server ends the connection
/// without waiting for the payload — which is never sent, so the server cannot
/// have buffered it.
pub(crate) async fn assert_huge_frame_header_is_refused(port: u16, path_and_query: &str) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let request = format!(
        "GET {path_and_query} HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        let read = tokio::time::timeout(PATIENCE, stream.read(&mut byte))
            .await
            .expect("the upgrade is answered")
            .unwrap();
        assert_ne!(read, 0, "the connection closed during the handshake");
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head);
    assert!(
        head.starts_with("HTTP/1.1 101"),
        "the upgrade is accepted: {head}"
    );

    // FIN + binary; masked (as every client frame must be) with a 64-bit
    // length of 1 GiB; then the masking key. No payload follows.
    let mut header = vec![0x82, 0x80 | 127];
    header.extend_from_slice(&(1u64 << 30).to_be_bytes());
    header.extend_from_slice(&[1, 2, 3, 4]);
    stream.write_all(&header).await.unwrap();

    let mut sink = [0u8; 1024];
    loop {
        match tokio::time::timeout(PATIENCE, stream.read(&mut sink)).await {
            Ok(Ok(0)) | Ok(Err(_)) => return,
            // A close frame the server sends on its way out.
            Ok(Ok(_)) => continue,
            Err(_) => panic!("the server waited for the announced payload"),
        }
    }
}
