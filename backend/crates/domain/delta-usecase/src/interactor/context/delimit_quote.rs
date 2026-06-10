/// Delimit a passage so a provenance frame and its quoted text stay
/// distinguishable. Centralised so every frame quotes passages the same way.
pub(in crate::interactor::context) fn delimit_quote(quote: &str) -> String {
    format!("\"{}\"", quote.trim())
}
