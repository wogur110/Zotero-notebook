//! Fetch a paper abstract from public scholarly-metadata APIs.
//!
//! Many Zotero items arrive without an abstract, which starves the AI
//! summary/classification prompts. Before calling the LLM we try to fill
//! the gap from three free, key-less backends:
//!
//!   1. **Crossref**         (`{crossref}/works/{doi}`)
//!   2. **Semantic Scholar** (`{s2}/graph/v1/paper/DOI:{doi}`, title search fallback)
//!   3. **OpenAlex**         (`{openalex}/works/https://doi.org/{doi}`)
//!
//! Everything is best-effort: any single backend failure is silent, and the
//! caller receives `None` only when every backend came up empty. Base URLs
//! are injectable so the whole chain is tested against mock servers.

use std::time::Duration;

use reqwest::Client;

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Polite User-Agent per Crossref's etiquette guidelines. The mailto is a
/// placeholder — Crossref only uses it to contact heavy users.
const USER_AGENT: &str =
    "Zotero-Notebook/1.0 (https://github.com/wogur110/Zotero-notebook; mailto:noreply@zotero-notebook.local)";

/// Backend base URLs. `Default` points at the public services; tests point
/// every field at a wiremock server.
#[derive(Debug, Clone)]
pub struct Sources {
    pub crossref: String,
    pub semantic_scholar: String,
    pub openalex: String,
}

impl Default for Sources {
    fn default() -> Self {
        Sources {
            crossref: "https://api.crossref.org".into(),
            semantic_scholar: "https://api.semanticscholar.org".into(),
            openalex: "https://api.openalex.org".into(),
        }
    }
}

fn build_client() -> Client {
    Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .expect("abstract_lookup: failed to build HTTP client")
}

// ── Public API ──────────────────────────────────────────────────────────

/// Best-effort abstract lookup: DOI-based chain first (Crossref → Semantic
/// Scholar → OpenAlex), then a title search on Semantic Scholar when there
/// is no DOI or the DOI chain found nothing.
pub async fn lookup(sources: &Sources, doi: Option<&str>, title: &str) -> Option<String> {
    let client = build_client();

    if let Some(doi) = doi.map(str::trim).filter(|d| !d.is_empty()) {
        if let Some(a) = fetch_crossref(&client, sources, doi).await {
            return Some(a);
        }
        if let Some(a) = fetch_semantic_scholar_by_doi(&client, sources, doi).await {
            return Some(a);
        }
        if let Some(a) = fetch_openalex_by_doi(&client, sources, doi).await {
            return Some(a);
        }
    }

    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    fetch_semantic_scholar_by_title(&client, sources, title).await
}

// ── Backend: Crossref ───────────────────────────────────────────────────

async fn fetch_crossref(client: &Client, sources: &Sources, doi: &str) -> Option<String> {
    let url = format!("{}/works/{}", sources.crossref, url_encode_path(doi));
    let json: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let raw = json.get("message")?.get("abstract")?.as_str()?;
    non_empty(normalise_whitespace(&strip_html_tags(raw)))
}

// ── Backend: Semantic Scholar ───────────────────────────────────────────

async fn fetch_semantic_scholar_by_doi(
    client: &Client,
    sources: &Sources,
    doi: &str,
) -> Option<String> {
    let url = format!(
        "{}/graph/v1/paper/DOI:{}?fields=abstract",
        sources.semantic_scholar,
        url_encode_path(doi)
    );
    let json: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    non_empty(normalise_whitespace(json.get("abstract")?.as_str()?))
}

async fn fetch_semantic_scholar_by_title(
    client: &Client,
    sources: &Sources,
    title: &str,
) -> Option<String> {
    let url = format!("{}/graph/v1/paper/search", sources.semantic_scholar);
    let json: serde_json::Value = client
        .get(&url)
        .query(&[
            ("query", title),
            ("limit", "1"),
            ("fields", "abstract,title"),
        ])
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let raw = json
        .get("data")?
        .as_array()?
        .first()?
        .get("abstract")?
        .as_str()?;
    non_empty(normalise_whitespace(raw))
}

// ── Backend: OpenAlex ───────────────────────────────────────────────────

async fn fetch_openalex_by_doi(client: &Client, sources: &Sources, doi: &str) -> Option<String> {
    let url = format!(
        "{}/works/https://doi.org/{}",
        sources.openalex,
        url_encode_path(doi)
    );
    let json: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let inverted = json.get("abstract_inverted_index")?.as_object()?;
    non_empty(normalise_whitespace(&reconstruct_inverted_index(inverted)))
}

// ── Text helpers ────────────────────────────────────────────────────────

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// OpenAlex stores abstracts as `{word: [positions]}` so the text doesn't
/// trip Elasticsearch copyright filters. Reconstruct word order from the
/// position arrays.
fn reconstruct_inverted_index(inverted: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut positions: Vec<(usize, &str)> = Vec::new();
    for (word, locs) in inverted {
        if let Some(arr) = locs.as_array() {
            for pos in arr {
                if let Some(p) = pos.as_u64() {
                    positions.push((p as usize, word.as_str()));
                }
            }
        }
    }
    positions.sort_by_key(|(p, _)| *p);
    positions
        .into_iter()
        .map(|(_, w)| w)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Strip JATS / HTML tags without a full HTML parser. Anything between `<`
/// and `>` is removed; the brackets become spaces so adjacent words do not
/// get glued together.
fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Collapse runs of whitespace into single spaces and trim.
fn normalise_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Percent-encode a string for use inside a URL path segment. Keeps the
/// characters that are safe inside a DOI path (`A-Za-z0-9-._~/:`).
fn url_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'/'
            | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strip_html_tags_removes_jats_paragraph() {
        let jats = "<jats:p>Hello <jats:italic>world</jats:italic>.</jats:p>";
        assert_eq!(normalise_whitespace(&strip_html_tags(jats)), "Hello world .");
    }

    #[test]
    fn normalise_whitespace_collapses_runs() {
        assert_eq!(
            normalise_whitespace("  multiple\n\t  spaces  here  "),
            "multiple spaces here"
        );
    }

    #[test]
    fn url_encode_path_encodes_special_chars() {
        assert_eq!(url_encode_path("10.1/abc def"), "10.1/abc%20def");
        assert_eq!(
            url_encode_path("10.48550/arXiv.2106.09685"),
            "10.48550/arXiv.2106.09685"
        );
    }

    #[test]
    fn reconstruct_inverted_index_orders_and_repeats() {
        let m = json!({ "the": [0, 2], "cat": [1], "ran": [3] });
        assert_eq!(
            reconstruct_inverted_index(m.as_object().unwrap()),
            "the cat the ran"
        );
    }

    #[tokio::test]
    async fn lookup_short_circuits_on_empty_inputs() {
        // No DOI and an empty title must not hit the network at all (the
        // sources point at an unroutable port, so a request would error
        // loudly rather than hang).
        let sources = Sources {
            crossref: "http://127.0.0.1:1".into(),
            semantic_scholar: "http://127.0.0.1:1".into(),
            openalex: "http://127.0.0.1:1".into(),
        };
        assert!(lookup(&sources, None, "   ").await.is_none());
        assert!(lookup(&sources, Some("  "), "").await.is_none());
    }
}
