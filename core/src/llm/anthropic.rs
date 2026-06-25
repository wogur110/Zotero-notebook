//! Anthropic Messages API client (raw HTTP — there is no official Rust SDK).
//!
//! POST {base}/v1/messages with headers `x-api-key`, `anthropic-version:
//! 2023-06-01`. NOTE: no `temperature`/`top_p`/`top_k` — these are removed on
//! Opus 4.7+ models and the request would 400.

use std::time::Duration;

use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::llm::provider::{
    audit_prompt, audit_schema, classify_prompt, classify_schema, summarize_prompt,
    AuditRequest, AuditResponse, ClassifyRequest, ClassifyResponse, SummarizeRequest, Usage,
};
use crate::llm::sse;
use crate::models::{ChatMessage, ChatRole};

const API_VERSION: &str = "2023-06-01";
const PROVIDER: &str = "anthropic";

pub struct AnthropicClient {
    api_key: String,
    model: String,
    base_url: String,
    http: reqwest::Client,
    last_usage: std::sync::Mutex<Option<Usage>>,
}

impl AnthropicClient {
    pub fn new(api_key: String, model: String, base_url: String) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest client");
        AnthropicClient {
            api_key,
            model,
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
            last_usage: std::sync::Mutex::new(None),
        }
    }

    fn record_usage(&self, value: &Value) {
        let input = value
            .pointer("/usage/input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let output = value
            .pointer("/usage/output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        *self.last_usage.lock().expect("usage mutex") = Some(Usage {
            input_tokens: input,
            output_tokens: output,
        });
    }

    pub fn last_usage(&self) -> Option<Usage> {
        *self.last_usage.lock().expect("usage mutex")
    }

    fn request(&self, body: &Value) -> reqwest::RequestBuilder {
        self.http
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(body)
    }

    fn map_http_error(&self, status: reqwest::StatusCode, body: &str) -> Error {
        let api_message = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| snippet(body));
        match status.as_u16() {
            401 => Error::llm(PROVIDER, "Invalid Anthropic API key"),
            404 => Error::llm(
                PROVIDER,
                format!("Unknown model '{}': {api_message}", self.model),
            ),
            429 => Error::llm(PROVIDER, "Anthropic rate limit reached — try again shortly"),
            _ => Error::llm(PROVIDER, format!("HTTP {status}: {api_message}")),
        }
    }

    async fn messages(&self, body: Value) -> Result<Value> {
        let resp = self
            .request(&body)
            .send()
            .await
            .map_err(|e| Error::llm(PROVIDER, format!("request failed: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::llm(PROVIDER, format!("reading response failed: {e}")))?;

        if !status.is_success() {
            return Err(self.map_http_error(status, &text));
        }

        let value: Value = serde_json::from_str(&text)
            .map_err(|e| Error::llm(PROVIDER, format!("invalid JSON response: {e}")))?;
        self.record_usage(&value);
        Ok(value)
    }

    /// Concatenated text blocks; errors on safety refusals and truncation.
    fn extract_text(value: &Value, classifying: bool) -> Result<String> {
        let stop_reason = value.get("stop_reason").and_then(|v| v.as_str());
        if stop_reason == Some("refusal") {
            return Err(Error::llm(
                PROVIDER,
                "the request was declined by the model's safety system",
            ));
        }
        if classifying && stop_reason == Some("max_tokens") {
            return Err(Error::llm(
                PROVIDER,
                "classification output was truncated (max_tokens reached)",
            ));
        }
        let content = value
            .get("content")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::llm(PROVIDER, "response had no content"))?;
        let text: String = content
            .iter()
            .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("");
        if text.trim().is_empty() {
            return Err(Error::llm(PROVIDER, "response contained no text"));
        }
        Ok(text)
    }

    pub async fn test_key(&self) -> Result<()> {
        let body = json!({
            "model": self.model,
            "max_tokens": 8,
            "messages": [{ "role": "user", "content": "ping" }]
        });
        self.messages(body).await.map(|_| ())
    }

    pub async fn summarize(&self, req: &SummarizeRequest) -> Result<String> {
        let body = json!({
            "model": self.model,
            "max_tokens": 1024,
            "messages": [{ "role": "user", "content": summarize_prompt(req) }]
        });
        let value = self.messages(body).await?;
        Ok(Self::extract_text(&value, false)?.trim().to_string())
    }

    pub async fn classify(&self, req: &ClassifyRequest) -> Result<ClassifyResponse> {
        let body = json!({
            "model": self.model,
            "max_tokens": 600,
            "messages": [{ "role": "user", "content": classify_prompt(req) }],
            "output_config": {
                "format": {
                    "type": "json_schema",
                    "schema": classify_schema()
                }
            }
        });
        let value = self.messages(body).await?;
        let text = Self::extract_text(&value, true)?;
        serde_json::from_str(&text)
            .map_err(|e| Error::llm(PROVIDER, format!("classification was not valid JSON: {e}")))
    }

    pub async fn audit(&self, req: &AuditRequest) -> Result<AuditResponse> {
        let body = json!({
            "model": self.model,
            "max_tokens": 600,
            "messages": [{ "role": "user", "content": audit_prompt(req) }],
            "output_config": {
                "format": {
                    "type": "json_schema",
                    "schema": audit_schema()
                }
            }
        });
        let value = self.messages(body).await?;
        let text = Self::extract_text(&value, true)?;
        serde_json::from_str(&text)
            .map_err(|e| Error::llm(PROVIDER, format!("audit result was not valid JSON: {e}")))
    }

    /// Streamed chat (`"stream": true`). Emits text deltas through
    /// `on_delta` and returns the concatenated answer.
    pub async fn chat_stream<F: FnMut(&str)>(
        &self,
        system: &str,
        messages: &[ChatMessage],
        on_delta: &mut F,
    ) -> Result<String> {
        let wire_messages: Vec<Value> = messages
            .iter()
            .map(|m| {
                json!({
                    "role": match m.role {
                        ChatRole::User => "user",
                        ChatRole::Assistant => "assistant",
                    },
                    "content": m.content
                })
            })
            .collect();
        let body = json!({
            "model": self.model,
            "max_tokens": 2048,
            "stream": true,
            "system": system,
            "messages": wire_messages,
        });
        let resp = self
            .request(&body)
            .send()
            .await
            .map_err(|e| Error::llm(PROVIDER, format!("request failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(self.map_http_error(status, &text));
        }

        let mut full = String::new();
        sse::for_each_data(resp, |data| {
            let value: Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => return Ok(()),
            };
            match value.get("type").and_then(|t| t.as_str()) {
                Some("content_block_delta") => {
                    if value.pointer("/delta/type").and_then(|t| t.as_str())
                        == Some("text_delta")
                    {
                        if let Some(t) = value.pointer("/delta/text").and_then(|t| t.as_str()) {
                            full.push_str(t);
                            on_delta(t);
                        }
                    }
                }
                Some("message_delta") => {
                    if value.pointer("/delta/stop_reason").and_then(|s| s.as_str())
                        == Some("refusal")
                    {
                        return Err(Error::llm(
                            PROVIDER,
                            "the request was declined by the model's safety system",
                        ));
                    }
                }
                Some("error") => {
                    let msg = value
                        .pointer("/error/message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("stream error");
                    return Err(Error::llm(PROVIDER, msg));
                }
                _ => {}
            }
            Ok(())
        })
        .await?;

        if full.trim().is_empty() {
            return Err(Error::llm(PROVIDER, "response contained no text"));
        }
        Ok(full)
    }
}

fn snippet(s: &str) -> String {
    let s = s.trim().replace('\n', " ");
    if s.len() <= 200 {
        s
    } else {
        let mut end = 200;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}
