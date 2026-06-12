//! Pure helpers for the classify → review → apply pipeline.
//!
//! The LLM's answer is treated as untrusted input: paths are normalized and
//! canonicalized against the real collection tree, and the "is this new?"
//! flag is recomputed here rather than believed.

use crate::error::{Error, Result};
use crate::llm::provider::{AuditRequest, AuditResponse, ClassifyRequest, ClassifyResponse};
use crate::models::{
    AuditProposal, ClassificationProposal, Item, Library, UNCLASSIFIED_COLLECTION,
};

const MAX_DEPTH: usize = 3;
const MAX_RATIONALE: usize = 500;

fn eq_ci(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// How many of the library's most-used tags the classifier prompt
/// advertises as the preferred vocabulary.
const TAG_VOCAB_SIZE: usize = 40;
const MAX_SUGGESTED_TAGS: usize = 4;
const MAX_TAG_LEN: usize = 50;

/// The library's most-used tags, most frequent first (ties alphabetical).
pub fn popular_tags(library: &Library, limit: usize) -> Vec<String> {
    use std::collections::HashMap;
    // Count case-insensitively but keep the first-seen casing for display.
    let mut counts: HashMap<String, (String, usize)> = HashMap::new();
    for item in &library.items {
        for tag in &item.tags {
            let t = tag.trim();
            if t.is_empty() {
                continue;
            }
            let entry = counts
                .entry(t.to_lowercase())
                .or_insert_with(|| (t.to_string(), 0));
            entry.1 += 1;
        }
    }
    let mut tags: Vec<(String, usize)> = counts.into_values().collect();
    tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    tags.into_iter().take(limit).map(|(t, _)| t).collect()
}

/// Build the LLM request for one item, advertising every existing collection
/// path except the Unclassified tree itself, plus the tag vocabulary.
pub fn build_request(item: &Item, library: &Library) -> ClassifyRequest {
    let existing_paths = library
        .all_paths()
        .into_iter()
        .filter(|p| !p.first().is_some_and(|root| eq_ci(root, UNCLASSIFIED_COLLECTION)))
        .collect();
    ClassifyRequest {
        title: item.title.clone(),
        creators: item.creators.clone(),
        year: item.year,
        publication: item.publication.clone(),
        abstract_text: item.abstract_text.clone(),
        tags: item.tags.clone(),
        existing_paths,
        existing_tags: popular_tags(library, TAG_VOCAB_SIZE),
    }
}

/// Clean up model-suggested tags: trim, drop empties/overlong ones, dedupe
/// case-insensitively, skip tags the item already has, canonicalize casing
/// to the library's existing vocabulary, cap the count.
pub fn normalize_tags(raw: &[String], item: &Item, library: &Library) -> Vec<String> {
    let vocab = popular_tags(library, usize::MAX);
    let item_tags: Vec<String> = item.tags.iter().map(|t| t.trim().to_lowercase()).collect();
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for raw_tag in raw {
        let tag = raw_tag.trim();
        if tag.is_empty() || tag.len() > MAX_TAG_LEN {
            continue;
        }
        let lower = tag.to_lowercase();
        if item_tags.contains(&lower) || seen.contains(&lower) {
            continue;
        }
        // Adopt the library's exact casing when the tag already exists.
        let canonical = vocab
            .iter()
            .find(|v| v.to_lowercase() == lower)
            .cloned()
            .unwrap_or_else(|| tag.to_string());
        seen.push(lower);
        out.push(canonical);
        if out.len() == MAX_SUGGESTED_TAGS {
            break;
        }
    }
    out
}

/// Normalize and canonicalize a model-proposed path.
///
/// Returns the cleaned path and whether it creates at least one new
/// collection (computed from the tree, ignoring the model's claim).
pub fn normalize_response(
    resp: &ClassifyResponse,
    library: &Library,
) -> Result<(Vec<String>, bool)> {
    normalize_path(&resp.path, library)
}

/// Shared path normalization for classify and audit proposals.
pub fn normalize_path(raw: &[String], library: &Library) -> Result<(Vec<String>, bool)> {
    let mut path: Vec<String> = raw
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if path.is_empty() {
        return Err(Error::InvalidResponse(
            "the model returned an empty collection path".into(),
        ));
    }
    path.truncate(MAX_DEPTH);
    if eq_ci(&path[0], UNCLASSIFIED_COLLECTION) {
        return Err(Error::InvalidResponse(
            "the model proposed the Unclassified collection as a target".into(),
        ));
    }

    // Walk the existing tree level by level. While a (case-insensitive)
    // match exists, adopt the existing collection's exact name; the first
    // miss makes the rest of the path new.
    let mut is_new = false;
    let mut parent_key: Option<String> = None;
    for segment in path.iter_mut() {
        if is_new {
            break;
        }
        let found = library
            .collections
            .iter()
            .find(|c| c.parent_key == parent_key && eq_ci(&c.name, segment));
        match found {
            Some(c) => {
                *segment = c.name.clone();
                parent_key = Some(c.key.clone());
            }
            None => is_new = true,
        }
    }
    Ok((path, is_new))
}

fn trim_rationale(raw: &str) -> String {
    let mut rationale = raw.trim().to_string();
    if rationale.len() > MAX_RATIONALE {
        let mut end = MAX_RATIONALE;
        while !rationale.is_char_boundary(end) {
            end -= 1;
        }
        rationale.truncate(end);
        rationale.push('…');
    }
    rationale
}

pub fn to_proposal(
    item: &Item,
    resp: ClassifyResponse,
    library: &Library,
) -> Result<ClassificationProposal> {
    let (path, is_new) = normalize_response(&resp, library)?;
    Ok(ClassificationProposal {
        item_key: item.key.clone(),
        proposed_path: path,
        is_new_collection: is_new,
        confidence: resp.confidence.clamp(0.0, 1.0),
        rationale: trim_rationale(&resp.rationale),
        suggested_tags: normalize_tags(&resp.tags, item, library),
    })
}

// --- audit (re-checking already-classified papers) ---------------------

/// The item's collection memberships that count as "real" filing: every
/// membership whose path does not start at the Unclassified root.
/// Returns (collection key, nested path) pairs.
pub fn audit_memberships(item: &Item, library: &Library) -> Vec<(String, Vec<String>)> {
    item.collection_keys
        .iter()
        .filter_map(|key| {
            let path = library.collection_path(key)?;
            if path.first().is_some_and(|root| eq_ci(root, UNCLASSIFIED_COLLECTION)) {
                return None;
            }
            Some((key.clone(), path))
        })
        .collect()
}

/// Build the audit request for one item. `None` when the item has no real
/// filing to audit (it belongs in the Unclassified flow instead).
pub fn build_audit_request(item: &Item, library: &Library) -> Option<AuditRequest> {
    let memberships = audit_memberships(item, library);
    if memberships.is_empty() {
        return None;
    }
    let base = build_request(item, library);
    Some(AuditRequest {
        title: base.title,
        creators: base.creators,
        year: base.year,
        publication: base.publication,
        abstract_text: base.abstract_text,
        tags: base.tags,
        current_paths: memberships.into_iter().map(|(_, p)| p).collect(),
        existing_paths: base.existing_paths,
    })
}

/// Turn an audit answer into a move proposal.
///
/// `Ok(None)` means "leave the paper where it is" — either the model judged
/// the current filing fine, or its proposal normalized to a path the paper
/// is already in.
pub fn audit_to_proposal(
    item: &Item,
    resp: AuditResponse,
    library: &Library,
) -> Result<Option<AuditProposal>> {
    if !resp.misplaced {
        return Ok(None);
    }
    let (path, is_new) = normalize_path(&resp.path, library)?;
    let memberships = audit_memberships(item, library);
    let already_there = memberships.iter().any(|(_, current)| {
        current.len() == path.len()
            && current.iter().zip(&path).all(|(a, b)| eq_ci(a, b))
    });
    if already_there {
        return Ok(None);
    }
    Ok(Some(AuditProposal {
        item_key: item.key.clone(),
        current_paths: memberships.iter().map(|(_, p)| p.clone()).collect(),
        current_keys: memberships.into_iter().map(|(k, _)| k).collect(),
        proposed_path: path,
        is_new_collection: is_new,
        confidence: resp.confidence.clamp(0.0, 1.0),
        rationale: trim_rationale(&resp.rationale),
    }))
}
