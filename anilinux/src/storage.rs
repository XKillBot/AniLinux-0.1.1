use crate::models::{DownloadStatus, LocalState};
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("anilinux")
}
pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(data_dir)
        .join("anilinux")
        .join("covers")
}
pub fn load() -> LocalState {
    let mut state: LocalState = std::fs::read(data_dir().join("library.json"))
        .ok()
        .and_then(|s| serde_json::from_slice(&s).ok())
        .unwrap_or_default();
    for job in &mut state.downloads {
        if matches!(job.status, DownloadStatus::Running | DownloadStatus::Queued) {
            job.status = DownloadStatus::Cancelled;
        }
    }
    state
}
pub async fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension("tmp");
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::File::create(&tmp).await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&tmp, path)
        .await
        .context("No se pudo guardar el archivo")?;
    Ok(())
}
pub async fn save(state: &impl Serialize) -> Result<()> {
    atomic_write(
        &data_dir().join("library.json"),
        &serde_json::to_vec_pretty(state)?,
    )
    .await
}
pub fn hash(value: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
pub fn safe_name(s: &str) -> String {
    let value: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(96)
        .scan(0usize, |bytes, c| {
            *bytes += c.len_utf8();
            (*bytes <= 160).then_some(c)
        })
        .collect();
    let value = value.trim_matches([' ', '.']);
    if value.is_empty() {
        "episodio".into()
    } else {
        value.into()
    }
}
