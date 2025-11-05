use crate::agents::prompt_instruction::PromptStateSnapshot;
use crate::config::paths::Paths;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub struct PromptStateStore;

impl PromptStateStore {
    pub fn save(
        snapshot: &PromptStateSnapshot,
        project_path: &Path,
        session_id: &str,
    ) -> Result<PathBuf> {
        let path = snapshot_path(project_path, session_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Failed to create prompt-state directory")?;
        }
        let payload =
            serde_json::to_vec_pretty(snapshot).context("Failed to serialize prompt snapshot")?;
        fs::write(&path, payload).with_context(|| {
            format!("Failed to write prompt snapshot {}", path.to_string_lossy())
        })?;
        Ok(path)
    }

    pub fn load(project_path: &Path, session_id: &str) -> Result<Option<PromptStateSnapshot>> {
        let path = snapshot_path(project_path, session_id);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read(&path).with_context(|| {
            format!("Failed to read prompt snapshot {}", path.to_string_lossy())
        })?;
        let snapshot = serde_json::from_slice(&data).context("Failed to deserialize snapshot")?;
        Ok(Some(snapshot))
    }

    pub fn remove(project_path: &Path, session_id: &str) -> Result<()> {
        let path = snapshot_path(project_path, session_id);
        if path.exists() {
            fs::remove_file(&path).with_context(|| {
                format!(
                    "Failed to remove prompt snapshot {}",
                    path.to_string_lossy()
                )
            })?;
        }
        Ok(())
    }
}

fn snapshot_path(project_path: &Path, session_id: &str) -> PathBuf {
    let directory = Paths::in_config_dir("prompt-state").join(hash_project_path(project_path));
    directory.join(format!("{session_id}.json"))
}

fn hash_project_path(path: &Path) -> String {
    let normalized = path.to_string_lossy();
    let digest = Sha256::digest(normalized.as_bytes());
    digest.iter().map(|byte| format!("{:02x}", byte)).collect()
}
