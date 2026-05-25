use anyhow::{Context, Result, bail};
use reqwest::Client;
use reqwest::RequestBuilder;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct GatewayClient {
    base: String,
    http: Client,
    api_key: Option<String>,
    admin_token: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub edge_configured: bool,
    pub cloud_configured: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GatewayStatus {
    pub status: String,
    pub version: String,
    pub listen: String,
    pub pid: u32,
    pub uptime_secs: u64,
    pub edge_configured: bool,
    pub cloud_configured: bool,
    pub default_profile: String,
    pub pid_file: String,
    pub data_dir: String,
}

impl GatewayClient {
    pub fn base_url(&self) -> &str {
        &self.base
    }

    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        admin_token: Option<String>,
    ) -> Self {
        Self {
            base: base_url.into().trim_end_matches('/').to_string(),
            http: Client::new(),
            api_key,
            admin_token,
        }
    }

    pub async fn health(&self) -> Result<HealthResponse> {
        self.get("/health", false).await
    }

    pub async fn status(&self) -> Result<GatewayStatus> {
        self.get("/v1/admin/status", false).await
    }

    pub async fn stats_session(&self) -> Result<crate::stats_cmd::GatewayStats> {
        self.get("/v1/admin/stats", false).await
    }

    pub async fn stats_global(&self) -> Result<crate::stats_cmd::GatewayStats> {
        self.get("/v1/admin/stats?scope=global", false).await
    }

    pub async fn shutdown(&self) -> Result<serde_json::Value> {
        let mut req = self.http.post(format!("{}/v1/admin/shutdown", self.base));
        req = self.attach_admin_token(req);
        let resp = req
            .send()
            .await
            .context("POST /v1/admin/shutdown")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("shutdown failed {status}: {body}");
        }
        resp.json().await.context("decode shutdown response")
    }

    fn attach_api_key(&self, req: RequestBuilder) -> RequestBuilder {
        if let Some(key) = &self.api_key {
            req.bearer_auth(key)
        } else {
            req
        }
    }

    fn attach_admin_token(&self, req: RequestBuilder) -> RequestBuilder {
        if let Some(token) = &self.admin_token {
            req.header("X-Flowy-Admin-Token", token)
        } else {
            req
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, with_api_key: bool) -> Result<T> {
        let mut req = self.http.get(format!("{}{}", self.base, path));
        if with_api_key {
            req = self.attach_api_key(req);
        }
        let resp = req.send().await.with_context(|| format!("GET {path}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("GET {path} failed {status}: {body}");
        }
        resp.json().await.with_context(|| format!("decode GET {path}"))
    }
}
