use crate::shadow::ShadowError;

#[derive(thiserror::Error, Debug)]
pub enum HarmonyError {
    // Serialization
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    // Shadow diff
    #[error("shadow diff error: {0}")]
    Shadow(#[from] ShadowError),

    // IPC
    #[error("IPC connection failed: {0}")]
    IpcConnection(String),
    #[error("IPC timeout after {timeout_ms}ms")]
    IpcTimeout { timeout_ms: u64 },
    #[error("IPC response parse error: {0}")]
    IpcParse(String),

    // Agent
    #[error("agent {agent_id} not found")]
    AgentNotFound { agent_id: uuid::Uuid },
    #[error("agent ACP endpoint unreachable: {endpoint}")]
    AgentUnreachable { endpoint: String },
    #[error("agent task rejected: {reason}")]
    AgentTaskRejected { reason: String },

    // Analysis
    #[error("Tree-sitter parse error for language {language}: {detail}")]
    TreeSitterParse { language: String, detail: String },
    #[error("LSP server not found for language {language}. Install {install_hint}.")]
    LspNotFound { language: String, install_hint: String },
    #[error("LSP request timed out")]
    LspTimeout,

    // Sandbox
    #[error("no test command found in project root")]
    NoTestCommand,
    #[error("sandbox test run timed out after {timeout_s}s")]
    SandboxTimeout { timeout_s: u64 },
    #[error("sandbox test run failed to start: {0}")]
    SandboxStartFailed(String),

    // Negotiation
    #[error("negotiation failed: LLM returned invalid JSON: {0}")]
    NegotiationInvalidResponse(String),
    #[error("negotiation failed: proposed diff does not apply cleanly")]
    NegotiationBadDiff,
    #[error("negotiation backend not configured")]
    NegotiationNotConfigured,

    // Memory
    #[error("embedding model failed to initialize: {0}")]
    EmbeddingInit(String),
    #[error("embedding computation failed: {0}")]
    EmbeddingFailed(String),

    // Config
    #[error("config file parse error: {0}")]
    ConfigParse(String),

    // Generic
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unexpected error: {0}")]
    Internal(String),
}

/// IPC error codes for SidecarResponse::Error
pub mod error_codes {
    pub const AGENT_NOT_FOUND: u32 = 1001;
    pub const AGENT_UNREACHABLE: u32 = 1002;
    pub const OVERLAP_NOT_FOUND: u32 = 1003;
    pub const DIFF_NOT_APPLICABLE: u32 = 1004;
    pub const NEGOTIATION_FAILED: u32 = 1005;
    pub const ANALYSIS_FAILED: u32 = 1006;
    pub const DATABASE_ERROR: u32 = 2001;
    pub const INTERNAL: u32 = 9999;
}
