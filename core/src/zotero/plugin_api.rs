//! Client for the Zotero Notebook companion plugin endpoints.
//! Wire format: docs/PLUGIN_API.md (the single source of truth).

use std::time::Duration;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::models::{Library, MoveResult, ZoteroStatus};
use crate::zotero::local_api;

/// Lowest plugin version (major, minor) the app is willing to WRITE through.
/// Bump together with breaking changes to docs/PLUGIN_API.md.
pub const EXPECTED_PLUGIN_VERSION: (u32, u32) = (0, 1);

/// True when `version` ("major.minor.patch") is at least
/// `EXPECTED_PLUGIN_VERSION`. Unparsable versions are incompatible.
pub fn plugin_version_compatible(version: &str) -> bool {
    let mut parts = version.trim().split('.');
    let major: u32 = match parts.next().and_then(|p| p.parse().ok()) {
        Some(v) => v,
        None => return false,
    };
    let minor: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    (major, minor) >= EXPECTED_PLUGIN_VERSION
}

pub struct PluginClient {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct PingResponse {
    version: String,
    #[serde(rename = "zoteroVersion")]
    zotero_version: String,
}

#[derive(Deserialize)]
struct MoveResponse {
    ok: bool,
    #[serde(rename = "collectionKey")]
    collection_key: Option<String>,
    #[serde(rename = "newFilePath")]
    new_file_path: Option<String>,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

/// Extracted PDF text served by `/zotero-notebook/fulltext`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Fulltext {
    pub text: String,
    /// Total extracted length before truncation.
    pub chars: usize,
    pub truncated: bool,
}

/// Result of an additive write-back (`/zotero-notebook/update-item`).
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateItemResult {
    pub ok: bool,
    #[serde(default)]
    pub wrote_abstract: bool,
    #[serde(default)]
    pub added_tags: Vec<String>,
    #[serde(default)]
    pub note_key: Option<String>,
}

#[derive(Deserialize)]
struct FulltextResponse {
    text: Option<String>,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    chars: usize,
}

impl PluginClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        // Generous total timeout: /library resolves a file path per item and
        // /move-item moves PDFs on disk — both can be slow on big libraries
        // or network drives. Connect stays tight so "Zotero not running" is
        // detected quickly.
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(120))
            .build()
            .expect("reqwest client");
        PluginClient { base_url, http }
    }

    fn offline(&self, e: reqwest::Error) -> Error {
        // Only connect-phase failures mean Zotero is not running. A read
        // timeout means Zotero IS there but slow — misreporting it as
        // offline would tell the user to "start Zotero" for no reason.
        if e.is_connect() {
            Error::ZoteroOffline(self.base_url.clone())
        } else if e.is_timeout() {
            Error::Other(format!(
                "the request to Zotero timed out ({e}) — the library may be very large or the disk slow; try again"
            ))
        } else {
            Error::Http(e)
        }
    }

    /// Returns (plugin version, zotero version).
    pub async fn ping(&self) -> Result<(String, String)> {
        let url = format!("{}/zotero-notebook/ping", self.base_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| self.offline(e))?;
        if !resp.status().is_success() {
            return Err(Error::PluginMissing);
        }
        let body: PingResponse = resp
            .json()
            .await
            .map_err(|_| Error::PluginMissing)?;
        Ok((body.version, body.zotero_version))
    }

    pub async fn fetch_library(&self) -> Result<Library> {
        let url = format!("{}/zotero-notebook/library", self.base_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| self.offline(e))?;
        let status = resp.status();
        let text = resp.text().await.map_err(Error::Http)?;
        if !status.is_success() {
            return Err(self.classify_error(status.as_u16(), &text));
        }
        let mut library: Library = serde_json::from_str(&text)
            .map_err(|e| Error::InvalidResponse(format!("library payload: {e}")))?;
        library.writable = true;
        Ok(library)
    }

    /// The extracted text of the item's primary PDF, from Zotero's own
    /// full-text cache. `Ok(None)` covers every benign "no text" case —
    /// plugin too old for the route, no PDF, scanned PDF without OCR — so
    /// callers can treat full text as strictly optional.
    pub async fn fetch_fulltext(
        &self,
        item_key: &str,
        max_chars: usize,
    ) -> Result<Option<Fulltext>> {
        let url = format!(
            "{}/zotero-notebook/fulltext?itemKey={}&maxChars={}",
            self.base_url,
            urlencoding::encode(item_key),
            max_chars
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| self.offline(e))?;
        let status = resp.status();
        let text = resp.text().await.map_err(Error::Http)?;
        if !status.is_success() {
            // A plugin without this route (pre-1.2) answers 404 with a
            // non-JSON body; treat as "no full text" rather than an error.
            return match self.classify_error(status.as_u16(), &text) {
                Error::PluginMissing => Ok(None),
                e => Err(e),
            };
        }
        let parsed: FulltextResponse = serde_json::from_str(&text)
            .map_err(|e| Error::InvalidResponse(format!("fulltext payload: {e}")))?;
        Ok(parsed
            .text
            .filter(|t| !t.trim().is_empty())
            .map(|t| Fulltext {
                text: t,
                chars: parsed.chars,
                truncated: parsed.truncated,
            }))
    }

    /// Additive write-back: fill an empty abstract, add tags, and/or upsert
    /// the AI-summary child note. Never overwrites user data (see
    /// docs/PLUGIN_API.md). Errors map like move_item; a pre-1.3 plugin
    /// without the route surfaces as `Error::PluginMissing`.
    pub async fn update_item(
        &self,
        item_key: &str,
        abstract_if_empty: Option<&str>,
        add_tags: &[String],
        summary_note_html: Option<&str>,
    ) -> Result<UpdateItemResult> {
        let url = format!("{}/zotero-notebook/update-item", self.base_url);
        let body = serde_json::json!({
            "itemKey": item_key,
            "abstractIfEmpty": abstract_if_empty,
            "addTags": add_tags,
            "summaryNoteHtml": summary_note_html,
        });
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| self.offline(e))?;
        let status = resp.status();
        let text = resp.text().await.map_err(Error::Http)?;
        if !status.is_success() {
            return Err(self.classify_error(status.as_u16(), &text));
        }
        serde_json::from_str(&text)
            .map_err(|e| Error::InvalidResponse(format!("update-item payload: {e}")))
    }

    pub async fn move_item(
        &self,
        item_key: &str,
        target_path: &[String],
        remove_from: &[String],
        file_root: Option<&str>,
    ) -> Result<MoveResult> {
        let url = format!("{}/zotero-notebook/move-item", self.base_url);
        let body = serde_json::json!({
            "itemKey": item_key,
            "targetPath": target_path,
            "removeFromCollections": remove_from,
            "fileRoot": file_root,
        });
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| self.offline(e))?;
        let status = resp.status();
        let text = resp.text().await.map_err(Error::Http)?;
        if !status.is_success() {
            return Err(self.classify_error(status.as_u16(), &text));
        }
        let parsed: MoveResponse = serde_json::from_str(&text)
            .map_err(|e| Error::InvalidResponse(format!("move-item payload: {e}")))?;
        Ok(MoveResult {
            item_key: item_key.to_string(),
            ok: parsed.ok,
            error: None,
            collection_key: parsed.collection_key,
            new_file_path: parsed.new_file_path,
        })
    }

    /// Distinguish "the plugin answered with an error" (JSON `{"error"}`)
    /// from "Zotero answered but the plugin route does not exist" (HTML or
    /// plain-text body from Zotero's own server).
    fn classify_error(&self, status: u16, body: &str) -> Error {
        match serde_json::from_str::<ErrorBody>(body) {
            Ok(e) => Error::ZoteroRejected {
                status,
                message: e.error,
            },
            Err(_) => {
                if status == 404 {
                    Error::PluginMissing
                } else {
                    Error::ZoteroRejected {
                        status,
                        message: truncate(body, 200),
                    }
                }
            }
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::plugin_version_compatible;

    #[test]
    fn version_compatibility() {
        assert!(plugin_version_compatible("0.1.0"));
        assert!(plugin_version_compatible("0.1"));
        assert!(plugin_version_compatible("0.2.0"));
        assert!(plugin_version_compatible("1.0.0"));
        assert!(!plugin_version_compatible("0.0.9"));
        assert!(!plugin_version_compatible(""));
        assert!(!plugin_version_compatible("garbage"));
    }
}

/// Composite status probe used by `get_status` and the background watcher.
/// Never errors — every failure mode maps to a status with a hint.
pub async fn check_status(base_url: &str) -> ZoteroStatus {
    let client = PluginClient::new(base_url);
    match client.ping().await {
        Ok((plugin_version, _zotero_version)) => ZoteroStatus {
            running: true,
            plugin_installed: true,
            plugin_version: Some(plugin_version),
            hint: None,
        },
        Err(Error::ZoteroOffline(_)) => ZoteroStatus {
            running: false,
            plugin_installed: false,
            plugin_version: None,
            hint: Some("Start Zotero to connect.".into()),
        },
        Err(_) => {
            // Something answered on the port but not our plugin.
            match local_api::ping(base_url).await {
                Ok(()) => ZoteroStatus {
                    running: true,
                    plugin_installed: false,
                    plugin_version: None,
                    hint: Some(
                        "Zotero is running, but the Zotero Notebook plugin is not installed. \
                         Install it from Settings to enable AI classification and file moves."
                            .into(),
                    ),
                },
                Err(_) => ZoteroStatus {
                    running: false,
                    plugin_installed: false,
                    plugin_version: None,
                    hint: Some("Start Zotero to connect.".into()),
                },
            }
        }
    }
}
