use super::{FilterFallback, FilterSource};

/// The directives to log with, and why the ones asked for were not used when
/// they were not.
#[derive(Debug)]
pub struct ResolvedFilter {
    /// Valid `EnvFilter` directives.
    pub directives: String,
    pub source: FilterSource,
    /// To be logged at `warn` once logging is up.
    pub fallback: Option<FilterFallback>,
}
