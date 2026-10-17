//! The served `index.html`, and the policy it is sent with.

use axum::body::Bytes;
use axum::http::HeaderValue;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use include_dir::Dir;
use sha2::{Digest, Sha256};

/// The page's Content-Security-Policy, sent as a response header with it.
///
/// It mirrors the `<meta http-equiv>` policy in `index.html`, minus
/// `'unsafe-eval'` and `'unsafe-inline'` in `script-src`, which only Vite's
/// dev client needs. Both policies are enforced, so the header tightens the
/// page, and unlike the `<meta>` tag it also enforces `frame-ancestors`.
///
/// `script-src` allows each inline `<script>` of `page` by its `'sha256-…'`
/// source instead of `'unsafe-inline'`, so the theme bootstrap runs but an
/// injected inline script or handler does not. The hashes are taken from the
/// page as built, so editing the bootstrap cannot leave a stale hash behind.
/// `style-src` keeps `'unsafe-inline'`: xterm injects inline styles.
pub(super) fn content_security_policy(page: &[u8]) -> HeaderValue {
    let mut script_src = String::from("'self'");
    for script in inline_scripts(page) {
        script_src.push_str(" 'sha256-");
        script_src.push_str(&BASE64.encode(Sha256::digest(script)));
        script_src.push('\'');
    }
    let policy = format!(
        "default-src 'self'; base-uri 'self'; img-src 'self' data:; \
         font-src 'self' data:; connect-src 'self'; script-src {script_src}; \
         style-src 'self' 'unsafe-inline'; frame-ancestors 'none'; object-src 'none'"
    );
    HeaderValue::from_str(&policy).expect("the policy is ASCII without control characters")
}

/// `index.html` from `site`, exactly as the build wrote it.
pub(super) fn page(site: &'static Dir<'static>) -> Bytes {
    let page = site
        .get_file("index.html")
        .expect("the embedded web build has no index.html");
    Bytes::from_static(page.contents())
}

/// The text of every `<script>` element in `page` without a `src` attribute,
/// as the browser hashes it: newlines normalized to `\n`, as HTML parsing does.
///
/// A scan, not a full HTML parser: it skips comments, and takes a script's
/// text up to the first `</script`, which is where the browser ends it too.
/// It runs on the build's own `index.html`, so it only has to cover what Vite
/// writes there.
fn inline_scripts(page: &[u8]) -> Vec<Vec<u8>> {
    let mut scripts = Vec::new();
    let mut rest = page;
    while let Some(start) = find(rest, b"<") {
        rest = &rest[start..];
        if rest.starts_with(b"<!--") {
            rest = match find(rest, b"-->") {
                Some(end) => &rest[end + 3..],
                None => break,
            };
            continue;
        }
        if !starts_with_tag(rest, b"<script") {
            rest = &rest[1..];
            continue;
        }
        let Some(tag_end) = find(rest, b">") else {
            break;
        };
        let attributes = &rest[b"<script".len()..tag_end];
        rest = &rest[tag_end + 1..];
        let text_end = find_ignore_case(rest, b"</script").unwrap_or(rest.len());
        if !has_attribute(attributes, b"src") {
            scripts.push(normalize_newlines(&rest[..text_end]));
        }
        rest = &rest[text_end..];
    }
    scripts
}

/// Whether `input` starts with the tag name `tag` (ASCII case-insensitive),
/// followed by whitespace, `>` or `/`, so `<scripts>` is not `<script`.
fn starts_with_tag(input: &[u8], tag: &[u8]) -> bool {
    input.len() > tag.len()
        && input[..tag.len()].eq_ignore_ascii_case(tag)
        && matches!(
            input[tag.len()],
            b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r' | b'\x0c'
        )
}

/// Whether the attribute list of a start tag names `name`, with or without a
/// value. Quoted values are skipped, so `data-x="src"` is not `src`.
fn has_attribute(attributes: &[u8], name: &[u8]) -> bool {
    let mut rest = attributes;
    loop {
        rest = trim_start(rest);
        if rest.is_empty() {
            return false;
        }
        let name_end = rest
            .iter()
            .position(|&b| b.is_ascii_whitespace() || b == b'=' || b == b'/')
            .unwrap_or(rest.len())
            .max(1);
        if rest[..name_end].eq_ignore_ascii_case(name) {
            return true;
        }
        rest = trim_start(&rest[name_end..]);
        if let Some(value) = rest.strip_prefix(b"=") {
            rest = trim_start(value);
            rest = match rest.first() {
                Some(&quote @ (b'"' | b'\'')) => match rest[1..].iter().position(|&b| b == quote) {
                    Some(close) => &rest[close + 2..],
                    None => return false,
                },
                _ => {
                    let end = rest
                        .iter()
                        .position(u8::is_ascii_whitespace)
                        .unwrap_or(rest.len());
                    &rest[end..]
                }
            };
        }
    }
}

