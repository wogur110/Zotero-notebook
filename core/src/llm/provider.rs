//! Provider-agnostic LLM interface.
//!
//! `AnyProvider` is enum dispatch over the concrete clients (no trait
//! objects). Both clients must expose the same inherent methods:
//!
//! ```ignore
//! pub fn new(api_key: String, model: String, base_url: String) -> Self;
//! pub async fn test_key(&self) -> Result<()>;
//! pub async fn summarize(&self, req: &SummarizeRequest) -> Result<String>;
//! pub async fn classify(&self, req: &ClassifyRequest) -> Result<ClassifyResponse>;
//! ```
//!
//! The prompts below are shared verbatim by both providers so behavior is
//! comparable across them; the clients only differ in wire format.

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::llm::{anthropic::AnthropicClient, gemini::GeminiClient};
use crate::models::ProviderId;

/// Default public base URLs (overridable for tests).
pub const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com";
pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SummarizeRequest {
    pub title: String,
    pub creators: Vec<String>,
    pub year: Option<i32>,
    pub publication: Option<String>,
    pub abstract_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClassifyRequest {
    pub title: String,
    pub creators: Vec<String>,
    pub year: Option<i32>,
    pub publication: Option<String>,
    pub abstract_text: Option<String>,
    pub tags: Vec<String>,
    /// Existing collection paths, root → leaf (the "Unclassified" collection
    /// itself is excluded by the caller).
    pub existing_paths: Vec<Vec<String>>,
}

/// What the model must return for a classify call. Clients enforce this
/// shape with structured output (Gemini `responseSchema`, Anthropic
/// `output_config.format`) and deserialize into it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClassifyResponse {
    /// Chosen collection path, root → leaf.
    pub path: Vec<String>,
    /// Model's claim that this is a new path (the caller re-verifies
    /// against the actual tree, case-insensitively).
    pub is_new: bool,
    /// 0.0–1.0
    pub confidence: f64,
    pub rationale: String,
}

/// JSON Schema for `ClassifyResponse`, shared by both clients.
pub fn classify_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Target collection path, root to leaf, e.g. [\"Computer Vision\", \"Diffusion Models\"]"
            },
            "is_new": { "type": "boolean" },
            "confidence": { "type": "number" },
            "rationale": { "type": "string" }
        },
        "required": ["path", "is_new", "confidence", "rationale"],
        "additionalProperties": false
    })
}

fn meta_block(
    title: &str,
    creators: &[String],
    year: Option<i32>,
    publication: Option<&str>,
    abstract_text: Option<&str>,
) -> String {
    let mut s = format!("Title: {title}\n");
    if !creators.is_empty() {
        s.push_str(&format!("Authors: {}\n", creators.join(", ")));
    }
    if let Some(y) = year {
        s.push_str(&format!("Year: {y}\n"));
    }
    if let Some(p) = publication {
        s.push_str(&format!("Venue: {p}\n"));
    }
    match abstract_text {
        Some(a) if !a.trim().is_empty() => s.push_str(&format!("Abstract: {}\n", a.trim())),
        _ => s.push_str("Abstract: (not available)\n"),
    }
    s
}

pub fn summarize_prompt(req: &SummarizeRequest) -> String {
    format!(
        "You are helping a researcher maintain notes on academic papers.\n\
         Write a summary of the following paper in English, 5 to 8 sentences,\n\
         as a single plain-text paragraph (no markdown, no headings, no lists).\n\
         Cover: the problem addressed, the key idea or method, and the main\n\
         results or significance. Base the summary only on the metadata below;\n\
         do not invent specific numbers that are not present.\n\n{}",
        meta_block(
            &req.title,
            &req.creators,
            req.year,
            req.publication.as_deref(),
            req.abstract_text.as_deref(),
        )
    )
}

pub fn classify_prompt(req: &ClassifyRequest) -> String {
    let paths = if req.existing_paths.is_empty() {
        "(no collections exist yet)".to_string()
    } else {
        req.existing_paths
            .iter()
            .map(|p| format!("- {}", p.join(" / ")))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let tags = if req.tags.is_empty() {
        String::new()
    } else {
        format!("Tags: {}\n", req.tags.join(", "))
    };
    format!(
        "You are organizing a researcher's paper library. Assign the paper\n\
         below to exactly one collection.\n\n\
         Existing collections (full nested paths):\n{paths}\n\n\
         Rules:\n\
         1. STRONGLY prefer an existing collection. Choose the most specific\n\
            path that genuinely fits.\n\
         2. Only when no existing collection fits, propose a new one. A new\n\
            path may extend an existing path by one level (preferred) or be a\n\
            new top-level collection. Use concise Title Case names that match\n\
            the naming style of the existing collections.\n\
         3. Never propose a path deeper than 3 levels, and never invent more\n\
            than one new level at a time.\n\
         4. Do not use \"Unclassified\" as a target.\n\n\
         Paper:\n{meta}{tags}",
        meta = meta_block(
            &req.title,
            &req.creators,
            req.year,
            req.publication.as_deref(),
            req.abstract_text.as_deref(),
        ),
    )
}

/// Enum dispatch over the concrete providers.
pub enum AnyProvider {
    Gemini(GeminiClient),
    Anthropic(AnthropicClient),
}

impl AnyProvider {
    pub fn id(&self) -> ProviderId {
        match self {
            AnyProvider::Gemini(_) => ProviderId::Gemini,
            AnyProvider::Anthropic(_) => ProviderId::Anthropic,
        }
    }

    pub async fn test_key(&self) -> Result<()> {
        match self {
            AnyProvider::Gemini(c) => c.test_key().await,
            AnyProvider::Anthropic(c) => c.test_key().await,
        }
    }

    pub async fn summarize(&self, req: &SummarizeRequest) -> Result<String> {
        match self {
            AnyProvider::Gemini(c) => c.summarize(req).await,
            AnyProvider::Anthropic(c) => c.summarize(req).await,
        }
    }

    pub async fn classify(&self, req: &ClassifyRequest) -> Result<ClassifyResponse> {
        match self {
            AnyProvider::Gemini(c) => c.classify(req).await,
            AnyProvider::Anthropic(c) => c.classify(req).await,
        }
    }
}
