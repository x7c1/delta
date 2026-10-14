use std::ffi::OsStr;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread::JoinHandle;

use super::*;
use crate::child_env::{in_child, run_in_child};

/// One CA certificate in PEM (ISRG Root X1), to trust in the tests: none
/// of them reaches a TLS server.
const ROOT_PEM: &str = include_str!("testdata/isrg-root-x1.pem");

/// A file holding [`ROOT_PEM`], in a directory dropped with it.
struct CaFile {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

fn ca_file() -> CaFile {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ca.pem");
    std::fs::write(&path, ROOT_PEM).unwrap();
    CaFile { _dir: dir, path }
}

/// Answer one request on a loopback port with a redirect to `location`;
/// hands back the URL to ask.
fn serve_redirect(location: &str) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/sig", listener.local_addr().unwrap());
    let response = format!(
        "HTTP/1.1 302 Found\r\nlocation: {location}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    );
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 1024];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut buf).unwrap();
            assert!(n > 0, "the client hung up mid-request");
            request.extend_from_slice(&buf[..n]);
        }
        socket.write_all(response.as_bytes()).unwrap();
    });
    (url, server)
}

/// Whether anything connected to `listener` so far.
fn connected(listener: &TcpListener) -> bool {
    listener.set_nonblocking(true).unwrap();
    match listener.accept() {
        Ok(_) => true,
        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(err) => panic!("{err}"),
    }
}

fn source_of(refusal: &Refusal) -> &(dyn std::error::Error + 'static) {
    match refusal {
        Refusal::SignatureUnavailable { source, .. }
        | Refusal::ReleaseUnavailable { source, .. } => source.as_ref(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_redirect_to_plain_http_is_refused() {
    let ca = ca_file();
    let (url, server) = serve_redirect("http://127.0.0.1:9/elsewhere");
    let github = Github::with("v0.6.0", &ca.path, Schemes::PlainFirstHop).unwrap();

    let refusal = github.signature("v0.6.0", &url).unwrap_err();
    let source = source_of(&refusal);
    assert!(
        source
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_redirect),
        "{refusal:?}"
    );
    server.join().unwrap();
}

#[test]
fn a_plain_http_url_is_refused_before_connecting() {
    let ca = ca_file();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/sig", listener.local_addr().unwrap());
    let github = Github::with("v0.6.0", &ca.path, Schemes::HttpsOnly).unwrap();

    let refusal = github.signature("v0.6.0", &url).unwrap_err();
    assert!(
        matches!(refusal, Refusal::SignatureUnavailable { .. }),
        "{refusal:?}"
    );
    assert!(!connected(&listener));
}

#[test]
fn the_certificates_are_read_from_the_given_file_only() {
    let ca = ca_file();
    assert_eq!(trust_anchors(&ca.path).unwrap().len(), 1);

    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty.pem");
    std::fs::write(&empty, "").unwrap();
    let missing = dir.path().join("missing.pem");
    for file in [&empty, &missing] {
        let refusal = Github::with("v0.6.0", file, Schemes::HttpsOnly)
            .err()
            .unwrap();
        assert!(
            matches!(
                refusal,
                Refusal::ReleaseUnavailable {
                    what: "could not read the system's CA certificates",
                    ..
                }
            ),
            "{file:?}: {refusal:?}"
        );
    }
}

/// `SSL_CERT_FILE` and `SSL_CERT_DIR` add nothing to what the helper
/// trusts: with them naming a valid bundle, a CA file without certificates
/// still leaves it nothing to trust.
#[test]
fn the_certificate_variables_are_ignored() {
    let ca = ca_file();
    let dir = ca.path.parent().unwrap();
    run_in_child(
        "github::tests::child_the_certificate_variables_are_ignored",
        &[
            ("SSL_CERT_FILE", ca.path.as_os_str()),
            ("SSL_CERT_DIR", dir.as_os_str()),
        ],
    );
}

#[test]
#[ignore = "run by the_certificate_variables_are_ignored in a child process"]
fn child_the_certificate_variables_are_ignored() {
    if !in_child() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty.pem");
    std::fs::write(&empty, "").unwrap();
    assert!(trust_anchors(&empty).is_err());
    assert!(Github::with("v0.6.0", &empty, Schemes::HttpsOnly).is_err());
}

/// The variable naming the CA file the child of
/// [`the_proxy_variables_are_ignored`] trusts.
const CA_VAR: &str = "DELTA_UPDATE_HELPER_TEST_CA_FILE";

/// The proxy variables send nothing through a proxy: a request for a host
/// that does not resolve fails without connecting to the proxy they name.
#[test]
fn the_proxy_variables_are_ignored() {
    let ca = ca_file();
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    let proxy_url = OsStr::new(&proxy_url);
    run_in_child(
        "github::tests::child_the_proxy_variables_are_ignored",
        &[
            ("HTTPS_PROXY", proxy_url),
            ("https_proxy", proxy_url),
            ("HTTP_PROXY", proxy_url),
            ("http_proxy", proxy_url),
            ("ALL_PROXY", proxy_url),
            ("all_proxy", proxy_url),
            ("NO_PROXY", OsStr::new("")),
            (CA_VAR, ca.path.as_os_str()),
        ],
    );
    assert!(!connected(&proxy), "the helper went through the proxy");
}

#[test]
#[ignore = "run by the_proxy_variables_are_ignored in a child process"]
fn child_the_proxy_variables_are_ignored() {
    if !in_child() {
        return;
    }
    let ca = PathBuf::from(std::env::var_os(CA_VAR).expect("the parent sets it"));
    let github = Github::with("v0.6.0", &ca, Schemes::HttpsOnly).unwrap();
    // `.invalid` never resolves (RFC 2606): only a proxy could take it.
    let refusal = github
        .signature("v0.6.0", "https://delta-update-helper.invalid/sig")
        .unwrap_err();
    assert!(
        matches!(refusal, Refusal::SignatureUnavailable { .. }),
        "{refusal:?}"
    );
}
