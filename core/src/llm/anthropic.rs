//! Anthropic Messages API client (raw HTTP — there is no official Rust SDK).
//!
//! POST {base}/v1/messages with headers `x-api-key`, `anthropic-version:
//! 2023-06-01`. NOTE: no `temperature`/`top_p`/`top_k` — these are removed on
//! Opus 4.7+ models and the request would 400.

use std::time::Duration;

use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::llm::provider::{
    classify_prompt, classify_schema, summarize_prompt, ClassifyRequest, ClassifyResponse,
    SummarizeRequest,
};

const API_VERSION: &str = "2023-06-01";
const PROVIDER: &str = "anthropic";

pub struct AnthropicClient {
    api_key: String,
    model: String,
    base_url: String,
    http: reqwest::Client,
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
        }
    }

    async fn messages(&self, body: Value) -> Result<Value> {
        let url = format!("{}/v1/messages", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::llm(PROVIDER, format!("request failed: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::llm(PROVIDER, format!("reading response failed: {e}")))?;

        if !status.is_success() {
            let api_message = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| {
                    v.pointer("/error/message")
                        .and_then(|m| m.as_str())
                        .map(String::from)
                })
                .unwrap_or_else(|| snippet(&text));
            return Err(match status.as_u16() {
                401 => Error::llm(PROVIDER, "Invalid Anthropic API key"),
                404 => Error::llm(
                    PROVIDER,
                    format!("Unknown model '{}': {api_message}", self.model),
                ),
                429 => Error::llm(PROVIDER, "Anthropic rate limit reached — try again shortly"),
                _ => Error::llm(PROVIDER, format!("HTTP {status}: {api_message}")),
            });
        }

        serde_json::from_str(&text)
            .map_err(|e| Error::llm(PROVIDER, format!("invalid JSON response: {e}")))
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
