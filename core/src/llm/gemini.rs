//! Google AI Studio (Gemini) REST client.
//! Wire format: POST {base}/v1beta/models/{model}:generateContent?key={key}

use std::time::Duration;

use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::llm::provider::{
    audit_prompt, classify_prompt, summarize_prompt, AuditRequest, AuditResponse,
    ClassifyRequest, ClassifyResponse, SummarizeRequest,
};
use crate::llm::sse;
use crate::models::{ChatMessage, ChatRole};

const TEST_MODEL: &str = "gemini-2.5-flash";
const PROVIDER: &str = "gemini";

pub struct GeminiClient {
    api_key: String,
    model: String,
    base_url: String,
    http: reqwest::Client,
}

impl GeminiClient {
    pub fn new(api_key: String, model: String, base_url: String) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest client");
        GeminiClient {
            api_key,
            model,
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        }
    }

    async fn generate(&self, model: &str, body: Value) -> Result<Value> {
        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.base_url, model, self.api_key
        );
        let resp = self
            .http
            .post(&url)
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
            let snippet = snippet(&text);
            return Err(match status.as_u16() {
                400 if text.contains("API_KEY_INVALID") => {
                    Error::llm(PROVIDER, "Invalid Gemini API key")
                }
                401 | 403 => Error::llm(PROVIDER, "Invalid Gemini API key"),
                429 => Error::llm(PROVIDER, "Gemini rate limit reached — try again shortly"),
                _ => Error::llm(PROVIDER, format!("HTTP {status}: {snippet}")),
            });
        }

        serde_json::from_str(&text)
            .map_err(|e| Error::llm(PROVIDER, format!("invalid JSON response: {e}")))
    }

    fn extract_text(value: &Value) -> Result<String> {
        if let Some(reason) = value
            .pointer("/promptFeedback/blockReason")
            .and_then(|v| v.as_str())
        {
            return Err(Error::llm(
                PROVIDER,
                format!("the request was blocked by Gemini (reason: {reason})"),
            ));
        }
        let parts = value
            .pointer("/candidates/0/content/parts")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::llm(PROVIDER, "Gemini returned no candidates"))?;
        let text: String = parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("");
        if text.trim().is_empty() {
            return Err(Error::llm(PROVIDER, "Gemini returned an empty response"));
        }
        Ok(text)
    }

    pub async fn test_key(&self) -> Result<()> {
        let body = json!({
            "contents": [{ "parts": [{ "text": "ping" }] }],
            "generationConfig": { "maxOutputTokens": 1 }
        });
        self.generate(TEST_MODEL, body).await.map(|_| ())
    }

    pub async fn summarize(&self, req: &SummarizeRequest) -> Result<String> {
        let body = json!({
            "contents": [{ "parts": [{ "text": summarize_prompt(req) }] }],
        });
        let value = self.generate(&self.model, body).await?;
        Ok(Self::extract_text(&value)?.trim().to_string())
    }

    pub async fn classify(&self, req: &ClassifyRequest) -> Result<ClassifyResponse> {
        // Gemini's responseSchema dialect (OpenAPI-style, UPPERCASE types).
        let schema = json!({
            "type": "OBJECT",
            "properties": {
                "path": {
                    "type": "ARRAY",
                    "items": { "type": "STRING" },
                    "description": "Target collection path, root to leaf"
                },
                "is_new": { "type": "BOOLEAN" },
                "confidence": { "type": "NUMBER" },
                "rationale": { "type": "STRING" }
            },
            "required": ["path", "is_new", "confidence", "rationale"]
        });
        let body = json!({
            "contents": [{ "parts": [{ "text": classify_prompt(req) }] }],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseSchema": schema
            }
        });
        let value = self.generate(&self.model, body).await?;
        let text = Self::extract_text(&value)?;
        serde_json::from_str(&text)
            .map_err(|e| Error::llm(PROVIDER, format!("classification was not valid JSON: {e}")))
    }

    pub async fn audit(&self, req: &AuditRequest) -> Result<AuditResponse> {
        // Gemini's responseSchema dialect (OpenAPI-style, UPPERCASE types).
        let schema = json!({
            "type": "OBJECT",
            "properties": {
                "misplaced": { "type": "BOOLEAN" },
                "path": { "type": "ARRAY", "items": { "type": "STRING" } },
                "confidence": { "type": "NUMBER" },
                "rationale": { "type": "STRING" }
            },
            "required": ["misplaced", "path", "confidence", "rationale"]
        });
        let body = json!({
            "contents": [{ "parts": [{ "text": audit_prompt(req) }] }],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseSchema": schema
            }
        });
        let value = self.generate(&self.model, body).await?;
        let text = Self::extract_text(&value)?;
        serde_json::from_str(&text)
            .map_err(|e| Error::llm(PROVIDER, format!("audit result was not valid JSON: {e}")))
    }

    /// Streamed chat (`:streamGenerateContent?alt=sse`). Emits text deltas
    /// through `on_delta` and returns the concatenated answer.
    pub async fn chat_stream<F: FnMut(&str)>(
        &self,
        system: &str,
        messages: &[ChatMessage],
        on_delta: &mut F,
    ) -> Result<String> {
        let contents: Vec<Value> = messages
            .iter()
            .map(|m| {
                json!({
                    "role": match m.role {
                        ChatRole::User => "user",
                        ChatRole::Assistant => "model",
                    },
                    "parts": [{ "text": m.content }]
                })
            })
            .collect();
        let body = json!({
            "systemInstruction": { "parts": [{ "text": system }] },
            "contents": contents,
        });
        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            self.base_url, self.model, self.api_key
        );
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::llm(PROVIDER, format!("request failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(match status.as_u16() {
                400 if text.contains("API_KEY_INVALID") => {
                    Error::llm(PROVIDER, "Invalid Gemini API key")
                }
                401 | 403 => Error::llm(PROVIDER, "Invalid Gemini API key"),
                429 => Error::llm(PROVIDER, "Gemini rate limit reached — try again shortly"),
                _ => Error::llm(PROVIDER, format!("HTTP {status}: {}", snippet(&text))),
            });
        }

        let mut full = String::new();
        sse::for_each_data(resp, |data| {
            let value: Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => return Ok(()), // tolerate non-JSON keep-alives
            };
            if let Some(reason) = value
                .pointer("/promptFeedback/blockReason")
                .and_then(|v| v.as_str())
            {
                return Err(Error::llm(
                    PROVIDER,
                    format!("the request was blocked by Gemini (reason: {reason})"),
                ));
            }
            if let Some(parts) = value
                .pointer("/candidates/0/content/parts")
                .and_then(|v| v.as_array())
            {
                for part in parts {
                    if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                        full.push_str(t);
                        on_delta(t);
                    }
                }
            }
            Ok(())
        })
        .await?;

        if full.trim().is_empty() {
            return Err(Error::llm(PROVIDER, "Gemini returned an empty response"));
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
