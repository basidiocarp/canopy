use serde::{Deserialize, Serialize};

/// Token usage for a single task execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    /// Number of tool calls made during this task.
    pub tool_calls: u32,
}

/// Structured output from a completed task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutput {
    /// Raw agent output as a string (always present).
    pub raw: String,
    /// Parsed JSON if the agent returned structured data.
    pub json: Option<serde_json::Value>,
    /// Token usage for this task execution.
    pub usage: TokenUsage,
}

/// The result of completing a canopy task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TaskResult {
    /// Task completed with output.
    Success(TaskOutput),
    /// Task failed with an error message.
    Failed { reason: String },
    /// Task was intentionally skipped.
    Skipped { reason: String },
}
