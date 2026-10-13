/// Why the HTTPS client could not be built.
///
/// `Display` describes only this level; the underlying failure is the
/// error's `source`, so a log renders the whole chain
/// ([`delta_usecase::with_causes`]).
#[derive(Debug, thiserror::Error)]
pub enum ClientBuildError {
    /// The platform trust store could not be set up.
    #[error("could not set up TLS against the platform trust store")]
    Tls(#[from] rustls::Error),
    /// reqwest refused the configuration.
    #[error("could not build the HTTPS client")]
    Client(#[from] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use delta_usecase::with_causes;

    use super::*;

    #[test]
    fn the_rendered_chain_names_the_cause_once() {
        let err = ClientBuildError::from(rustls::Error::General("no usable root".into()));
        assert_eq!(
            with_causes(&err),
            "could not set up TLS against the platform trust store: unexpected error: no usable root"
        );
    }
}
