//! The URL that opens the app: an origin with a one-time code in its fragment,
//! and `delta-server open-url`, which asks a running server for a fresh one.
//!
//! The code rides in the fragment (`<origin>/#code=<code>`) so it never reaches
//! a server's request log or a `Referer`; the page reads it, removes it from
//! the address bar and trades it for the bearer token (see
//! `crate::one_time_codes`).
//!
//! `open-url` finds the running server the way the server itself found its
//! data directory and port — from `DELTA_IDENTIFIER`, `DELTA_DATA_DIR` and
//! `DELTA_PORT` — reads the bearer token from the server's token file
//! (`crate::token_file`), and mints a code through `POST /api/auth/codes`. The
//! request is a single plain HTTP/1.1 exchange on the loopback interface, so it
//! needs no HTTP client library.

use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use delta_wire::endpoint::{Endpoint, MintAuthCode};
use delta_wire::rest::WireAuthCodeResponse;

use crate::token_file::read_token;

/// How long `open-url` waits for the running server to connect and answer.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The URL that opens the app at `origin` with `code`: `<origin>/#code=<code>`.
pub fn code_url(origin: &str, code: &str) -> String {
    format!("{}/#code={code}", origin.trim_end_matches('/'))
}

/// The server's own origin on the loopback `port`.
pub fn loopback_origin(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// Why `open-url` could not print a URL.
#[derive(Debug, thiserror::Error)]
pub enum OpenUrlError {
    #[error("{0}")]
    Usage(String),
    #[error(
        "could not read the server's token file {path}: {source}. Is delta-server running \
         with this data directory? Set DELTA_IDENTIFIER, DELTA_DATA_DIR and DELTA_PORT as \
         the server was started with"
    )]
    NoTokenFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the token file {0} does not hold a usable token")]
    UnusableToken(PathBuf),
    #[error("no delta-server answered on {addr}: {source}")]
    NotListening {
        addr: SocketAddr,
        source: std::io::Error,
    },
    #[error(
        "the server on {addr} refused the token in {path}; the file may be left from an \
         earlier run, or another server holds the port"
    )]
    TokenRefused { addr: SocketAddr, path: PathBuf },
    #[error("the server on {addr} answered {status}")]
    UnexpectedStatus { addr: SocketAddr, status: String },
    #[error("could not talk to the server on {addr}: {source}")]
    Io {
        addr: SocketAddr,
        source: std::io::Error,
    },
    #[error("the server on {addr} answered with a body that holds no code")]
    MalformedResponse { addr: SocketAddr },
}

/// What `open-url` was asked for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpenUrlArgs {
    /// The origin to print the URL for instead of the server's own — the Vite
    /// dev server's, for `make dev`.
    pub origin: Option<String>,
}

/// Parse the arguments after `open-url`.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<OpenUrlArgs, OpenUrlError> {
    let mut parsed = OpenUrlArgs::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let origin = match arg.as_str() {
            "--origin" => args
                .next()
                .ok_or_else(|| OpenUrlError::Usage("--origin needs a URL".into()))?,
            other => match other.strip_prefix("--origin=") {
                Some(origin) => origin.to_owned(),
                None => return Err(OpenUrlError::Usage(format!("unexpected argument: {other}"))),
            },
        };
        parsed.origin = Some(checked_origin(&origin)?);
    }
    Ok(parsed)
}

/// `origin` when it is an `http` or `https` origin with no path, query or
/// fragment (a trailing `/` is dropped).
fn checked_origin(origin: &str) -> Result<String, OpenUrlError> {
    let trimmed = origin.trim_end_matches('/');
    let authority = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"));
    match authority {
        Some(authority) if !authority.is_empty() && !authority.contains(['/', '?', '#']) => {
            Ok(trimmed.to_owned())
        }
        _ => Err(OpenUrlError::Usage(format!(
            "--origin must be an http or https origin such as http://localhost:5173, not {origin}"
        ))),
    }
}

/// Ask the server on the loopback `port` for a code, presenting the token kept
/// in `token_file`, and return the URL that opens the app at `args.origin`
/// (the server's own origin when unset).
pub fn open_url(
    args: &OpenUrlArgs,
    token_file: PathBuf,
    port: u16,
) -> Result<String, OpenUrlError> {
    let token = read_token(&token_file).map_err(|source| OpenUrlError::NoTokenFile {
        path: token_file.clone(),
        source,
    })?;
    if token.is_empty() || token.chars().any(|c| c.is_control()) {
        return Err(OpenUrlError::UnusableToken(token_file));
    }
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let code = request_code(addr, &token).map_err(|err| match err {
        OpenUrlError::TokenRefused { addr, .. } => OpenUrlError::TokenRefused {
            addr,
            path: token_file,
        },
        other => other,
    })?;
    let origin = args.origin.clone().unwrap_or_else(|| loopback_origin(port));
    Ok(code_url(&origin, &code))
}

