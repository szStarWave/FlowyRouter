use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

pub const MULTIMODAL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TierSupport {
    #[default]
    Unknown,
    Supported,
    Unsupported,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelCapability {
    #[serde(default)]
    pub edge: TierSupport,
    #[serde(default)]
    pub cloud: TierSupport,
    #[serde(default)]
    pub probed_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MultimodalData {
    pub version: u32,
    #[serde(default)]
    pub upstream_fingerprint: String,
    #[serde(default)]
    pub by_model: HashMap<String, ModelCapability>,
}

impl MultimodalData {
    pub fn touch(&mut self) {
        self.version = MULTIMODAL_VERSION;
    }

    pub fn model_entry(&mut self, model: &str) -> &mut ModelCapability {
        self.by_model.entry(model.to_string()).or_default()
    }
}

pub fn load(path: &Path) -> anyhow::Result<MultimodalData> {
    if !path.exists() {
        return Ok(MultimodalData::default());
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read multimodal {}", path.display()))?;
    match serde_json::from_str(&text) {
        Ok(data) => Ok(data),
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "invalid multimodal capability file, starting fresh"
            );
            Ok(MultimodalData::default())
        }
    }
}

pub fn save(path: &Path, data: &MultimodalData) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create multimodal dir {}", parent.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(data)?;
    std::fs::write(&tmp, json).with_context(|| format!("write multimodal {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("rename multimodal {}", path.display()))?;
    Ok(())
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
