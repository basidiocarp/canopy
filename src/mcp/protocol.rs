use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ===========================================================================
// JSON-RPC 2.0 message types
// ===========================================================================

fn validate_jsonrpc_version<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s != "2.0" {
        return Err(de::Error::custom(format!(
            "unsupported JSON-RPC version: {s}, expected 2.0"
        )));
    }
    Ok(s)
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcMessage {
    #[serde(deserialize_with = "validate_jsonrpc_version")]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: Option<String>,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

impl JsonRpcResponse {
    #[must_use]
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    #[must_use]
    pub fn err(id: Value, code: i64, message: String) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }

    #[must_use]
    pub fn method_not_found(id: Value, method: &str) -> Self {
        Self::err(id, -32601, format!("method not found: {method}"))
    }
}
