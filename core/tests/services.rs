//! Unit tests for the pure service modules: classification normalization,
//! settings persistence, and the summaries DB.

use zn_core::classify::{
    audit_to_proposal, build_audit_request, build_request, normalize_response, to_proposal,
};
use zn_core::llm::provider::{AuditResponse, ClassifyResponse};
use zn_core::models::{
    AppSettings, Collection, Item, Library, ProviderId, StoredSummary, SummarySource,
};
use zn_core::{db::Db, settings};

fn collection(key: &str, name: &str, parent: Option<&str>) -> Collection {
    Collection {
        key: key.into(),
        name: name.into(),
        parent_key: parent.map(String::from),
    }
}

fn item(key: &str, title: &str) -> Item {
    Item {
        key: key.into(),
        title: title.into(),
        item_type: "journalArticle".into(),
        creators: vec!["A B".into()],
        year: Some(2021),
        publication: Some("Venue".into()),
        doi: None,
        url: None,
        abstract_text: Some("Abstract.".into()),
        tags: vec![],
        date_added: None,
        collection_keys: vec![],
        attachment: None,
    }
}

fn library() -> Library {
    Library {
        collections: vec![
            collection("CV", "Computer Vision", None),
            collection("DM", "Diffusion Models", Some("CV")),
            collection("NLP", "NLP", None),
            collection("UNC", "Unclassified", None),
        ],
        items: vec![item("I1", "Paper one")],
        writable: true,
    }
}

fn resp(path: &[&str], is_new: bool) -> ClassifyResponse {
    ClassifyResponse {
        path: path.iter().map(|s| s.to_string()).collect(),
        is_new,
        confidence: 0.8,
        rationale: "because".into(),
        tags: vec![],
    }
}

#[test]
fn build_request_excludes_unclassified_paths() {
    let lib = library();
    let req = build_request(&lib.items[0], &lib);
    assert!(req
        .existing_paths
        .iter()
        .all(|p| !p[0].eq_ignore_ascii_case("Unclassified")));
    assert!(req
        .existing_paths
        .contains(&vec!["Computer Vision".to_string(), "Diffusion Models".to_string()]));
}

#[test]
fn normalize_adopts_existing_casing_and_detects_existing_path() {
    let lib = library();
    let (path, is_new) =
        normalize_response(&resp(&["computer VISION", "diffusion models"], true), &lib).unwrap();
    // Model lied about is_new and used wrong casing — both corrected.
    assert_eq!(path, vec!["Computer Vision", "Diffusion Models"]);
    assert!(!is_new);
}

#[test]
fn normalize_detects_new_leaf_under_existing_parent() {
    let lib = library();
    let (path, is_new) =
        normalize_response(&resp(&["computer vision", "3D Reconstruction"], false), &lib).unwrap();
    assert_eq!(path[0], "Computer Vision", "parent canonicalized");
    assert_eq!(path[1], "3D Reconstruction", "new leaf kept as proposed");
    assert!(is_new, "is_new recomputed from the tree, not the model's claim");
}

#[test]
fn normalize_truncates_depth_and_trims_segments() {
    let lib = library();
    let (path, _) = normalize_response(
        &resp(&["  NLP  ", "A", "B", "C", "D"], true),
        &lib,
    )
    .unwrap();
    assert_eq!(path.len(), 3, "max depth 3");
    assert_eq!(path[0], "NLP");
}

#[test]
fn normalize_rejects_unclassified_target() {
    let lib = library();
    assert!(normalize_response(&resp(&["unclassified", "X"], true), &lib).is_err());
}

#[test]
fn normalize_rejects_empty_path() {
    let lib = library();
    assert!(normalize_response(&resp(&["  ", ""], true), &lib).is_err());
}

#[test]
fn proposal_clamps_confidence_and_truncates_rationale() {
    let lib = library();
    let mut r = resp(&["NLP"], false);
    r.confidence = 7.5;
    r.rationale = "x".repeat(1000);
    let p = to_proposal(&lib.items[0], r, &lib).unwrap();
    assert!((p.confidence - 1.0).abs() < 1e-9);
    assert!(p.rationale.len() <= 504, "truncated with ellipsis");
    assert!(!p.is_new_collection);
}

