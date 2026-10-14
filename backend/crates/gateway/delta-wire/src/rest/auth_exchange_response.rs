//! Response for `POST /api/auth/exchange`.

use serde::Serialize;
use ts_rs::TS;

/// Response for `POST /api/auth/exchange`: the per-run bearer token, handed
/// out once per one-time code. The page keeps it and presents it on every
/// later request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "AuthExchangeResponse")]
pub struct WireAuthExchangeResponse {
    pub token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_the_token() {
        assert_eq!(
            serde_json::to_value(WireAuthExchangeResponse {
                token: "t".to_owned(),
            })
            .unwrap(),
            serde_json::json!({ "token": "t" }),
        );
    }
}
