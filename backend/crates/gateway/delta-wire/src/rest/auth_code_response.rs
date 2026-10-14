//! Response for `POST /api/auth/codes`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Response for `POST /api/auth/codes`: a fresh one-time code, for a URL that
/// opens the app (`<origin>/#code=<code>`). Read back by `delta-server
/// open-url`, hence `Deserialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(rename = "AuthCodeResponse")]
pub struct WireAuthCodeResponse {
    pub code: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_code() {
        let response = WireAuthCodeResponse {
            code: "c".to_owned(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, r#"{"code":"c"}"#);
        assert_eq!(
            serde_json::from_str::<WireAuthCodeResponse>(&json).unwrap(),
            response
        );
    }
}