fn audit_resp(misplaced: bool, path: &[&str]) -> AuditResponse {
    AuditResponse {
        misplaced,
        path: path.iter().map(|s| s.to_string()).collect(),
        confidence: 0.7,
        rationale: "reason".into(),
    }
}

#[test]
fn audit_request_excludes_unclassified_and_skips_unfiled() {
    let mut lib = library();
    // I1 currently in Diffusion Models + Unclassified.
    lib.items[0].collection_keys = vec!["DM".into(), "UNC".into()];
    let req = build_audit_request(&lib.items[0], &lib).unwrap();
    assert_eq!(
        req.current_paths,
        vec![vec!["Computer Vision".to_string(), "Diffusion Models".to_string()]],
        "Unclassified membership is not part of the audited filing"
    );

    // An item only in Unclassified (or unfiled) has nothing to audit.
    lib.items[0].collection_keys = vec!["UNC".into()];
    assert!(build_audit_request(&lib.items[0], &lib).is_none());
    lib.items[0].collection_keys = vec![];
    assert!(build_audit_request(&lib.items[0], &lib).is_none());
}

#[test]
fn audit_not_misplaced_yields_no_proposal() {
    let mut lib = library();
    lib.items[0].collection_keys = vec!["DM".into()];
    let out = audit_to_proposal(&lib.items[0], audit_resp(false, &[]), &lib).unwrap();
    assert!(out.is_none());
}

#[test]
fn audit_proposal_matching_current_path_is_dropped() {
    let mut lib = library();
    lib.items[0].collection_keys = vec!["DM".into()];
    // Model says misplaced but proposes (case-insensitively) where it already is.
    let out = audit_to_proposal(
        &lib.items[0],
        audit_resp(true, &["computer vision", "DIFFUSION MODELS"]),
        &lib,
    )
    .unwrap();
    assert!(out.is_none());
}

#[test]
fn audit_proposal_carries_current_keys_and_normalized_target() {
    let mut lib = library();
    lib.items[0].collection_keys = vec!["DM".into(), "UNC".into()];
    let out = audit_to_proposal(
        &lib.items[0],
        audit_resp(true, &["nlp"]),
        &lib,
    )
    .unwrap()
    .unwrap();
    assert_eq!(out.proposed_path, vec!["NLP"], "canonicalized to existing casing");
    assert!(!out.is_new_collection);
    assert_eq!(out.current_keys, vec!["DM"], "only real memberships are replaced");
    assert_eq!(out.current_paths.len(), 1);
}

#[test]
fn settings_round_trip_and_corrupt_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("settings.json");

    // Missing file → defaults.
    let s = settings::load(&path);
    assert_eq!(s, AppSettings::default());
    assert_eq!(s.default_provider, ProviderId::Gemini);

    // Round trip.
    let custom = AppSettings {
        default_provider: ProviderId::Anthropic,
        anthropic_model: "claude-opus-4-8".into(),
        file_root: Some("/papers".into()),
        ..AppSettings::default()
    };
    settings::save(&path, &custom).unwrap();
    assert_eq!(settings::load(&path), custom);

    // Corrupt file → defaults, not a crash.
    std::fs::write(&path, "{not json").unwrap();
    assert_eq!(settings::load(&path), AppSettings::default());
}

#[test]
fn db_upsert_get_and_overwrite() {
    let db = Db::open_in_memory().unwrap();
    assert!(db.get_summary("K1").unwrap().is_none());

    let first = StoredSummary {
        item_key: "K1".into(),
        summary: "First summary.".into(),
        provider: "gemini".into(),
        model: "gemini-2.5-pro".into(),
        created_at: "2026-06-11T00:00:00Z".into(),
        source: SummarySource::Metadata,
    };
    db.upsert_summary(&first).unwrap();
    let stored = db.get_summary("K1").unwrap().unwrap();
    assert_eq!(stored.summary, "First summary.");
    assert_eq!(stored.source, SummarySource::Metadata, "source round-trips");

    let second = StoredSummary {
        summary: "Regenerated with Claude.".into(),
        provider: "anthropic".into(),
        model: "claude-opus-4-8".into(),
        source: SummarySource::Fulltext,
        ..first
    };
    db.upsert_summary(&second).unwrap();
    let stored = db.get_summary("K1").unwrap().unwrap();
    assert_eq!(stored.summary, "Regenerated with Claude.");
    assert_eq!(stored.provider, "anthropic");
    assert_eq!(stored.source, SummarySource::Fulltext);
}

