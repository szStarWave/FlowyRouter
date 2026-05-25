use crate::gateway::config::AppConfig;

/// Fingerprint of upstream endpoints; when this changes, multimodal probes reset.
pub fn upstream_fingerprint(config: &AppConfig) -> String {
    format!(
        "edge:{}|ek:{}|cloud:{}|ck:{}",
        config.edge_base_url.as_deref().unwrap_or(""),
        config.edge_api_key.as_deref().unwrap_or(""),
        config.cloud_base_url.as_deref().unwrap_or(""),
        config.cloud_api_key.as_deref().unwrap_or(""),
    )
}