/// Ask the server on `addr` for a one-time code, presenting `token`.
fn request_code(addr: SocketAddr, token: &str) -> Result<String, OpenUrlError> {
    let io = |source| OpenUrlError::Io { addr, source };
    let mut stream = TcpStream::connect_timeout(&addr, REQUEST_TIMEOUT)
        .map_err(|source| OpenUrlError::NotListening { addr, source })?;
    stream.set_read_timeout(Some(REQUEST_TIMEOUT)).map_err(io)?;
    stream
        .set_write_timeout(Some(REQUEST_TIMEOUT))
        .map_err(io)?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\n\
         Content-Length: 0\r\nConnection: close\r\n\r\n",
        path = MintAuthCode::PATH,
    );
    stream.write_all(request.as_bytes()).map_err(io)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(io)?;
    parse_response(addr, &response)
}

/// The code in a raw HTTP response, or why there is none.
fn parse_response(addr: SocketAddr, response: &[u8]) -> Result<String, OpenUrlError> {
    let malformed = || OpenUrlError::MalformedResponse { addr };
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(malformed)?;
    let head = std::str::from_utf8(&response[..split]).map_err(|_| malformed())?;
    let body = &response[split + 4..];
    let status_line = head.lines().next().ok_or_else(malformed)?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(malformed)?;
    match status {
        "200" => {}
        "401" => {
            return Err(OpenUrlError::TokenRefused {
                addr,
                path: PathBuf::new(),
            })
        }
        _ => {
            return Err(OpenUrlError::UnexpectedStatus {
                addr,
                status: status_line.to_owned(),
            })
        }
    }
    let parsed: WireAuthCodeResponse = serde_json::from_slice(body).map_err(|_| malformed())?;
    Ok(parsed.code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr() -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, 7878))
    }

    #[test]
    fn the_code_rides_in_the_fragment() {
        assert_eq!(
            code_url("http://127.0.0.1:7878", "abc"),
            "http://127.0.0.1:7878/#code=abc"
        );
        assert_eq!(
            code_url("http://localhost:5173/", "abc"),
            "http://localhost:5173/#code=abc"
        );
    }

    #[test]
    fn parses_the_origin_flag() {
        let args = |list: &[&str]| parse_args(list.iter().map(|s| s.to_string()));
        assert_eq!(args(&[]).unwrap(), OpenUrlArgs::default());
        assert_eq!(
            args(&["--origin", "http://localhost:5173/"])
                .unwrap()
                .origin
                .as_deref(),
            Some("http://localhost:5173")
        );
        assert_eq!(
            args(&["--origin=https://127.0.0.1:8443"])
                .unwrap()
                .origin
                .as_deref(),
            Some("https://127.0.0.1:8443")
        );
        for bad in [
            &["--origin"][..],
            &["--origin", "localhost:5173"],
            &["--origin", "http://localhost:5173/app"],
            &["--origin", "file:///tmp"],
            &["--verbose"],
        ] {
            assert!(matches!(args(bad), Err(OpenUrlError::Usage(_))), "{bad:?}");
        }
    }

    #[test]
    fn reads_the_code_from_a_200() {
        let response = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                         content-length: 12\r\n\r\n{\"code\":\"c\"}";
        assert_eq!(parse_response(addr(), response).unwrap(), "c");
    }

    #[test]
    fn a_401_is_a_refused_token() {
        let response = b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\n\r\n";
        assert!(matches!(
            parse_response(addr(), response),
            Err(OpenUrlError::TokenRefused { .. })
        ));
    }

    #[test]
    fn other_answers_are_errors() {
        let response = b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n";
        assert!(matches!(
            parse_response(addr(), response),
            Err(OpenUrlError::UnexpectedStatus { .. })
        ));
        assert!(matches!(
            parse_response(addr(), b"garbage"),
            Err(OpenUrlError::MalformedResponse { .. })
        ));
        assert!(matches!(
            parse_response(addr(), b"HTTP/1.1 200 OK\r\n\r\n{}"),
            Err(OpenUrlError::MalformedResponse { .. })
        ));
    }

    #[test]
    fn a_missing_token_file_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth-token");
        let err = open_url(&OpenUrlArgs::default(), path.clone(), 1).unwrap_err();
        assert!(matches!(err, OpenUrlError::NoTokenFile { .. }));
        assert!(err.to_string().contains(&path.display().to_string()));
    }
}
