//! Shared data model. These types are the contract between the core crate,
//! the Tauri commands, and the frontend (`app/src/types.ts` mirrors them).
//! All serialization is camelCase so the frontend sees idiomatic JSON.

use serde::{Deserialize, Serialize};

/// Connection state of the local Zotero instance, polled by the app.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZoteroStatus {
    /// Something is listening on the Zotero port.
    pub running: bool,
    /// The companion plugin answered `/zotero-notebook/ping`.
    pub plugin_installed: bool,
    pub plugin_version: Option<String>,
    /// Human-readable hint when degraded (e.g. how to enable the local API
    /// or install the plugin).
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub key: String,
    pub name: String,
    pub parent_key: Option<String>,
}

/// How the attachment file is stored in Zotero.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkMode {
    /// File lives in Zotero's own storage directory.
    ImportedFile,
    ImportedUrl,
    /// File lives outside Zotero (e.g. managed by ZotMoov); the item stores
    /// a path. This is the mode the move pipeline operates on.
    LinkedFile,
    LinkedUrl,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub key: String,
    pub title: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub link_mode: LinkMode,
    /// Absolute path on disk, already resolved (the plugin resolves
    /// `attachments:`-relative paths and the storage directory for us).
    /// `None` when the file is missing or unresolvable.
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub key: String,
    pub title: String,
    pub item_type: String,
    /// Display names, first author first.
    pub creators: Vec<String>,
    pub year: Option<i32>,
    /// Journal / conference / publisher, whichever the item type carries.
    pub publication: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub abstract_text: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub date_added: Option<String>,
    /// Keys of every collection this item belongs to (empty = unfiled).
    #[serde(default)]
    pub collection_keys: Vec<String>,
    /// The primary PDF attachment, if any.
    pub attachment: Option<Attachment>,
}

/// One snapshot of the whole library, as served by the plugin (or, in
/// degraded mode, assembled from the read-only local API).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub collections: Vec<Collection>,
    pub items: Vec<Item>,
    /// True when served by the plugin (write operations available).
    /// Not part of the plugin wire format — set by the client.
    #[serde(default)]
    pub writable: bool,
}

impl Library {
    /// Nested path (root → leaf) for a collection key, e.g.
    /// `["Computer Vision", "Diffusion Models"]`.
    pub fn collection_path(&self, key: &str) -> Option<Vec<String>> {
        let mut path = Vec::new();
        let mut cursor = Some(key.to_string());
        let mut guard = 0;
        while let Some(k) = cursor {
            guard += 1;
            if guard > 64 {
                return None; // cycle guard
            }
            let col = self.collections.iter().find(|c| c.key == k)?;
            path.push(col.name.clone());
            cursor = col.parent_key.clone();
        }
        path.reverse();
        Some(path)
    }

    /// Every collection as a nested path, for the classifier prompt.
    pub fn all_paths(&self) -> Vec<Vec<String>> {
        self.collections
            .iter()
            .filter_map(|c| self.collection_path(&c.key))
            .collect()
    }
}

/// Name of the collection that holds not-yet-classified papers. Items with
/// no collection at all are treated as unclassified too.
pub const UNCLASSIFIED_COLLECTION: &str = "Unclassified";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Gemini,
    Anthropic,
}

impl ProviderId {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::Gemini => "gemini",
            ProviderId::Anthropic => "anthropic",
        }
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the LLM proposes for one unclassified item. Nothing moves until the
/// user approves a `ClassificationDecision`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationProposal {
    pub item_key: String,
    /// Target collection as a nested path (root → leaf).
    pub proposed_path: Vec<String>,
    /// True when no existing collection matched and a new one is proposed.
    pub is_new_collection: bool,
    /// 0.0–1.0
    pub confidence: f64,
    pub rationale: String,
}

/// A user-approved (possibly edited) move.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationDecision {
    pub item_key: String,
    pub target_path: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MoveResult {
    pub item_key: String,
    pub ok: bool,
    pub error: Option<String>,
    pub collection_key: Option<String>,
    pub new_file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredSummary {
    pub item_key: String,
    pub summary: String,
    pub provider: String,
    pub model: String,
    /// RFC 3339.
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub default_provider: ProviderId,
    pub gemini_model: String,
    pub anthropic_model: String,
    /// Base URL of the local Zotero server. Configurable so tests can point
    /// at a mock and WSL2 setups can point at the Windows host.
    pub zotero_base_url: String,
    /// Root directory for linked PDF files (the ZotMoov destination folder).
    /// When set, approved moves relocate files to
    /// `<file_root>/<collection path>/<filename>`.
    pub file_root: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            default_provider: ProviderId::Gemini,
            gemini_model: "gemini-2.5-pro".into(),
            anthropic_model: "claude-opus-4-8".into(),
            zotero_base_url: "http://127.0.0.1:23119".into(),
            file_root: None,
        }
    }
}

/// Progress event payload shared by the classify and apply pipelines.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub done: usize,
    pub total: usize,
    pub item_key: Option<String>,
    /// "running" | "ok" | "error"
    pub state: String,
    pub message: Option<String>,
}
