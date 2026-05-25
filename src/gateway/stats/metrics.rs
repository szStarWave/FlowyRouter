use crate::gateway::api::openai::{ChatCompletionResponse, Usage};

/// Metrics from a single upstream HTTP call (edge or cloud).
#[derive(Debug, Clone)]
pub struct UpstreamCallMetrics {
    pub tier: &'static str,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cached_tokens: u32,
    pub latency_ms: u64,
    pub ttft_ms: Option<u64>,
    pub stream: bool,
}

/// Client-visible response served from edge — counts toward cloud token savings.
#[derive(Debug, Clone)]
pub struct FinalResponseMetrics {
    pub served_tier: &'static str,
    pub cloud_input_saved: u32,
    pub completion_tokens: u32,
}

pub fn usage_triplet(usage: &Usage) -> (u32, u32, u32) {
    let cached = usage
        .prompt_tokens_details
        .as_ref()
        .map(|d| d.cached_tokens)
        .unwrap_or(0);
    (usage.prompt_tokens, usage.completion_tokens, cached)
}

pub fn tokens_from_response(resp: &ChatCompletionResponse, prompt_fallback: u32) -> (u32, u32, u32) {
    if let Some(usage) = &resp.usage {
        return usage_triplet(usage);
    }
    let completion = resp
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref())
        .map(|t| estimate_tokens(t.len()))
        .unwrap_or(0);
    (prompt_fallback, completion, 0)
}

pub fn estimate_tokens(char_len: usize) -> u32 {
    ((char_len as f64) / 4.0).ceil() as u32
}

/// Parse OpenAI-style SSE `data:` line for usage and content deltas.
pub fn inspect_sse_chunk(bytes: &[u8], completion_chars: &mut usize) -> Option<(u32, u32, u32)> {
    let text = std::str::from_utf8(bytes).ok()?;
    for line in text.lines() {
        let payload = line.strip_prefix("data: ")?.trim();
        if payload == "[DONE]" {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(payload).ok()?;
        if let Some(delta) = v
            .pointer("/choices/0/delta/content")
            .and_then(|c| c.as_str())
        {
            *completion_chars += delta.len();
        }
        if let Some(usage) = v.get("usage") {
            let prompt = usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let completion = usage
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let cached = usage
                .pointer("/prompt_tokens_details/cached_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            return Some((prompt, completion, cached));
        }
    }
    None
}

pub fn sse_has_content(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    for line in text.lines() {
        let Some(payload) = line.strip_prefix("data: ") else {
            continue;
        };
        if payload.trim() == "[DONE]" {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload.trim()) {
            if v.pointer("/choices/0/delta/content")
                .and_then(|c| c.as_str())
                .is_some_and(|s| !s.is_empty())
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sse_usage_chunk() {
        let chunk = br#"data: {"usage":{"prompt_tokens":100,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":80}}}

"#;
        let mut chars = 0;
        let u = inspect_sse_chunk(chunk, &mut chars).unwrap();
        assert_eq!(u, (100, 20, 80));
    }

    #[test]
    fn counts_delta_chars() {
        let chunk = br#"data: {"choices":[{"delta":{"content":"hello"}}]}

"#;
        let mut chars = 0;
        assert!(inspect_sse_chunk(chunk, &mut chars).is_none());
        assert_eq!(chars, 5);
    }
}
