//! Client for the Zotero Notebook companion plugin endpoints.
//! Wire format: docs/PLUGIN_API.md (the single source of truth).

use std::time::Duration;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::models::{Library, MoveResult, ZoteroStatus};
use crate::zotero::local_api;

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

impl PluginClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client");
        PluginClient { base_url, http }
    }

    fn offline(&self, e: reqwest::Error) -> Error {
        if e.is_connect() || e.is_timeout() {
            Error::ZoteroOffline(self.base_url.clone())
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