/// A summaries.sqlite created before the had_abstract/source columns
/// existed must upgrade in place, with old rows defaulting to a benign
/// source (no warning badge).
#[test]
fn db_migrates_pre_1_0_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("summaries.sqlite");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE summaries (
                item_key   TEXT PRIMARY KEY,
                summary    TEXT NOT NULL,
                provider   TEXT NOT NULL,
                model      TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            INSERT INTO summaries VALUES ('OLD', 'legacy summary', 'gemini', 'm', 't');",
        )
        .unwrap();
    }
    let db = Db::open(&path).unwrap();
    let row = db.get_summary("OLD").unwrap().unwrap();
    assert_eq!(row.summary, "legacy summary");
    assert_eq!(
        row.source,
        SummarySource::Abstract,
        "legacy rows default to abstract (no warning badge)"
    );
}

#[test]
fn db_open_on_disk_creates_parents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a").join("b").join("summaries.sqlite");
    let db = Db::open(&path).unwrap();
    db.upsert_summary(&StoredSummary {
        item_key: "K".into(),
        summary: "s".into(),
        provider: "gemini".into(),
        model: "m".into(),
        created_at: "t".into(),
        source: SummarySource::Abstract,
    })
    .unwrap();
    assert!(path.exists());
}

/// A settings.json written before the local-LLM fields existed must still
/// parse (serde defaults), keeping the user's other settings intact.
#[test]
fn settings_parse_pre_local_provider_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{
            "defaultProvider": "anthropic",
            "geminiModel": "gemini-2.5-pro",
            "anthropicModel": "claude-opus-4-8",
            "zoteroBaseUrl": "http://127.0.0.1:23119",
            "fileRoot": "/papers"
        }"#,
    )
    .unwrap();
    let s = settings::load(&path);
    assert_eq!(s.default_provider, ProviderId::Anthropic, "old fields kept");
    assert_eq!(s.file_root.as_deref(), Some("/papers"));
    assert_eq!(s.local_base_url, "http://127.0.0.1:11434/v1", "new field defaulted");
    assert_eq!(s.local_model, "llama3.1:8b");
}

#[test]
fn reading_state_upsert_get_delete() {
    use zn_core::models::{ReadingState, ReadingStatus};
    let db = Db::open_in_memory().unwrap();
    assert!(db.all_reading_states().unwrap().is_empty());

    let s = ReadingState {
        item_key: "K1".into(),
        status: Some(ReadingStatus::ToRead),
        starred: true,
        note: "read after the deadline".into(),
        updated_at: "2026-06-24T00:00:00Z".into(),
    };
    db.upsert_reading_state(&s).unwrap();
    let all = db.all_reading_states().unwrap();
    assert_eq!(all, vec![s.clone()]);

    // Upsert overwrites in place (still one row).
    let updated = ReadingState {
        status: Some(ReadingStatus::Read),
        starred: false,
        ..s
    };
    db.upsert_reading_state(&updated).unwrap();
    let all = db.all_reading_states().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].status, Some(ReadingStatus::Read));
    assert!(!all[0].starred);

    // Delete = untrack.
    db.delete_reading_state("K1").unwrap();
    assert!(db.all_reading_states().unwrap().is_empty());
}

#[test]
fn reading_state_starred_without_status_roundtrips() {
    use zn_core::models::ReadingState;
    let db = Db::open_in_memory().unwrap();
    // Star a paper with no reading status at all (the decoupled-star case).
    let s = ReadingState {
        item_key: "K2".into(),
        status: None,
        starred: true,
        note: String::new(),
        updated_at: "2026-06-25T00:00:00Z".into(),
    };
    db.upsert_reading_state(&s).unwrap();
    let all = db.all_reading_states().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].status, None, "no status persists as None, not ToRead");
    assert!(all[0].starred);
}

