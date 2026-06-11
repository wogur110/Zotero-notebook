//! Pure helpers for the classify → review → apply pipeline.
//!
//! The LLM's answer is treated as untrusted input: paths are normalized and
//! canonicalized against the real collection tree, and the "is this new?"
//! flag is recomputed here rather than believed.

use crate::error::{Error, Result};
use crate::llm::provider::{ClassifyRequest, ClassifyResponse};
use crate::models::{ClassificationProposal, Item, Library, UNCLASSIFIED_COLLECTION};

const MAX_DEPTH: usize = 3;
const MAX_RATIONALE: usize = 500;

fn eq_ci(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Build the LLM request for one item, advertising every existing collection
/// path except the Unclassified tree itself.
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
    }
}

/// Normalize and canonicalize a model-proposed path.
///
/// Returns the cleaned path and whether it creates at least one new
/// collection (computed from the tree, ignoring the model's claim).
pub fn normalize_response(
    resp: &ClassifyResponse,
    library: &Library,
) -> Result<(Vec<String>, bool)> {
    let mut path: Vec<String> = resp
        .path
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

pub fn to_proposal(
    item_key: &str,
    resp: ClassifyResponse,
    library: &Library,
) -> Result<ClassificationProposal> {
    let (path, is_new) = normalize_response(&resp, library)?;
    let mut rationale = resp.rationale.trim().to_string();
    if rationale.len() > MAX_RATIONALE {
        let mut end = MAX_RATIONALE;
        while !rationale.is_char_boundary(end) {
            end -= 1;
        }
        rationale.truncate(end);
        rationale.push('…');
    }
    Ok(ClassificationProposal {
        item_key: item_key.to_string(),
        proposed_path: path,
        is_new_collection: is_new,
        confidence: resp.confidence.clamp(0.0, 1.0),
        rationale,
    })
}
