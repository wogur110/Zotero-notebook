//! Assemble a bounded, abstracts-only context for multi-paper synthesis and
//! Q&A (collection overview, method comparison, library-wide questions).
//!
//! Pure and unit-tested: the Tauri command resolves the requested item keys to
//! `Item`s and hands them here; the prompt itself is built by
//! `crate::llm::provider::synthesis_system_prompt`.

use crate::llm::provider::{PaperBrief, MAX_SYNTHESIS_PAPERS};
use crate::models::Item;

/// The papers folded into a synthesis prompt, plus how many of the requested
/// items survived the cap (so the UI can tell the user when some were dropped).
pub struct SynthesisContext {
    pub papers: Vec<PaperBrief>,
    /// Papers actually included (after the cap).
    pub included: usize,
    /// Papers requested.
    pub total: usize,
}

/// Build the context from the requested items, in the given order, capping at
/// `MAX_SYNTHESIS_PAPERS`. Abstracts are passed through as-is (the prompt
/// builder truncates each); items without an abstract contribute their
/// title/venue only.
pub fn build_context(items: &[Item]) -> SynthesisContext {
    let total = items.len();
    let papers: Vec<PaperBrief> = items
        .iter()
        .take(MAX_SYNTHESIS_PAPERS)
        .map(|i| PaperBrief {
            title: i.title.clone(),
            creators: i.creators.clone(),
            year: i.year,
            publication: i.publication.clone(),
            abstract_text: i.abstract_text.clone(),
        })
        .collect();
    SynthesisContext {
        included: papers.len(),
        total,
        papers,
    }
}
