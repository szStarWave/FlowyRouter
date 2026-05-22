use futures::StreamExt;
use reqwest::Client;

use crate::api::openai::{ChatCompletionRequest, ChatCompletionResponse, FlowyMeta};
use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use std::sync::Arc;

use crate::routing::{RouteDecision, RouteTier};
use crate::stats::GatewayStats;
use crate::upstream::sse::SseStream;

struct UpstreamTarget<'a> {
    base_url: Option<&'a str>,
    api_key: Option<&'a str>,
    tier: &'a str,
}

#[derive(Clone)]
pub struct UpstreamClient {
    http: Client,
    config: AppConfig,
    stats: Arc<GatewayStats>,
}

impl UpstreamClient {
    pub fn new(config: AppConfig, stats: Arc<GatewayStats>) -> Self {
        Self {
            http: Client::new(),
            config,
            stats,
        }
    }

    pub fn edge_configured(&self) -> bool {
        self.config.edge_base_url.is_some()
    }

    pub async fn complete(
        &self,
        req: &ChatCompletionRequest,
        decision: &RouteDecision,
    ) -> AppResult<ChatCompletionResponse> {
        match decision.route {
            RouteTier::Edge => {
                let t = self.target_edge();
                self.call_target(req, t).await
            }
            RouteTier::Cloud => {
                let t = self.target_cloud();
                self.call_target(req, t).await
            }
            RouteTier::Cascade => {
                let edge = self.target_edge();
                if let Some(url) = edge.base_url {
                    if let Ok(resp) = self.call_url(req, url, edge.api_key, "edge").await {
                        if cascade_gate_pass(&resp) {
                            self.stats.record_cascade_edge_ok();
                            return Ok(attach_meta(resp, decision, false));
                        }
                    }
                }
                if edge.base_url.is_some() {
                    self.stats.record_cascade_fallback();
                }
                let cloud = self.target_cloud();
                let resp = self.call_target(req, cloud).await?;
                Ok(attach_meta(resp, decision, edge.base_url.is_some()))
            }
        }
    }

    /// Stream from a single upstream (cascade does not mid-stream fallback).
    pub async fn stream(
        &self,
        req: &ChatCompletionRequest,
        decision: &RouteDecision,
    ) -> AppResult<SseStream> {
        let target = self.resolve_stream_target(decision);
        let Some(url) = target.base_url else {
            return Err(missing_upstream(target.tier));
        };
        self.stream_url(req, url, target.api_key, target.tier).await
    }

    fn resolve_stream_target(&self, decision: &RouteDecision) -> UpstreamTarget<'_> {
        match decision.route {
            RouteTier::Edge => self.target_edge(),
            RouteTier::Cloud => self.target_cloud(),
            RouteTier::Cascade => {
                if self.config.edge_base_url.is_some() {
                    self.target_edge()
                } else {
                    self.target_cloud()
                }
            }
        }
    }

    fn target_edge(&self) -> UpstreamTarget<'_> {
        UpstreamTarget {
            base_url: self.config.edge_base_url.as_deref(),
            api_key: self.config.edge_api_key.as_deref(),
            tier: "edge",
        }
    }

    fn target_cloud(&self) -> UpstreamTarget<'_> {
        UpstreamTarget {
            base_url: self.config.cloud_base_url.as_deref(),
            api_key: self.config.cloud_api_key.as_deref(),
            tier: "cloud",
        }
    }

    async fn call_target(
        &self,
        req: &ChatCompletionRequest,
        target: UpstreamTarget<'_>,
    ) -> AppResult<ChatCompletionResponse> {
        let Some(url) = target.base_url else {
            return Err(missing_upstream(target.tier));
        };
        self.call_url(req, url, target.api_key, target.tier).await
    }

    async fn call_url(
        &self,
        req: &ChatCompletionRequest,
        base: &str,
        api_key: Option<&str>,
        tier: &str,
    ) -> AppResult<ChatCompletionResponse> {
        self.record_upstream_call(tier);
        let url = format!("{}/chat/completions", base.trim_end_matches('/'));
        let upstream_req = req.for_upstream();
        let mut builder = self.http.post(url).json(&upstream_req);
        if let Some(key) = api_key {
            builder = builder.bearer_auth(key);
        }

        let resp = builder
            .send()
            .await
            .map_err(|e| AppError::Upstream(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Upstream(format!("{status}: {body}")));
        }

        resp.json::<ChatCompletionResponse>()
            .await
            .map_err(|e| AppError::Upstream(e.to_string()))
    }

    async fn stream_url(
        &self,
        req: &ChatCompletionRequest,
        base: &str,
        api_key: Option<&str>,
        tier: &str,
    ) -> AppResult<SseStream> {
        self.record_upstream_call(tier);
        let url = format!("{}/chat/completions", base.trim_end_matches('/'));
        let upstream_req = req.for_upstream();
        let mut builder = self.http.post(url).json(&upstream_req);
        if let Some(key) = api_key {
            builder = builder.bearer_auth(key);
        }

        let resp = builder
            .send()
            .await
            .map_err(|e| AppError::Upstream(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Upstream(format!("{status}: {body}")));
        }

        Ok(Box::pin(
            resp.bytes_stream().map(|r| r.map_err(std::io::Error::other)),
        ))
    }

    fn record_upstream_call(&self, tier: &str) {
        match tier {
            "edge" => self.stats.record_upstream_edge(),
            "cloud" => self.stats.record_upstream_cloud(),
            _ => {}
        }
    }
}

fn missing_upstream(tier: &str) -> AppError {
    AppError::Unavailable(format!(
        "upstream.{tier} not configured — set [upstream.{tier}] in config.toml"
    ))
}

fn attach_meta(
    mut resp: ChatCompletionResponse,
    decision: &RouteDecision,
    fallback: bool,
) -> ChatCompletionResponse {
    let tokens_out = resp
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref())
        .map(|t| ((t.len() as f64) / 4.0).ceil() as u32)
        .unwrap_or(0);

    let tokens_in = decision.tokens_in_estimate;
    let input_ratio = if tokens_in + tokens_out > 0 {
        tokens_in as f32 / (tokens_in + tokens_out) as f32
    } else {
        0.0
    };

    resp.flowy_meta = Some(FlowyMeta {
        route: format!("{:?}", decision.route).to_ascii_lowercase(),
        fallback,
        difficulty_score: decision.difficulty,
        step_kind: format!("{:?}", decision.step_kind).to_ascii_lowercase(),
        reason_codes: decision.reason_codes.clone(),
        tokens_in,
        tokens_out,
        input_ratio,
        cloud_input_saved: decision.cloud_input_saved_estimate,
        profile: format!("{:?}", decision.profile).to_ascii_lowercase(),
    });
    resp
}

fn cascade_gate_pass(resp: &ChatCompletionResponse) -> bool {
    let Some(text) = resp.choices.first().and_then(|c| c.message.content.as_ref()) else {
        return false;
    };
    !text.is_empty() && !text.contains("不确定") && text.len() > 8
}
