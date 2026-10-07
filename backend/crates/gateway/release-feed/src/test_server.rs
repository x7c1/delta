//! A one-shot plain-HTTP server on a loopback port, for the client's tests.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Serve one request at `path` on a loopback port, answering with the raw
/// `response` bytes and then closing the connection; hands back the URL and
/// the raw request it received.
pub(crate) async fn serve_raw(path: &str, response: Vec<u8>) -> (String, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}{path}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 1024];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0, "the client hung up mid-request");
            request.extend_from_slice(&buf[..n]);
        }
        socket.write_all(&response).await.unwrap();
        socket.shutdown().await.unwrap();
        String::from_utf8(request).unwrap()
    });
    (url, server)
}

/// Serve one request for the latest release with `status` and a JSON `body`.
pub(crate) async fn serve_once(
    status: &'static str,
    body: &'static str,
) -> (String, JoinHandle<String>) {
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    serve_raw("/repos/x7c1/delta/releases/latest", response.into_bytes()).await
}
