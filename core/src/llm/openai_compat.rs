//! Client for OpenAI-compatible chat-completions servers — the de-facto
//! protocol of local LLM runtimes (Ollama's `/v1`, LM Studio, llama.cpp
//! server, vLLM, …).
//!
//! Differences from the cloud clients that shape this code:
//! - No API key is required by most local servers; when one is configured
//!   it is sent as `Authorization: Bearer`.
//! - Structured-output support (`response_format: json_schema`) varies by
//!   runtime and model. We always embed the schema in the prompt, request
//!   `response_format` opportunistically, retry once without it if the
//!   server rejects the parameter, and run the reply through a tolerant
//!   JSON extractor (strips code fences / surrounding prose).
//! - Local models are slower: generous timeouts.

use std::time::Duration;

use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::llm::provider::{
    audit_prompt, audit_schema, classify_prompt, classify_schema, summarize_prompt,
    AuditRequest, AuditResponse, ClassifyRequest, ClassifyResponse, SummarizeRequest, Usage,
};
use crate::llm::sse;
use crate::models::{ChatMessage, ChatRole};

const PROVIDER: &str = "local";

pub struct OpenAiCompatClient {
    api_key: Option<String>,
    model: String,
    base_url: String,
    http: reqwest::Client,
    last_usage: std::sync::Mutex<Option<Usage>>,
}

