//! Read-only fallback client for Zotero 7's built-in local API.
//!
//! Used when the companion plugin is not installed, so the user can at least
//! browse the library. Quirks (learned the hard way in the previous app):
//! - the usable base path is `/api/users/0`; bare `/api/collections` 404s;
//! - every request needs the `Zotero-Allowed-Request: 1` header;
//! - a 403 means "Allow other applications…" is disabled in Zotero settings;
//! - linked-file attachments may carry an `attachments:` relative path that
//!   cannot be resolved without knowing the base directory — those become
//!   `file_path: None` here (the plugin path resolves them properly).

use std::time::Duration;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::models::{Attachment, Collection, Item, Library, LinkMode};

const ALLOWED_HEADER: (&str, &str) = ("Zotero-Allowed-Request", "1");
const PAGE: usize = 100;

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(60))
        .build()
        .expect("reqwest client")
}

fn map_send_err(base_url: &str, e: reqwest::Error) -> Error {
    if e.is_connect() || e.is_timeout() {
        Error::ZoteroOffline(base_url.to_string())
    } else {
        Error::Http(e)
    }
}

fn check_forbidden(status: reqwest::StatusCode) -> Result<()> {
    if status.as_u16() == 403 {
        return Err(Error::ZoteroRejected {
            status: 403,
            message: "Zotero rejected the request. Enable \"Allow other applications on this \
                      computer to communicate with Zotero\" in Zotero Settings → Advanced."
                .into(),
        });
    }
    Ok(())
}

pub async fn ping(base_url: &str) -> Result<()> {
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/connector/ping");
    let resp = http()
        .get(&url)
        .header(ALLOWED_HEADER.0, ALLOWED_HEADER.1)
        .send()
        .await
        .map_err(|e| map_send_err(base_url, e))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(Error::ZoteroRejected {
            status: resp.status().as_u16(),
            message: "unexpected response from Zotero connector ping".into(),
        })
    }
}

// --- wire shapes ------------------------------------------------------

#[derive(Deserialize)]
struct ApiCollection {
    key: String,
    data: ApiCollectionData,
}