#[test]
fn usage_log_accumulates_summary() {
    let db = Db::open_in_memory().unwrap();
    let empty = db.usage_summary().unwrap();
    assert_eq!(empty.operation_count, 0);
    assert_eq!(empty.total_cost_usd, 0.0);

    db.insert_usage("summary", "anthropic", "claude-opus-4-8", 1000, 200, 0.030, "t1")
        .unwrap();
    db.insert_usage("classify", "gemini", "gemini-2.5-pro", 500, 50, 0.001125, "t2")
        .unwrap();

    let s = db.usage_summary().unwrap();
    assert_eq!(s.operation_count, 2);
    assert_eq!(s.total_input_tokens, 1500);
    assert_eq!(s.total_output_tokens, 250);
    assert!((s.total_cost_usd - 0.031125).abs() < 1e-9);
}

#[test]
fn citation_cache_upsert_and_get() {
    let db = Db::open_in_memory().unwrap();
    assert!(db.get_citation_cache("K1").unwrap().is_none());

    db.upsert_citation_cache("K1", "{\"references\":[]}", "2026-06-24T00:00:00Z")
        .unwrap();
    let (json, at) = db.get_citation_cache("K1").unwrap().unwrap();
    assert_eq!(json, "{\"references\":[]}");
    assert_eq!(at, "2026-06-24T00:00:00Z");

    // Upsert overwrites in place.
    db.upsert_citation_cache("K1", "{\"references\":[1]}", "2026-06-25T00:00:00Z")
        .unwrap();
    let (json, at) = db.get_citation_cache("K1").unwrap().unwrap();
    assert_eq!(json, "{\"references\":[1]}");
    assert_eq!(at, "2026-06-25T00:00:00Z");
}

#[test]
fn db_all_summaries_lists_every_row() {
    let db = Db::open_in_memory().unwrap();
    for (key, src) in [("A", SummarySource::Abstract), ("B", SummarySource::Fulltext)] {
        db.upsert_summary(&StoredSummary {
            item_key: key.into(),
            summary: format!("summary {key}"),
            provider: "gemini".into(),
            model: "m".into(),
            created_at: "t".into(),
            source: src,
        })
        .unwrap();
    }
    let mut all = db.all_summaries().unwrap();
    all.sort_by(|a, b| a.item_key.cmp(&b.item_key));
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].item_key, "A");
    assert_eq!(all[1].source, SummarySource::Fulltext);
}

#[test]
fn tag_suggestions_are_normalized_against_the_library() {
    use zn_core::classify::normalize_tags;
    let mut lib = library();
    // Library vocabulary: "Diffusion Models" (existing casing), on another item.
    lib.items.push(item("I2", "Other paper"));
    lib.items[1].tags = vec!["Diffusion Models".into(), "vision".into()];
    // The paper being classified already has "vision".
    lib.items[0].tags = vec!["vision".into()];

    let raw = vec![
        "diffusion models".to_string(), // exists in vocab → adopt exact casing
        "VISION".to_string(),           // item already has it → skipped
        "  ".to_string(),               // empty → skipped
        "diffusion models".to_string(), // duplicate → skipped
        "score matching".to_string(),   // new tag → kept as written
        "extra-1".to_string(),
        "extra-2".to_string(),
        "extra-3".to_string(),          // over the cap of 4 → dropped
    ];
    let tags = normalize_tags(&raw, &lib.items[0], &lib);
    assert_eq!(
        tags,
        vec!["Diffusion Models", "score matching", "extra-1", "extra-2"]
    );
}

#[test]
fn popular_tags_ranked_by_frequency() {
    use zn_core::classify::popular_tags;
    let mut lib = library();
    lib.items.push(item("I2", "B"));
    lib.items.push(item("I3", "C"));
    lib.items[0].tags = vec!["nlp".into(), "rare".into()];
    lib.items[1].tags = vec!["NLP".into()]; // case-insensitive merge
    lib.items[2].tags = vec!["nlp".into()];
    let tags = popular_tags(&lib, 10);
    assert_eq!(tags[0], "nlp", "most frequent first, first-seen casing kept");
    assert!(tags.contains(&"rare".to_string()));
}

