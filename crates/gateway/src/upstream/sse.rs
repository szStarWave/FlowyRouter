use std::pin::Pin;

use bytes::Bytes;

pub type SseStream = Pin<Box<dyn futures::Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::openai::{ChatCompletionRequest, Message, Role};
    use futures::stream::{self, StreamExt};
    use serde_json::json;

    fn stub_sse_stream(req: &ChatCompletionRequest, tier: &str) -> SseStream {
        let content = if tier == "edge" {
            "[flowy-router] edge stub — configure [upstream.edge] in ~/.flowy-router/config.toml"
        } else {
            "[flowy-router] cloud stub — configure [upstream.cloud] in ~/.flowy-router/config.toml"
        };

        let id = format!("flowy-stub-{}", uuid::Uuid::new_v4());
        let created = now_epoch();
        let model = req.model.clone();

        let first = json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": { "role": "assistant", "content": content },
                "finish_reason": null
            }]
        });
        let last = json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        });

        let events = vec![
            format!("data: {first}\n\n"),
            format!("data: {last}\n\n"),
            "data: [DONE]\n\n".to_string(),
        ];

        Box::pin(stream::iter(events).map(|line| Ok(Bytes::from(line))))
    }

    fn now_epoch() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    #[tokio::test]
    async fn stub_sse_emits_openai_chunks() {
        let req = ChatCompletionRequest {
            model: "flowy-auto".to_string(),
            messages: vec![Message {
                role: Role::User,
                content: Some("hi".to_string()),
                content_parts: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: vec![],
            stream: true,
            tool_choice: None,
            max_tokens: None,
            ..Default::default()
        };
        let mut stream = stub_sse_stream(&req, "edge");
        let first = stream.next().await.unwrap().unwrap();
        let text = String::from_utf8(first.to_vec()).unwrap();
        assert!(text.starts_with("data: "));
        assert!(text.contains("chat.completion.chunk"));
    }
}