impl OpenAiCompatClient {
    pub fn new(api_key: Option<String>, model: String, base_url: String) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            // Local inference on CPU/small GPUs can be slow.
            .timeout(Duration::from_secs(600))
            .build()
            .expect("reqwest client");
        OpenAiCompatClient {
            api_key,
            model,
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
            last_usage: std::sync::Mutex::new(None),
        }
    }

    fn record_usage(&self, value: &Value) {
        let input = value
            .pointer("/usage/prompt_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let output = value
            .pointer("/usage/completion_tokens")
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
        let mut req = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .header("content-type", "application/json")
            .json(body);
        if let Some(key) = self.api_key.as_deref().filter(|k| !k.is_empty()) {
            req = req.header("authorization", format!("Bearer {key}"));
        }
        req
    }

    fn map_send_err(&self, e: reqwest::Error) -> Error {
        if e.is_connect() {
            Error::llm(
                PROVIDER,
                format!(
                    "could not reach the local LLM server at {} — is it running? \
                     (Ollama: `ollama serve` usually runs automatically; \
                     LM Studio: start the local server)",
                    self.base_url
                ),
            )
        } else if e.is_timeout() {
            Error::llm(
                PROVIDER,
                "the local model took too long to answer — try a smaller model",
            )
        } else {
            Error::llm(PROVIDER, format!("request failed: {e}"))
        }
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
            401 | 403 => Error::llm(
                PROVIDER,
                "the local server rejected the request — set or clear the API key in Settings",
            ),
            404 => Error::llm(
                PROVIDER,
                format!(
                    "model '{}' not found on the local server: {api_message} \
                     (Ollama: `ollama pull {}`)",
                    self.model, self.model
                ),
            ),
            _ => Error::llm(PROVIDER, format!("HTTP {status}: {api_message}")),
        }
    }

    async fn completion(&self, body: Value) -> Result<Value> {
        let resp = self
            .request(&body)
            .send()
            .await
            .map_err(|e| self.map_send_err(e))?;
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

    fn extract_content(value: &Value) -> Result<String> {
        let content = value
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .unwrap_or("");
        if content.trim().is_empty() {
            return Err(Error::llm(PROVIDER, "the model returned an empty response"));
        }
        Ok(content.to_string())
    }

    /// A structured request: schema embedded in the prompt AND requested via
    /// `response_format`; retried once without `response_format` when the
    /// server rejects the parameter (HTTP 400).
    async fn structured(&self, prompt: String, schema: Value, kind: &str) -> Result<String> {
        let prompt = format!(
            "{prompt}\n\nRespond with ONLY a single JSON object (no markdown, no \
             explanation) matching this JSON Schema:\n{schema}\n"
        );
        let with_format = json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": prompt }],
            "response_format": {
                "type": "json_schema",
                "json_schema": { "name": kind, "schema": schema, "strict": true }
            }
        });
        let value = match self.completion(with_format).await {
            Ok(v) => v,
            Err(Error::Llm { message, .. })
                if message.starts_with("HTTP 400") && message.contains("response_format") =>
            {
                // Server doesn't know response_format — the prompt still
                // carries the schema, so plain mode usually works.
                let plain = json!({
                    "model": self.model,
                    "messages": [{ "role": "user", "content": prompt }],
                });
                self.completion(plain).await?
            }
            Err(e) => return Err(e),
        };
        Ok(Self::extract_content(&value)?)
    }

    pub async fn test_key(&self) -> Result<()> {
        let body = json!({
            "model": self.model,
            "max_tokens": 4,
            "messages": [{ "role": "user", "content": "ping" }]
        });
        self.completion(body).await.map(|_| ())
    }

    pub async fn summarize(&self, req: &SummarizeRequest) -> Result<String> {
        let body = json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": summarize_prompt(req) }]
        });
        let value = self.completion(body).await?;
        Ok(Self::extract_content(&value)?.trim().to_string())
    }

    pub async fn classify(&self, req: &ClassifyRequest) -> Result<ClassifyResponse> {
        let text = self
            .structured(classify_prompt(req), classify_schema(), "classification")
            .await?;
        serde_json::from_str(extract_json(&text))
            .map_err(|e| Error::llm(PROVIDER, format!("classification was not valid JSON: {e}")))
    }

    pub async fn audit(&self, req: &AuditRequest) -> Result<AuditResponse> {
        let text = self
            .structured(audit_prompt(req), audit_schema(), "audit")
            .await?;
        serde_json::from_str(extract_json(&text))
            .map_err(|e| Error::llm(PROVIDER, format!("audit result was not valid JSON: {e}")))
    }

    /// Streamed chat (`"stream": true`, SSE with `data: {...}` chunks and a
    /// final `data: [DONE]`).
    pub async fn chat_stream<F: FnMut(&str)>(
        &self,
        system: &str,
        messages: &[ChatMessage],
        on_delta: &mut F,
    ) -> Result<String> {
        let mut wire: Vec<Value> = vec![json!({ "role": "system", "content": system })];
        wire.extend(messages.iter().map(|m| {
            json!({
                "role": match m.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                },
                "content": m.content
            })
        }));
        let body = json!({
            "model": self.model,
            "stream": true,
            "messages": wire,
        });
        let resp = self
            .request(&body)
            .send()
            .await
            .map_err(|e| self.map_send_err(e))?;
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
            if let Some(t) = value
                .pointer("/choices/0/delta/content")
                .and_then(|c| c.as_str())
            {
                if !t.is_empty() {
                    full.push_str(t);
                    on_delta(t);
                }
            }
            Ok(())
        })
        .await?;

        if full.trim().is_empty() {
            return Err(Error::llm(PROVIDER, "the model returned an empty response"));
        }
        Ok(full)
    }
}

/// Local models often wrap JSON in markdown fences or prose despite
/// instructions. Pull out the first plausible JSON object.
fn extract_json(text: &str) -> &str {
    let trimmed = text.trim();
    // ```json ... ``` or ``` ... ```
    let inner = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|rest| rest.rsplit_once("```"))
        .map(|(body, _)| body.trim())
        .unwrap_or(trimmed);
    if inner.starts_with('{') {
        return inner;
    }
    // Last resort: substring from the first '{' to the last '}'.
    match (inner.find('{'), inner.rfind('}')) {
        (Some(start), Some(end)) if end > start => &inner[start..=end],
        _ => inner,
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

#[cfg(test)]
mod tests {
    use super::extract_json;

    #[test]
    fn json_extraction_handles_fences_and_prose() {
        assert_eq!(extract_json(r#"{"a":1}"#), r#"{"a":1}"#);
        assert_eq!(extract_json("```json\n{\"a\":1}\n```"), r#"{"a":1}"#);
        assert_eq!(extract_json("```\n{\"a\":1}\n```"), r#"{"a":1}"#);
        assert_eq!(
            extract_json("Sure! Here is the JSON: {\"a\": 1} Hope that helps."),
            r#"{"a": 1}"#
        );
    }
}
