//! Rendering an error with its whole cause chain, for a log line or a
//! message shown to the user.

/// `err` followed by each of its causes, joined with `: `.
///
/// An error's own `Display` describes only its level (reqwest's says just
/// "error sending request"); what a user needs — DNS, a refused connection, a
/// timeout, a certificate the trust store rejects — is further down the
/// chain. An error whose `Display` also embeds its `source` would print that
/// cause twice here, so errors in this workspace keep their `Display` to their
/// own level and leave the cause to `source`.
pub fn with_causes(err: &dyn std::error::Error) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    #[error("could not read the file")]
    struct Outer(#[source] std::io::Error);

    #[test]
    fn each_level_is_rendered_once_in_order() {
        let err = Outer(std::io::Error::other("permission denied"));
        assert_eq!(
            with_causes(&err),
            "could not read the file: permission denied"
        );
    }

    #[test]
    fn an_error_without_a_source_is_its_own_message() {
        let err = std::io::Error::other("no space left");
        assert_eq!(with_causes(&err), "no space left");
    }
}