fn trim_start(input: &[u8]) -> &[u8] {
    let start = input
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(input.len());
    &input[start..]
}

fn normalize_newlines(text: &[u8]) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(text.len());
    let mut bytes = text.iter().copied().peekable();
    while let Some(byte) = bytes.next() {
        if byte == b'\r' {
            normalized.push(b'\n');
            if bytes.peek() == Some(&b'\n') {
                bytes.next();
            }
        } else {
            normalized.push(byte);
        }
    }
    normalized
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn find_ignore_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The web app's `index.html`. Vite copies its inline scripts into the
    /// build verbatim, so their hashes are the built page's.
    const WEB_INDEX: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../frontend/packages/apps/web/index.html"
    ));

    /// `policy`'s directives, as `(name, sources)` pairs in order.
    fn directives(policy: &HeaderValue) -> Vec<(String, Vec<String>)> {
        policy
            .to_str()
            .unwrap()
            .split(';')
            .map(|directive| {
                let mut words = directive.split_whitespace().map(str::to_owned);
                (words.next().unwrap(), words.collect())
            })
            .collect()
    }

    fn script_src(policy: &HeaderValue) -> Vec<String> {
        directives(policy)
            .into_iter()
            .find(|(name, _)| name == "script-src")
            .expect("the policy has a script-src")
            .1
    }

    /// The `'sha256-…'` source of each inline script in `page`, found
    /// independently of [`inline_scripts`]: every `<script>` tag (the pages
    /// checked here write them in lower case, outside comments) whose
    /// attributes have no `src=`.
    fn expected_hashes(page: &str) -> Vec<String> {
        page.split("<script")
            .skip(1)
            .filter_map(|tail| {
                let (attributes, rest) = tail.split_once('>').unwrap();
                let text = rest.split_once("</script>").unwrap().0;
                (!attributes.contains("src=")).then(|| {
                    format!(
                        "'sha256-{}'",
                        BASE64.encode(Sha256::digest(text.as_bytes()))
                    )
                })
            })
            .collect()
    }

    fn assert_allows_every_inline_script(page: &str) {
        let expected = expected_hashes(page);
        assert!(
            !expected.is_empty(),
            "the page has the inline theme bootstrap"
        );
        let mut allowed = vec!["'self'".to_owned()];
        allowed.extend(expected);
        assert_eq!(
            script_src(&content_security_policy(page.as_bytes())),
            allowed,
            "script-src allows the bundle and exactly the page's inline scripts, by hash",
        );
    }

    #[test]
    fn allows_every_inline_script_of_the_web_page_by_its_hash() {
        assert_allows_every_inline_script(WEB_INDEX);
    }

    /// Only exists with the feature on, which needs the built `dist/`:
    /// `make check` runs it in `check-embedded`, and CI in the step that
    /// builds and tests the embedded server. By hand, run it after
    /// `make web-dist` with `cargo test -p delta-server --features embed-web`.
    #[cfg(feature = "embed-web")]
    #[test]
    fn allows_every_inline_script_of_the_embedded_build_by_its_hash() {
        let page = super::super::BUILT
            .get_file("index.html")
            .and_then(|file| file.contents_utf8())
            .unwrap();
        assert_allows_every_inline_script(page);
    }

    #[test]
    fn keeps_every_other_directive_and_inline_styles() {
        let policy = content_security_policy(WEB_INDEX.as_bytes());
        let sources = |list: &[&str]| list.iter().map(|&s| s.to_owned()).collect::<Vec<_>>();
        let expected = [
            ("default-src", sources(&["'self'"])),
            ("base-uri", sources(&["'self'"])),
            ("img-src", sources(&["'self'", "data:"])),
            ("font-src", sources(&["'self'", "data:"])),
            ("connect-src", sources(&["'self'"])),
            ("script-src", script_src(&policy)),
            ("style-src", sources(&["'self'", "'unsafe-inline'"])),
            ("frame-ancestors", sources(&["'none'"])),
            ("object-src", sources(&["'none'"])),
        ]
        .map(|(name, list)| (name.to_owned(), list));
        assert_eq!(directives(&policy), expected);
    }

    #[test]
    fn a_page_without_inline_scripts_allows_only_its_own_origin() {
        let page = b"<html><script type=\"module\" src=\"/assets/a.js\"></script></html>";
        assert_eq!(script_src(&content_security_policy(page)), ["'self'"]);
    }

    #[test]
    fn finds_only_the_inline_scripts_as_the_browser_reads_them() {
        let page = b"<!-- <script>commented()</script> -->\
            <SCRIPT TYPE=module>a()</SCRIPT>\
            <script src=/b.js></script>\
            <script data-x=\"src\" crossorigin>c()\r\nd()\r</script>\
            <scripts>not a script</scripts>\
            <script\nSrc = 'e.js'></script>";
        assert_eq!(
            inline_scripts(page),
            [b"a()".to_vec(), b"c()\nd()\n".to_vec()],
        );
    }
}
