use super::file::{ConfigFile, UpstreamEndpoint, UpstreamSection};
use serde::{Deserialize, Serialize};

pub const CLOUD_MODEL_AUTO: &str = "auto";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamSetupView {
    pub edge: Option<UpstreamEndpointView>,
    pub cloud: Option<UpstreamEndpointView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamEndpointView {
    pub configured: bool,
    pub base_url: String,
    pub model: Option<String>,
    pub api_key_set: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpstreamSetupUpdate {
    #[serde(default)]
    pub edge: Option<UpstreamEndpointPatch>,
    #[serde(default)]
    pub cloud: Option<UpstreamEndpointPatch>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpstreamEndpointPatch {
    /// Set to empty string to clear `base_url`.
    pub base_url: Option<String>,
    /// Omit to keep existing; empty string clears the key.
    pub api_key: Option<String>,
    pub model: Option<String>,
    /// When true, remove this tier entirely (edge only).
    #[serde(default)]
    pub clear: bool,
}

pub fn endpoint_configured(ep: &UpstreamEndpoint) -> bool {
    !ep.base_url.trim().is_empty()
}

pub fn view_from_config(file: &ConfigFile) -> UpstreamSetupView {
    UpstreamSetupView {
        edge: file.upstream.edge.as_ref().map(endpoint_view),
        cloud: file.upstream.cloud.as_ref().map(endpoint_view),
    }
}

fn endpoint_view(ep: &UpstreamEndpoint) -> UpstreamEndpointView {
    UpstreamEndpointView {
        configured: endpoint_configured(ep),
        base_url: ep.base_url.clone(),
        model: ep.model.clone(),
        api_key_set: ep
            .api_key
            .as_ref()
            .is_some_and(|k| !k.trim().is_empty()),
    }
}

/// Default upstream block: cloud model `auto`, edge unset.
pub fn apply_default_upstream(file: &mut ConfigFile) {
    file.upstream = UpstreamSection {
        cloud: Some(UpstreamEndpoint {
            base_url: String::new(),
            api_key: None,
            model: Some(CLOUD_MODEL_AUTO.to_string()),
        }),
        edge: None,
    };
}

pub fn apply_upstream_patch(file: &mut ConfigFile, patch: &UpstreamSetupUpdate) {
    if let Some(edge) = &patch.edge {
        apply_tier_patch(&mut file.upstream.edge, edge);
    }
    if let Some(cloud) = &patch.cloud {
        apply_tier_patch(&mut file.upstream.cloud, cloud);
    }
}

fn apply_tier_patch(slot: &mut Option<UpstreamEndpoint>, patch: &UpstreamEndpointPatch) {
    if patch.clear {
        *slot = None;
        return;
    }

    let entry = slot.get_or_insert_with(|| UpstreamEndpoint {
        base_url: String::new(),
        api_key: None,
        model: None,
    });

    if let Some(url) = &patch.base_url {
        entry.base_url = url.trim().to_string();
    }
    if let Some(model) = &patch.model {
        let m = model.trim();
        entry.model = if m.is_empty() { None } else { Some(m.to_string()) };
    }
    if let Some(key) = &patch.api_key {
        let k = key.trim();
        entry.api_key = if k.is_empty() { None } else { Some(k.to_string()) };
    }

    if !endpoint_configured(entry)
        && entry.api_key.is_none()
        && entry.model.is_none()
    {
        *slot = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_setup_cloud_auto_edge_empty() {
        let mut file = ConfigFile::default();
        apply_default_upstream(&mut file);
        assert!(file.upstream.edge.is_none());
        let cloud = file.upstream.cloud.as_ref().unwrap();
        assert_eq!(cloud.model.as_deref(), Some("auto"));
        assert!(cloud.base_url.is_empty());
        let view = view_from_config(&file);
        assert!(!view.cloud.as_ref().unwrap().configured);
        assert_eq!(view.cloud.as_ref().unwrap().model.as_deref(), Some("auto"));
    }

    #[test]
    fn patch_cloud_url() {
        let mut file = ConfigFile::default();
        apply_default_upstream(&mut file);
        apply_upstream_patch(
            &mut file,
            &UpstreamSetupUpdate {
                cloud: Some(UpstreamEndpointPatch {
                    base_url: Some("https://api.deepseek.com/v1".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        assert!(endpoint_configured(file.upstream.cloud.as_ref().unwrap()));
    }
}
