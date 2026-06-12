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
use crate::llm::{
    anthropic::AnthropicClient, gemini::GeminiClient, openai_compat::OpenAiCompatClient,
};
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
    /// Extracted PDF text (possibly truncated). Only set for the explicit
    /// "full-text summary" action — the default summary stays cheap.
    #[serde(default)]
    pub body_excerpt: Option<String>,
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
    /// The library's most-used tags, advertised so suggestions reuse the
    /// established vocabulary instead of inventing near-duplicates.
    #[serde(default)]
    pub existing_tags: Vec<String>,
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
    /// 2–4 suggested tags (existing vocabulary preferred). Default for
    /// lenient parsing when a model omits the field.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Audit of an already-classified paper: is its current filing appropriate?
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditRequest {
    pub title: String,
    pub creators: Vec<String>,
    pub year: Option<i32>,
    pub publication: Option<String>,
    pub abstract_text: Option<String>,
    pub tags: Vec<String>,
    /// Collections the paper currently belongs to (nested paths, root →
    /// leaf; the Unclassified tree is excluded by the caller).
    pub current_paths: Vec<Vec<String>>,
    /// Every existing collection path (Unclassified excluded).
    pub existing_paths: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditResponse {
    /// True only when NO current collection is a reasonable home.
    pub misplaced: bool,
    /// Proposed path when misplaced; empty otherwise.
    pub path: Vec<String>,
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
            "rationale": { "type": "string" },
            "tags": {
                "type": "array",
                "items": { "type": "string" },
                "description": "2-4 suggested tags, preferring the library's existing tags"
            }
        },
        "required": ["path", "is_new", "confidence", "rationale", "tags"],
        "additionalProperties": false
    })
}

/// JSON Schema for `AuditResponse`, shared by both clients.
pub fn audit_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "misplaced": {
                "type": "boolean",
                "description": "True only when no current collection fits the paper"
            },
            "path": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Proposed collection path, root to leaf; empty when misplaced is false"
            },
            "confidence": { "type": "number" },
            "rationale": { "type": "string" }
        },
        "required": ["misplaced", "path", "confidence", "rationale"],
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
    let meta = meta_block(
        &req.title,
        &req.creators,
        req.year,
        req.publication.as_deref(),
        req.abstract_text.as_deref(),
    );
    match req.body_excerpt.as_deref().filter(|b| !b.trim().is_empty()) {
        Some(body) => format!(
            "You are helping a researcher maintain notes on academic papers.\n\
             Write a summary of the following paper in English, 5 to 8 sentences,\n\
             as a single plain-text paragraph (no markdown, no headings, no lists).\n\
             Cover: the problem addressed, the key idea or method, the main\n\
             experimental results (with concrete numbers when the text provides\n\
             them), and the significance or limitations. Base the summary on the\n\
             metadata and the full-text excerpt below; do not invent anything\n\
             that is not present.\n\n{meta}\n\
             Full text (may be truncated):\n\"\"\"\n{body}\n\"\"\"\n"
        ),
        None => format!(
            "You are helping a researcher maintain notes on academic papers.\n\
             Write a summary of the following paper in English, 5 to 8 sentences,\n\
             as a single plain-text paragraph (no markdown, no headings, no lists).\n\
             Cover: the problem addressed, the key idea or method, and the main\n\
             results or significance. Base the summary only on the metadata below;\n\
             do not invent specific numbers that are not present.\n\n{meta}"
        ),
    }
}

