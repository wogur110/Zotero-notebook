//! Unit tests for the pure service modules: classification normalization,
//! settings persistence, and the summaries DB.

use zn_core::classify::{build_request, normalize_response, to_proposal};
use zn_core::llm::provider::ClassifyResponse;
use zn_core::models::{
    AppSettings, Collection, Item, Library, ProviderId, StoredSummary,
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
    let p = to_proposal("I1", r, &lib).unwrap();
    assert!((p.confidence - 1.0).abs() < 1e-9);
    assert!(p.rationale.len() <= 504, "truncated with ellipsis");
    assert!(!p.is_new_collection);
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
    };
    db.upsert_summary(&first).unwrap();
    assert_eq!(db.get_summary("K1").unwrap().unwrap().summary, "First summary.");

    let second = StoredSummary {
        summary: "Regenerated with Claude.".into(),
        provider: "anthropic".into(),
        model: "claude-opus-4-8".into(),
        ..first
    };
    db.upsert_summary(&second).unwrap();
    let stored = db.get_summary("K1").unwrap().unwrap();
    assert_eq!(stored.summary, "Regenerated with Claude.");
    assert_eq!(stored.provider, "anthropic");
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
    })
    .unwrap();
    assert!(path.exists());
}