#[derive(Deserialize)]
struct ApiCollectionData {
    name: String,
    /// `false` (bool) when top-level, otherwise the parent key (string).
    #[serde(rename = "parentCollection", default)]
    parent_collection: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ApiItem {
    key: String,
    data: serde_json::Value,
}

fn fetch_str(v: &serde_json::Value, field: &str) -> Option<String> {
    v.get(field)
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn year_from_date(date: &str) -> Option<i32> {
    let bytes = date.as_bytes();
    for i in 0..bytes.len().saturating_sub(3) {
        let window = &date[i..i + 4];
        if window.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(y) = window.parse::<i32>() {
                if (1000..=2999).contains(&y) {
                    return Some(y);
                }
            }
        }
    }
    None
}

async fn get_paginated<T: serde::de::DeserializeOwned>(
    base_url: &str,
    path: &str,
) -> Result<Vec<T>> {
    let base = base_url.trim_end_matches('/');
    let client = http();
    let mut out: Vec<T> = Vec::new();
    let mut start = 0usize;
    loop {
        let sep = if path.contains('?') { '&' } else { '?' };
        let url = format!("{base}/api/users/0/{path}{sep}limit={PAGE}&start={start}");
        let resp = client
            .get(&url)
            .header(ALLOWED_HEADER.0, ALLOWED_HEADER.1)
            .send()
            .await
            .map_err(|e| map_send_err(base_url, e))?;
        check_forbidden(resp.status())?;
        if !resp.status().is_success() {
            return Err(Error::ZoteroRejected {
                status: resp.status().as_u16(),
                message: format!("GET {path} failed"),
            });
        }
        let page: Vec<T> = resp
            .json()
            .await
            .map_err(|e| Error::InvalidResponse(format!("local API {path}: {e}")))?;
        let n = page.len();
        out.extend(page);
        if n < PAGE {
            return Ok(out);
        }
        start += n;
    }
}

fn parse_link_mode(s: Option<&str>) -> LinkMode {
    match s {
        Some("imported_file") => LinkMode::ImportedFile,
        Some("imported_url") => LinkMode::ImportedUrl,
        Some("linked_file") => LinkMode::LinkedFile,
        Some("linked_url") => LinkMode::LinkedUrl,
        _ => LinkMode::Other,
    }
}

fn item_from_data(key: &str, data: &serde_json::Value) -> Item {
    let creators = data
        .get("creators")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    if let Some(name) = c.get("name").and_then(|x| x.as_str()) {
                        Some(name.trim().to_string())
                    } else {
                        let first = c.get("firstName").and_then(|x| x.as_str()).unwrap_or("");
                        let last = c.get("lastName").and_then(|x| x.as_str()).unwrap_or("");
                        let full = format!("{} {}", first.trim(), last.trim());
                        let full = full.trim().to_string();
                        if full.is_empty() {
                            None
                        } else {
                            Some(full)
                        }
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let publication = fetch_str(data, "publicationTitle")
        .or_else(|| fetch_str(data, "proceedingsTitle"))
        .or_else(|| fetch_str(data, "conferenceName"))
        .or_else(|| fetch_str(data, "publisher"));

    let tags = data
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.get("tag").and_then(|x| x.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let collection_keys = data
        .get("collections")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|k| k.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Item {
        key: key.to_string(),
        title: fetch_str(data, "title").unwrap_or_else(|| "(untitled)".into()),
        item_type: fetch_str(data, "itemType").unwrap_or_else(|| "document".into()),
        creators,
        year: fetch_str(data, "date").as_deref().and_then(year_from_date),
        publication,
        doi: fetch_str(data, "DOI"),
        url: fetch_str(data, "url"),
        abstract_text: fetch_str(data, "abstractNote"),
        tags,
        date_added: fetch_str(data, "dateAdded"),
        collection_keys,
        attachment: None,
    }
}

/// Read-only library snapshot (`writable: false`).
pub async fn fetch_library(base_url: &str) -> Result<Library> {
    let raw_collections: Vec<ApiCollection> = get_paginated(base_url, "collections").await?;
    let collections = raw_collections
        .into_iter()
        .map(|c| {
            let parent_key = match c.data.parent_collection {
                Some(serde_json::Value::String(s)) if !s.is_empty() => Some(s),
                _ => None,
            };
            Collection {
                key: c.key,
                name: c.data.name,
                parent_key,
            }
        })
        .collect::<Vec<_>>();

    let raw_items: Vec<ApiItem> = get_paginated(base_url, "items/top").await?;
    let mut items = Vec::new();
    for raw in raw_items {
        let item_type = raw
            .data
            .get("itemType")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        if matches!(item_type, "attachment" | "note" | "annotation") {
            continue;
        }
        let mut item = item_from_data(&raw.key, &raw.data);
        item.attachment = fetch_pdf_child(base_url, &raw.key).await.ok().flatten();
        items.push(item);
    }

    Ok(Library {
        collections,
        items,
        writable: false,
    })
}

async fn fetch_pdf_child(base_url: &str, item_key: &str) -> Result<Option<Attachment>> {
    let children: Vec<ApiItem> =
        get_paginated(base_url, &format!("items/{item_key}/children")).await?;
    for child in children {
        let data = &child.data;
        let content_type = fetch_str(data, "contentType");
        if content_type.as_deref() != Some("application/pdf") {
            continue;
        }
        let link_mode = parse_link_mode(data.get("linkMode").and_then(|x| x.as_str()));
        // Only absolute linked-file paths are usable here; the
        // `attachments:`-relative form needs the base dir (plugin resolves it).
        let file_path = match link_mode {
            LinkMode::LinkedFile => fetch_str(data, "path")
                .filter(|p| !p.starts_with("attachments:")),
            _ => None,
        };
        return Ok(Some(Attachment {
            key: child.key,
            title: fetch_str(data, "title").unwrap_or_else(|| "PDF".into()),
            filename: fetch_str(data, "filename"),
            content_type,
            link_mode,
            file_path,
        }));
    }
    Ok(None)
}