/// System prompt for the per-paper "Ask AI" chat. Answers are ALWAYS in
/// English (user decision), grounded in the provided material.
pub fn chat_system_prompt(
    title: &str,
    creators: &[String],
    year: Option<i32>,
    publication: Option<&str>,
    abstract_text: Option<&str>,
    body_excerpt: Option<&str>,
) -> String {
    let meta = meta_block(title, creators, year, publication, abstract_text);
    let body = match body_excerpt.filter(|b| !b.trim().is_empty()) {
        Some(b) => format!("Full text (may be truncated):\n\"\"\"\n{b}\n\"\"\"\n"),
        None => "Full text: (not available — answer from the metadata above and say so \
                 when a question needs the body of the paper)\n"
            .to_string(),
    };
    format!(
        "You are helping a researcher understand one specific academic paper.\n\
         Answer questions about it using the material below.\n\n\
         Rules:\n\
         - Always answer in English, regardless of the language of the question.\n\
         - Ground every claim in the provided text; when the answer is not in\n\
           the material, say so plainly instead of guessing.\n\
         - Be concise: short paragraphs, no headings or bullet lists unless\n\
           the user asks for them.\n\n\
         {meta}\n{body}"
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
    let tag_vocab = if req.existing_tags.is_empty() {
        "(no tags exist yet)".to_string()
    } else {
        req.existing_tags.join(", ")
    };
    format!(
        "You are organizing a researcher's paper library. Assign the paper\n\
         below to exactly one collection, and suggest tags for it.\n\n\
         Existing collections (full nested paths):\n{paths}\n\n\
         Collection rules:\n\
         1. STRONGLY prefer an existing collection. Choose the most specific\n\
            path that genuinely fits.\n\
         2. Only when no existing collection fits, propose a new one. A new\n\
            path may extend an existing path by one level (preferred) or be a\n\
            new top-level collection. Use concise Title Case names that match\n\
            the naming style of the existing collections.\n\
         3. Never propose a path deeper than 3 levels, and never invent more\n\
            than one new level at a time.\n\
         4. Do not use \"Unclassified\" as a target.\n\n\
         Tag rules:\n\
         5. Suggest 2 to 4 topical tags for the paper.\n\
         6. STRONGLY prefer reusing tags from the library's existing\n\
            vocabulary below (exact spelling). Propose a new tag only when\n\
            no existing tag describes the topic; new tags should be short,\n\
            lowercase, and specific. Do not repeat tags the paper already\n\
            has.\n\n\
         Existing tags in the library:\n{tag_vocab}\n\n\
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

pub fn audit_prompt(req: &AuditRequest) -> String {
    let format_paths = |paths: &[Vec<String>], empty: &str| {
        if paths.is_empty() {
            empty.to_string()
        } else {
            paths
                .iter()
                .map(|p| format!("- {}", p.join(" / ")))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };
    let existing = format_paths(&req.existing_paths, "(no collections exist yet)");
    let current = format_paths(&req.current_paths, "(none)");
    let tags = if req.tags.is_empty() {
        String::new()
    } else {
        format!("Tags: {}\n", req.tags.join(", "))
    };
    format!(
        "You are auditing how a researcher's paper library is filed. Decide\n\
         whether the paper below is appropriately filed where it is.\n\n\
         Existing collections (full nested paths):\n{existing}\n\n\
         The paper is currently filed in:\n{current}\n\n\
         Rules:\n\
         1. Be conservative — reorganizing has a cost. If ANY current\n\
            collection is a reasonable home for this paper, answer\n\
            misplaced=false. Do not flag borderline cases or stylistic\n\
            preferences.\n\
         2. Answer misplaced=true ONLY when no current collection fits the\n\
            paper's topic at all (e.g. an NLP paper filed under Hardware).\n\
         3. When misplaced=true, propose the best path: STRONGLY prefer an\n\
            existing collection; only when nothing fits, propose a new path\n\
            (max 3 levels, extending an existing path by at most one new\n\
            level). Never propose \"Unclassified\".\n\
         4. When misplaced=false, return an empty path array.\n\n\
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
    Local(OpenAiCompatClient),
}

impl AnyProvider {
    pub fn id(&self) -> ProviderId {
        match self {
            AnyProvider::Gemini(_) => ProviderId::Gemini,
            AnyProvider::Anthropic(_) => ProviderId::Anthropic,
            AnyProvider::Local(_) => ProviderId::Local,
        }
    }

    pub async fn test_key(&self) -> Result<()> {
        match self {
            AnyProvider::Gemini(c) => c.test_key().await,
            AnyProvider::Anthropic(c) => c.test_key().await,
            AnyProvider::Local(c) => c.test_key().await,
        }
    }

    pub async fn summarize(&self, req: &SummarizeRequest) -> Result<String> {
        match self {
            AnyProvider::Gemini(c) => c.summarize(req).await,
            AnyProvider::Anthropic(c) => c.summarize(req).await,
            AnyProvider::Local(c) => c.summarize(req).await,
        }
    }

    pub async fn classify(&self, req: &ClassifyRequest) -> Result<ClassifyResponse> {
        match self {
            AnyProvider::Gemini(c) => c.classify(req).await,
            AnyProvider::Anthropic(c) => c.classify(req).await,
            AnyProvider::Local(c) => c.classify(req).await,
        }
    }

    pub async fn audit(&self, req: &AuditRequest) -> Result<AuditResponse> {
        match self {
            AnyProvider::Gemini(c) => c.audit(req).await,
            AnyProvider::Anthropic(c) => c.audit(req).await,
            AnyProvider::Local(c) => c.audit(req).await,
        }
    }

    /// Streamed chat about one paper. `on_delta` is invoked with each text
    /// fragment as it arrives; the full answer is returned at the end.
    pub async fn chat_stream<F: FnMut(&str)>(
        &self,
        system: &str,
        messages: &[crate::models::ChatMessage],
        on_delta: &mut F,
    ) -> Result<String> {
        match self {
            AnyProvider::Gemini(c) => c.chat_stream(system, messages, on_delta).await,
            AnyProvider::Anthropic(c) => c.chat_stream(system, messages, on_delta).await,
            AnyProvider::Local(c) => c.chat_stream(system, messages, on_delta).await,
        }
    }
}
