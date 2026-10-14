//! Request body for `POST /api/auth/exchange`.

use serde::Deserialize;
use ts_rs::TS;

/// Request body for `POST /api/auth/exchange`: the one-time code the page was
/// opened with (the `#code=` fragment of the URL the server printed, or the one
/// the desktop shell navigated to).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "AuthExchangeRequest")]
pub struct WireAuthExchangeRequest {
    pub code: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_the_code() {
        let request: WireAuthExchangeRequest = serde_json::from_str(r#"{"code":"abc"}"#).unwrap();
        assert_eq!(request.code, "abc");
    }

    #[test]
    fn a_missing_code_is_refused() {
        assert!(serde_json::from_str::<WireAuthExchangeRequest>("{}").is_err());
    }
}
