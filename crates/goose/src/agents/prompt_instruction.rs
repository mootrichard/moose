use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstructionId(String);

impl InstructionId {
    pub fn new_random() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstructionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstructionState {
    Active,
    Retired,
    Rejected,
}

impl Default for InstructionState {
    fn default() -> Self {
        InstructionState::Active
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstructionScope {
    Session,
    Conversation,
    Tool { name: String },
    Persistent,
}

impl Default for InstructionScope {
    fn default() -> Self {
        InstructionScope::Session
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstructionSource {
    CliDefault,
    CliUser,
    Hint { path: PathBuf },
    Recipe { id: String },
    FinalOutput,
    Api { route: String },
    Extension { name: String },
    Custom { label: String },
    Unknown,
}

impl InstructionSource {
    pub fn key(&self) -> Option<String> {
        match self {
            InstructionSource::CliDefault => Some("cli_default".to_string()),
            InstructionSource::CliUser => Some("cli_user".to_string()),
            InstructionSource::Hint { path } => Some(format!("hint:{}", path.display())),
            InstructionSource::Recipe { id } => Some(format!("recipe:{id}")),
            InstructionSource::FinalOutput => Some("final_output".to_string()),
            InstructionSource::Api { route } => Some(format!("api:{route}")),
            InstructionSource::Extension { name } => Some(format!("extension:{name}")),
            InstructionSource::Custom { label } => Some(format!("custom:{label}")),
            InstructionSource::Unknown => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            InstructionSource::CliDefault => "CLI default instructions".to_string(),
            InstructionSource::CliUser => "CLI user instructions".to_string(),
            InstructionSource::Hint { path } => format!("Hint {}", path.display()),
            InstructionSource::Recipe { id } => format!("Recipe {id}"),
            InstructionSource::FinalOutput => "Final output tool".to_string(),
            InstructionSource::Api { route } => format!("API route {route}"),
            InstructionSource::Extension { name } => format!("Extension {name}"),
            InstructionSource::Custom { label } => label.clone(),
            InstructionSource::Unknown => "Unknown".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptInstruction {
    pub id: InstructionId,
    pub source: InstructionSource,
    pub scope: InstructionScope,
    pub state: InstructionState,
    pub content: String,
    pub content_hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub order: u64,
}

impl PromptInstruction {
    pub fn new(
        id: InstructionId,
        source: InstructionSource,
        scope: InstructionScope,
        content: String,
        order: u64,
        timestamp: DateTime<Utc>,
    ) -> Self {
        let content_hash = hash_instruction_content(&content);
        PromptInstruction {
            id,
            source,
            scope,
            state: InstructionState::Active,
            content,
            content_hash,
            created_at: timestamp,
            updated_at: timestamp,
            order,
        }
    }

    pub fn update_content(&mut self, content: String) {
        self.content_hash = hash_instruction_content(&content);
        self.content = content;
        self.updated_at = Utc::now();
        self.state = InstructionState::Active;
    }
}

pub fn hash_instruction_content(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    digest.iter().map(|byte| format!("{:02x}", byte)).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptStateSnapshot {
    pub override_prompt: Option<String>,
    pub instructions: Vec<PromptInstruction>,
    pub applied_order: Vec<InstructionId>,
    pub current_date_timestamp: String,
    pub order_counter: u64,
}
