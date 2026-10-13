//! The decision body: `behavior` is `"allow"` or `"deny"`.

use serde::{Deserialize, Serialize};

/// The decision body: `behavior` is `"allow"` or `"deny"`.
#[derive(Debug, Serialize, Deserialize)]
pub struct PermissionDecisionBody {
    pub behavior: String,
}
