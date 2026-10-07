/// Why the HTTPS client could not be built.
#[derive(Debug, thiserror::Error)]
pub enum ClientBuildError {
    /// The platform trust store could not be set up.
    #[error("could not set up TLS against the platform trust store: {0}")]
    Tls(#[from] rustls::Error),
    /// reqwest refused the configuration.
    #[error("could not build the HTTPS client: {0}")]
    Client(#[from] reqwest::Error),
}
