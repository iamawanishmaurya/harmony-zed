// MCP-specific request/response types
// Used for type-safe parsing of JSON-RPC messages

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// MCP-specific constants
pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

pub const MACHINE_NAME_HEADER: &str = "x-harmony-machine-name";
pub const MACHINE_IP_HEADER: &str = "x-harmony-machine-ip";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestContext {
    pub machine_name: String,
    pub machine_ip: String,
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::local()
    }
}

impl RequestContext {
    pub fn new(machine_name: impl Into<String>, machine_ip: impl Into<String>) -> Self {
        Self {
            machine_name: machine_name.into(),
            machine_ip: machine_ip.into(),
        }
    }

    pub fn local() -> Self {
        let machine_name = std::env::var("HARMONY_MACHINE_NAME")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "local".to_string());
        let machine_ip = std::env::var("HARMONY_MACHINE_IP")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "127.0.0.1".to_string());

        Self {
            machine_name,
            machine_ip,
        }
    }
}