#[test]
fn synthesis_context_caps_papers_and_preserves_order() {
    use zn_core::llm::provider::MAX_SYNTHESIS_PAPERS;
    use zn_core::synthesis::build_context;

    let items: Vec<Item> = (0..MAX_SYNTHESIS_PAPERS + 5)
        .map(|i| item(&format!("K{i}"), &format!("Paper {i}")))
        .collect();
    let ctx = build_context(&items);
    assert_eq!(ctx.total, MAX_SYNTHESIS_PAPERS + 5);
    assert_eq!(ctx.included, MAX_SYNTHESIS_PAPERS, "capped");
    assert_eq!(ctx.papers.len(), MAX_SYNTHESIS_PAPERS);
    assert_eq!(ctx.papers[0].title, "Paper 0", "order preserved");
    assert_eq!(ctx.papers[MAX_SYNTHESIS_PAPERS - 1].title, format!("Paper {}", MAX_SYNTHESIS_PAPERS - 1));
}

#[test]
fn synthesis_prompt_numbers_papers_and_pins_english() {
    use zn_core::llm::provider::{synthesis_system_prompt, PaperBrief};

    let papers = vec![
        PaperBrief {
            title: "Denoising Diffusion".into(),
            creators: vec!["Ho".into()],
            year: Some(2020),
            publication: Some("NeurIPS".into()),
            abstract_text: Some("We present high quality image synthesis.".into()),
        },
        PaperBrief {
            title: "Attention Is All You Need".into(),
            creators: vec!["Vaswani".into()],
            year: Some(2017),
            publication: Some("NeurIPS".into()),
            abstract_text: None,
        },
    ];
    let prompt = synthesis_system_prompt(&papers, "English");
    assert!(prompt.contains("[Paper 1]"));
    assert!(prompt.contains("[Paper 2]"));
    assert!(prompt.contains("set of 2 academic"));
    assert!(prompt.contains("Always answer in English"));
    assert!(prompt.contains("Denoising Diffusion"));
    // A paper with no abstract still appears, flagged as unavailable.
    assert!(prompt.contains("Abstract: (not available)"));
}

#[test]
fn synthesis_prompt_truncates_long_abstracts() {
    use zn_core::llm::provider::{synthesis_system_prompt, PaperBrief, MAX_SYNTHESIS_ABSTRACT_CHARS};

    let long = "word ".repeat(2000); // ~10k chars
    let papers = vec![PaperBrief {
        title: "Long".into(),
        creators: vec![],
        year: None,
        publication: None,
        abstract_text: Some(long),
    }];
    let prompt = synthesis_system_prompt(&papers, "English");
    assert!(prompt.contains('…'), "long abstract truncated with ellipsis");
    // The whole prompt stays bounded well under the raw abstract length.
    assert!(prompt.len() < MAX_SYNTHESIS_ABSTRACT_CHARS * 4);
}

#[test]
fn prompts_honor_the_output_language() {
    use zn_core::classify::build_request;
    use zn_core::llm::provider::{
        classify_prompt, summarize_prompt, synthesis_system_prompt, PaperBrief, SummarizeRequest,
    };

    let mut sreq = SummarizeRequest {
        title: "T".into(),
        creators: vec![],
        year: None,
        publication: None,
        abstract_text: None,
        body_excerpt: None,
        language: "Korean".into(),
    };
    assert!(summarize_prompt(&sreq).contains("in Korean"));
    sreq.language = "English".into();
    assert!(summarize_prompt(&sreq).contains("in English"));

    // Classify localizes only the rationale; the language flows in via the
    // request field (the Tauri command sets it from settings).
    let lib = library();
    let mut creq = build_request(&lib.items[0], &lib);
    creq.language = "Korean".into();
    assert!(classify_prompt(&creq).contains("rationale` field in Korean"));

    let papers = vec![PaperBrief {
        title: "P".into(),
        creators: vec![],
        year: None,
        publication: None,
        abstract_text: None,
    }];
    assert!(synthesis_system_prompt(&papers, "Korean").contains("answer in Korean"));
}

#[test]
fn summary_note_html_escapes_and_carries_marker() {
    use zn_core::models::SUMMARY_NOTE_MARKER;
    let s = StoredSummary {
        item_key: "K".into(),
        summary: "Shows a < b & c > d.".into(),
        provider: "anthropic".into(),
        model: "claude-opus-4-8".into(),
        created_at: "2026-06-11T00:00:00Z".into(),
        source: SummarySource::Fulltext,
    };
    let html = s.note_html();
    assert!(html.contains(SUMMARY_NOTE_MARKER));
    assert!(html.contains("a &lt; b &amp; c &gt; d"), "summary text escaped");
    assert!(html.contains("based on full text"));
}
