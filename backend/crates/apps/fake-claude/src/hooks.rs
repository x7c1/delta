//! Firing Claude Code hooks at the server.
//!
//! The payload bodies are the `delta_wire::hooks` types — the exact structs
//! the server deserializes — so the fake cannot drift from the contract. The
//! transport is a deliberately tiny HTTP/1.1 client over `TcpStream`: every
//! hook goes to `127.0.0.1`, the bodies are small, and a one-shot
//! `Connection: close` POST is all the contract needs, so a full HTTP client
//! dependency would buy nothing.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde::Serialize;

/// How long a hook POST may take end to end. Hooks block the (fake) session
/// like they block the real `claude`, so a wedged server must not hang the
/// pane forever.
const HOOK_TIMEOUT: Duration = Duration::from_secs(30);

/// POST `payload` as JSON to `url`, returning the response status code and
/// body.
///
/// The body matters for the hooks whose HTTP response the real `claude`
/// consumes — `UserPromptSubmit`'s `additionalContext` and
/// `PermissionRequest`'s decision — so the fake returns it for the engine to
/// interpret. Errors are returned (not panicked) so the scenario engine can
/// decide what a failed hook means; a real `claude` likewise carries on when a
/// hook endpoint is unreachable.
pub fn post_json<P: Serialize>(url: &str, payload: &P) -> Result<(u16, String), String> {
    let body = serde_json::to_string(payload).map_err(|e| format!("serialize hook body: {e}"))?;
    let (host, path) = split_url(url)?;

    let mut stream = TcpStream::connect(&host).map_err(|e| format!("connect {host}: {e}"))?;
    stream
        .set_read_timeout(Some(HOOK_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(HOOK_TIMEOUT)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let request = format!(
        "POST {path} HTTP/1.1\r\n\
         Host: {host}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("send to {url}: {e}"))?;

    // Read the whole response (Connection: close ends it), pull the status out
    // of the first line and the body from after the blank line.
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| format!("read response from {url}: {e}"))?;
    let status = parse_status(&response).ok_or_else(|| format!("malformed response from {url}"))?;
    Ok((status, parse_body(&response)))
}

/// The body of an HTTP/1.x response: everything after the head's blank line.
/// Empty when there is no body.
fn parse_body(response: &str) -> String {
    match response.split_once("\r\n\r\n") {
        Some((_head, body)) => body.to_owned(),
        None => String::new(),
    }
}

/// Split `http://host:port/path` into (`host:port`, `/path`).
fn split_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("hook URL is not http: {url}"))?;
    match rest.split_once('/') {
        Some((host, path)) => Ok((host.to_owned(), format!("/{path}"))),
        None => Ok((rest.to_owned(), "/".to_owned())),
    }
}

/// The status code of an HTTP/1.x response head.
fn parse_status(response: &str) -> Option<u16> {
    response.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_hook_url_into_host_and_path() {
        assert_eq!(
            split_url("http://127.0.0.1:7878/hooks/stop").unwrap(),
            ("127.0.0.1:7878".to_owned(), "/hooks/stop".to_owned())
        );
    }

    #[test]
    fn rejects_a_non_http_url() {
        assert!(split_url("https://example.com/x").is_err());
    }

    #[test]
    fn parses_the_status_line() {
        assert_eq!(parse_status("HTTP/1.1 200 OK\r\n\r\n"), Some(200));
        assert_eq!(parse_status("HTTP/1.1 404 Not Found\r\n"), Some(404));
        assert_eq!(parse_status(""), None);
    }

    #[test]
    fn parses_the_body_after_the_blank_line() {
        assert_eq!(
            parse_body("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}"),
            "{\"a\":1}"
        );
        assert_eq!(parse_body("HTTP/1.1 200 OK\r\n\r\n"), "");
        assert_eq!(parse_body(""), "");
    }
}
