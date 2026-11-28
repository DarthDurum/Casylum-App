//! Provider-neutral conversation model shared by the agent loop, the
//! provider adapters, session storage and the UI.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
    },
    /// Model reasoning. `signature` must be replayed unchanged to Anthropic.
    Thinking {
        thinking: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    RedactedThinking {
        data: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
        /// Opaque provider data that must be echoed back (e.g. Gemini thought signatures).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_meta: Option<Value>,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
        /// UI-only extras (diffs etc.). Never sent to a provider.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        meta: Option<Value>,
    },
    /// An image the user attached. `data` is base64 with no `data:` prefix.
    Image {
        name: String,
        media_type: String,
        data: String,
    },
    /// A text file the user attached. Sent to the model inline as text.
    File {
        name: String,
        text: String,
    },
}

impl Block {
    /// Whether this block is something the user attached to a message.
    pub fn is_attachment(&self) -> bool {
        matches!(self, Block::Image { .. } | Block::File { .. })
    }
}

/// How an attached text file is shown to a model.
pub fn file_as_text(name: &str, text: &str) -> String {
    format!("<attached_file name=\"{name}\">\n{text}\n</attached_file>")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<Block>,
}

impl Message {
    #[cfg(test)]
    pub fn user_text(text: impl Into<String>) -> Self {
        Self { role: Role::User, content: vec![Block::Text { text: text.into() }] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Refusal,
    Other,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct Turn {
    pub content: Vec<Block>,
    pub stop: StopReason,
    pub usage: Usage,
}

/// Live deltas emitted while a provider streams.
pub enum StreamDelta {
    Text(String),
    Thinking(String),
    ToolStarted { name: String },
}
