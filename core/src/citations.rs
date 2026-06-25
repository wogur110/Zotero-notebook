//! Fetch a paper's citation graph (references + citing works) from OpenAlex
//! and match each entry against the user's library.
//!
//! OpenAlex is the primary source: a single work object carries the full
//! `referenced_works` list plus `cited_by_count`, and key-less batched
//! (`filter=openalex:..|..`) and `filter=cites:` queries cover the rest. The
//! whole thing is best-effort and read-only — nothing is written to Zotero.
//!
//! Base URL is injectable so the network code is tested against a mock server
//! (see `core/tests/citations.rs`), mirroring `abstract_lookup`.

use std::collections::HashMap;
use std::time::Duration;

use reqwest::Client;

use crate::models::{CitationGraph, Library, RelatedPaper};

const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const USER_AGENT: &str =
    "Zotero-Notebook/1.0 (https://github.com/wogur110/Zotero-notebook; mailto:noreply@zotero-notebook.local)";

/// Cap how many references we expand (a paper can cite hundreds) and how many
/// citing works we pull (top by citation count).
const MAX_REFERENCES: usize = 100;
const MAX_CITATIONS: usize = 50;
/// OpenAlex caps an OR filter group; expand references in chunks this size.
const ID_CHUNK: usize = 50;
const SELECT: &str = "id,doi,title,publication_year,cited_by_count";

fn build_client() -> Client {
    Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .expect("citations: failed to build HTTP client")
}

/// Fetch the citation graph for `doi` from OpenAlex. `None` on any failure
/// (DOI unknown to OpenAlex, network error, rate limit) so the caller can
/// surface "couldn't fetch" distinctly from an empty-but-successful result.
/// Returned papers have `in_library_key = None`; call [`apply_library_match`].
pub async fn fetch(openalex_base: &str, doi: &str) -> Option<CitationGraph> {
    let client = build_client();
    let doi = normalize_doi(doi);
    if doi.is_empty() {
        return None;
    }

    // 1. The work itself: referenced_works + cited_by_count + its OpenAlex id.
    let work_url = format!(
        "{}/works/https://doi.org/{}?select=id,cited_by_count,referenced_works",
        openalex_base,
        url_encode_path(&doi)
    );
    let work: serde_json::Value = client
        .get(&work_url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;

    let cited_by_count = work
        .get("cited_by_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let work_id = work
        .get("id")
        .and_then(|v| v.as_str())
        .map(short_id)
        .unwrap_or_default();

    // 2. Expand referenced_works (cap + chunk).
    let ref_ids: Vec<String> = work
        .get("referenced_works")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(short_id)
                .filter(|s| !s.is_empty())
                .take(MAX_REFERENCES)
                .collect()
        })
        .unwrap_or_default();
    let mut references = fetch_works_by_ids(&client, openalex_base, &ref_ids).await;
    references.sort_by(|a, b| b.cited_by_count.cmp(&a.cited_by_count));

    // 3. Citing works (top by citation count).
    let citations = if work_id.is_empty() {
        Vec::new()
    } else {
        fetch_citing(&client, openalex_base, &work_id).await
    };

    Some(CitationGraph {
        references,
        citations,
        cited_by_count,
        fetch_failed: false,
    })
}

/// Tag every reference/citation with the library item it matches (by
/// normalized DOI first, then normalized title). Recomputed live so a paper
/// the user just added shows up as in-library on the next view.
pub fn apply_library_match(graph: &mut CitationGraph, library: &Library) {
    let mut by_doi: HashMap<String, String> = HashMap::new();
    let mut by_title: HashMap<String, String> = HashMap::new();
    for item in &library.items {
        if let Some(doi) = item.doi.as_deref() {
            let n = normalize_doi(doi);
            if !n.is_empty() {
                by_doi.entry(n).or_insert_with(|| item.key.clone());
            }
        }
        let t = normalize_title(&item.title);
        if !t.is_empty() {
            by_title.entry(t).or_insert_with(|| item.key.clone());
        }
    }
    let match_one = |p: &RelatedPaper| -> Option<String> {
        if let Some(doi) = p.doi.as_deref() {
            let n = normalize_doi(doi);
            if let Some(k) = by_doi.get(&n) {
                return Some(k.clone());
            }
        }
        let t = normalize_title(&p.title);
        if t.is_empty() {
            return None;
        }
        by_title.get(&t).cloned()
    };
    for p in graph.references.iter_mut().chain(graph.citations.iter_mut()) {
        p.in_library_key = match_one(p);
    }
}

/// Serialize a graph for the sidecar cache (best-effort).
pub fn to_cache_json(graph: &CitationGraph) -> String {
    serde_json::to_string(graph).unwrap_or_default()
}

/// Parse a cached graph; `None` when the JSON is unreadable (schema drift).
pub fn from_cache_json(json: &str) -> Option<CitationGraph> {
    serde_json::from_str(json).ok()
}

async fn fetch_works_by_ids(
    client: &Client,
    base: &str,
    ids: &[String],
) -> Vec<RelatedPaper> {
    let mut out = Vec::new();
    for chunk in ids.chunks(ID_CHUNK) {
        let filter = format!("openalex:{}", chunk.join("|"));
        let url = format!("{base}/works");
        let json: Option<serde_json::Value> = async {
            client
                .get(&url)
                .query(&[
                    ("filter", filter.as_str()),
                    ("per-page", "50"),
                    ("select", SELECT),
                ])
                .send()
                .await
                .ok()?
                .error_for_status()
                .ok()?
                .json()
                .await
                .ok()
        }
        .await;
        if let Some(results) = json
            .as_ref()
            .and_then(|j| j.get("results"))
            .and_then(|v| v.as_array())
        {
            out.extend(results.iter().filter_map(parse_related));
        }
    }
    out
}

