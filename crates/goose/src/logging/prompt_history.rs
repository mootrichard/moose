use crate::agents::prompt_instruction::{
    hash_instruction_content, InstructionScope, InstructionSource,
};
use crate::config::paths::Paths;
use crate::utils::sanitize_unicode_tags;
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct PromptHistoryWriter {
    path: PathBuf,
    file: Mutex<File>,
    project_path: PathBuf,
    session_id: String,
}

impl PromptHistoryWriter {
    pub fn new(project_path: &Path, session_id: &str) -> Result<Self> {
        let directory = prompt_history_root().join(hash_project_path(project_path));
        fs::create_dir_all(&directory).context("Failed to create prompt-history directory")?;

        let path = directory.join(format!("{session_id}.jsonl"));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to open prompt history file at {}", path.display()))?;

        Ok(Self {
            path,
            file: Mutex::new(file),
            project_path: project_path.to_path_buf(),
            session_id: session_id.to_string(),
        })
    }

    pub fn record(&self, event: InstructionEvent) -> Result<()> {
        let mut guard = self
            .file
            .lock()
            .map_err(|_| anyhow!("Prompt history writer lock poisoned"))?;
        let line = serde_json::to_string(&event)?;
        guard
            .write_all(line.as_bytes())
            .and_then(|_| guard.write_all(b"\n"))
            .context("Failed to write prompt history entry")?;
        Ok(())
    }

    pub fn record_instruction(
        &self,
        action: InstructionAction,
        instruction_id: &str,
        source: &InstructionSource,
        scope: &InstructionScope,
        content: &str,
        order: u64,
    ) -> Result<()> {
        let event = InstructionEvent::new(
            action,
            &self.session_id,
            &self.project_path,
            instruction_id,
            source,
            scope,
            content,
            order,
        );
        self.record(event)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Serialize)]
pub enum InstructionAction {
    Register,
    Apply,
    Refresh,
    Retire,
}

#[derive(Debug, Serialize)]
pub struct InstructionEvent {
    pub timestamp: DateTime<Utc>,
    pub action: InstructionAction,
    pub session_id: String,
    pub project_path: String,
    pub instruction_id: String,
    pub source_label: String,
    pub scope: InstructionScope,
    pub content_hash: String,
    pub preview: String,
    pub order: u64,
}

impl InstructionEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        action: InstructionAction,
        session_id: &str,
        project_path: &Path,
        instruction_id: &str,
        source: &InstructionSource,
        scope: &InstructionScope,
        content: &str,
        order: u64,
    ) -> Self {
        InstructionEvent {
            timestamp: Utc::now(),
            action,
            session_id: session_id.to_string(),
            project_path: project_path.to_string_lossy().to_string(),
            instruction_id: instruction_id.to_string(),
            source_label: source.label(),
            scope: scope.clone(),
            content_hash: hash_instruction_content(content),
            preview: preview_from_content(content),
            order,
        }
    }
}

fn preview_from_content(content: &str) -> String {
    let sanitized = sanitize_unicode_tags(content);
    const MAX_PREVIEW_CHARS: usize = 200;
    if sanitized.chars().count() <= MAX_PREVIEW_CHARS {
        sanitized
    } else {
        sanitized
            .chars()
            .take(MAX_PREVIEW_CHARS)
            .collect::<String>()
            + "…"
    }
}

fn prompt_history_root() -> PathBuf {
    Paths::in_config_dir("prompt-history")
}

fn hash_project_path(path: &Path) -> String {
    let normalized = path.to_string_lossy();
    let digest = Sha256::digest(normalized.as_bytes());
    digest.iter().map(|byte| format!("{:02x}", byte)).collect()
}
