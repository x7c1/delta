use std::io;
use std::path::Path;

use tracing_subscriber::EnvFilter;

use super::{FilterFallback, FilterSource, ResolvedFilter, DEFAULT_DIRECTIVES};

/// Choose the filter from `rust_log` (the value of `RUST_LOG`) and the filter
/// file at `file`, whose read gave `contents`.
///
/// `RUST_LOG`, when set and not empty, wins. Otherwise the file's directives
/// are used: one directive list per line, the lines joined with `,`, and a line
/// starting with `#` ignored. A missing or empty file means
/// [`DEFAULT_DIRECTIVES`]. Directives that do not parse — from either source —
/// or a file that cannot be read also fall back to [`DEFAULT_DIRECTIVES`], with
/// a warning naming where they came from.
pub fn resolve_filter(
    rust_log: Option<&str>,
    file: &Path,
    contents: io::Result<String>,
) -> ResolvedFilter {
    if let Some(directives) = rust_log.filter(|value| !value.trim().is_empty()) {
        return match EnvFilter::try_new(directives) {
            Ok(_) => ResolvedFilter {
                directives: directives.to_owned(),
                source: FilterSource::Env,
                fallback: None,
            },
            Err(cause) => fallback(FilterFallback::InvalidRustLog {
                directives: directives.to_owned(),
                cause,
            }),
        };
    }
    let contents = match contents {
        Ok(contents) => contents,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return default(),
        Err(cause) => {
            return fallback(FilterFallback::UnreadableFile {
                file: file.to_owned(),
                cause,
            })
        }
    };
    let directives = file_directives(&contents);
    if directives.is_empty() {
        return default();
    }
    match EnvFilter::try_new(&directives) {
        Ok(_) => ResolvedFilter {
            directives,
            source: FilterSource::File(file.to_owned()),
            fallback: None,
        },
        Err(cause) => fallback(FilterFallback::InvalidFile {
            file: file.to_owned(),
            directives,
            cause,
        }),
    }
}

/// The file's directive lists, comments and blank lines dropped, joined with
/// `,`.
fn file_directives(contents: &str) -> String {
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join(",")
}

fn default() -> ResolvedFilter {
    ResolvedFilter {
        directives: DEFAULT_DIRECTIVES.to_owned(),
        source: FilterSource::Default,
        fallback: None,
    }
}

fn fallback(reason: FilterFallback) -> ResolvedFilter {
    ResolvedFilter {
        fallback: Some(reason),
        ..default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "/data/io.github.x7c1.delta/log-filter";

    fn file() -> &'static Path {
        Path::new(FILE)
    }

    fn missing() -> io::Result<String> {
        Err(io::Error::from(io::ErrorKind::NotFound))
    }

    fn holding(contents: &str) -> io::Result<String> {
        Ok(contents.to_owned())
    }

    #[test]
    fn rust_log_wins_over_the_file() {
        let resolved = resolve_filter(
            Some("warn"),
            file(),
            holding("info,delta_server::http=debug"),
        );
        assert_eq!(resolved.directives, "warn");
        assert_eq!(resolved.source, FilterSource::Env);
        assert!(resolved.fallback.is_none());
    }

    #[test]
    fn an_empty_rust_log_is_treated_as_unset() {
        let resolved = resolve_filter(Some(""), file(), holding("debug"));
        assert_eq!(resolved.directives, "debug");
    }

    #[test]
    fn a_missing_file_gives_info() {
        let resolved = resolve_filter(None, file(), missing());
        assert_eq!(resolved.directives, "info");
        assert_eq!(resolved.source, FilterSource::Default);
        assert!(resolved.fallback.is_none());
    }

    #[test]
    fn a_valid_file_s_directives_are_used() {
        let resolved = resolve_filter(None, file(), holding("info,delta_server::http=debug\n"));
        assert_eq!(resolved.directives, "info,delta_server::http=debug");
        assert_eq!(resolved.source, FilterSource::File(file().to_owned()));
        assert!(resolved.fallback.is_none());
    }

    #[test]
    fn the_file_s_lines_are_joined_and_its_comments_dropped() {
        let resolved = resolve_filter(
            None,
            file(),
            holding("# request log on\ninfo\n\n  delta_server::http=debug  \n"),
        );
        assert_eq!(resolved.directives, "info,delta_server::http=debug");
    }

    #[test]
    fn an_empty_file_gives_info_without_a_warning() {
        let resolved = resolve_filter(None, file(), holding("# nothing yet\n\n"));
        assert_eq!(resolved.directives, "info");
        assert!(resolved.fallback.is_none());
    }

    #[test]
    fn a_malformed_file_gives_info_and_a_warning_naming_the_file() {
        let resolved = resolve_filter(None, file(), holding("delta_server=loud"));
        assert_eq!(resolved.directives, "info");
        assert_eq!(resolved.source, FilterSource::Default);
        let fallback = resolved.fallback.expect("a fallback");
        assert!(
            matches!(&fallback, FilterFallback::InvalidFile { file: named, .. } if named == file()),
            "{fallback:?}"
        );
        assert!(fallback.to_string().contains(FILE), "{fallback}");
    }

    #[test]
    fn an_unreadable_file_gives_info_and_a_warning_naming_the_file() {
        let resolved = resolve_filter(
            None,
            file(),
            Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        );
        assert_eq!(resolved.directives, "info");
        let fallback = resolved.fallback.expect("a fallback");
        assert!(
            matches!(&fallback, FilterFallback::UnreadableFile { file: named, .. } if named == file()),
            "{fallback:?}"
        );
        assert!(fallback.to_string().contains(FILE), "{fallback}");
    }

    #[test]
    fn a_malformed_rust_log_gives_info_and_a_warning_naming_it() {
        let resolved = resolve_filter(Some("delta_server=loud"), file(), missing());
        assert_eq!(resolved.directives, "info");
        let fallback = resolved.fallback.expect("a fallback");
        assert!(
            matches!(&fallback, FilterFallback::InvalidRustLog { directives, .. } if directives == "delta_server=loud"),
            "{fallback:?}"
        );
        assert!(fallback.to_string().contains("RUST_LOG"), "{fallback}");
    }
}