async fn fetch_citing(client: &Client, base: &str, work_id: &str) -> Vec<RelatedPaper> {
    let url = format!("{base}/works");
    let json: Option<serde_json::Value> = async {
        client
            .get(&url)
            .query(&[
                ("filter", format!("cites:{work_id}").as_str()),
                ("per-page", MAX_CITATIONS.to_string().as_str()),
                ("sort", "cited_by_count:desc"),
                ("select", SELECT),
            ])
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()
    }
    .await;
    json.as_ref()
        .and_then(|j| j.get("results"))
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(parse_related).collect())
        .unwrap_or_default()
}

fn parse_related(v: &serde_json::Value) -> Option<RelatedPaper> {
    let title = v.get("title").and_then(|t| t.as_str())?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let doi = v
        .get("doi")
        .and_then(|d| d.as_str())
        .map(normalize_doi)
        .filter(|d| !d.is_empty());
    let year = v
        .get("publication_year")
        .and_then(|y| y.as_i64())
        .map(|y| y as i32);
    let cited_by_count = v.get("cited_by_count").and_then(|c| c.as_i64()).unwrap_or(0);
    Some(RelatedPaper {
        title,
        doi,
        year,
        cited_by_count,
        in_library_key: None,
    })
}

/// The last path segment of an OpenAlex id URL (`.../W2741809807` → `W2741809807`).
fn short_id(id: &str) -> String {
    id.rsplit('/').next().unwrap_or("").trim().to_string()
}

/// Normalize a DOI to a bare lowercase form (strip URL prefixes), for matching.
fn normalize_doi(doi: &str) -> String {
    let d = doi.trim().to_lowercase();
    let d = d
        .strip_prefix("https://doi.org/")
        .or_else(|| d.strip_prefix("http://doi.org/"))
        .or_else(|| d.strip_prefix("doi.org/"))
        .unwrap_or(&d);
    d.trim().to_string()
}

/// Normalize a title for fuzzy-but-strict matching: lowercase, drop anything
/// that isn't a letter/number, collapse to single spaces.
fn normalize_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut prev_space = false;
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

/// Percent-encode a string for a URL path segment (keeps DOI-safe chars).
fn url_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Collection, Item};

    fn item(key: &str, title: &str, doi: Option<&str>) -> Item {
        Item {
            key: key.into(),
            title: title.into(),
            item_type: "journalArticle".into(),
            creators: vec![],
            year: None,
            publication: None,
            doi: doi.map(String::from),
            url: None,
            abstract_text: None,
            tags: vec![],
            date_added: None,
            collection_keys: vec![],
            attachment: None,
        }
    }

    #[test]
    fn normalize_doi_strips_prefixes_and_lowercases() {
        assert_eq!(normalize_doi("https://doi.org/10.1/ABC"), "10.1/abc");
        assert_eq!(normalize_doi("  10.1/AbC "), "10.1/abc");
    }

    #[test]
    fn normalize_title_drops_punctuation() {
        assert_eq!(
            normalize_title("Attention Is All You Need!"),
            "attention is all you need"
        );
        assert_eq!(
            normalize_title("Denoising  Diffusion: Probabilistic-Models"),
            "denoising diffusion probabilistic models"
        );
    }

    #[test]
    fn short_id_extracts_trailing_segment() {
        assert_eq!(short_id("https://openalex.org/W2741809807"), "W2741809807");
        assert_eq!(short_id("W123"), "W123");
    }

    #[test]
    fn library_match_by_doi_and_title() {
        let library = Library {
            collections: vec![Collection {
                key: "C".into(),
                name: "X".into(),
                parent_key: None,
            }],
            items: vec![
                item("HAVE_DOI", "Some Paper", Some("https://doi.org/10.1/XYZ")),
                item("HAVE_TITLE", "Attention Is All You Need", None),
            ],
            writable: true,
        };
        let mut graph = CitationGraph {
            references: vec![
                RelatedPaper {
                    title: "totally different".into(),
                    doi: Some("10.1/xyz".into()), // matches HAVE_DOI by normalized DOI
                    year: None,
                    cited_by_count: 5,
                    in_library_key: None,
                },
                RelatedPaper {
                    title: "attention is all you need".into(), // matches HAVE_TITLE
                    doi: None,
                    year: None,
                    cited_by_count: 99,
                    in_library_key: None,
                },
                RelatedPaper {
                    title: "Not in library".into(),
                    doi: Some("10.9/none".into()),
                    year: None,
                    cited_by_count: 1,
                    in_library_key: None,
                },
            ],
            citations: vec![],
            cited_by_count: 10,
            fetch_failed: false,
        };
        apply_library_match(&mut graph, &library);
        assert_eq!(graph.references[0].in_library_key.as_deref(), Some("HAVE_DOI"));
        assert_eq!(
            graph.references[1].in_library_key.as_deref(),
            Some("HAVE_TITLE")
        );
        assert_eq!(graph.references[2].in_library_key, None);
    }
}
