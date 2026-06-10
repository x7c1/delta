/// Maximum length of a provisional branch title, in characters.
const PROVISIONAL_TITLE_MAX_CHARS: usize = 40;

/// Derive a provisional branch-thread title from a locator quote.
///
/// The quote is trimmed and truncated to [`PROVISIONAL_TITLE_MAX_CHARS`]
/// characters; an absent or blank quote falls back to `"untitled"`.
pub(in crate::interactor::enqueue) fn provisional_branch_title(
    locator_quote: Option<&str>,
) -> String {
    let trimmed = locator_quote.map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        return "untitled".to_owned();
    }
    trimmed.chars().take(PROVISIONAL_TITLE_MAX_CHARS).collect()
}
