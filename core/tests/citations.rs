//! `citations::fetch` against a mock OpenAlex (work lookup + reference
//! expansion + citing-works query), mirroring the abstract_lookup harness.

use serde_json::json;
use wiremock::matchers::{method, path, path_regex, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use zn_core::citations::fetch;

#[tokio::test]
async fn fetch_builds_graph_from_openalex() {
    let server = MockServer::start().await;

    // 1. The work itself: its id, citation count, and referenced works.
    Mock::given(method("GET"))
        .and(path_regex(r"^/works/https://doi\.org/.*$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "https://openalex.org/W100",
            "cited_by_count": 42,
            "referenced_works": ["https://openalex.org/W2741809807"]
        })))
        .mount(&server)
        .await;

    // 2. Reference expansion (batched OR filter).
    Mock::given(method("GET"))
        .and(path("/works"))
        .and(query_param("filter", "openalex:W2741809807"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "id": "https://openalex.org/W2741809807",
                "doi": "https://doi.org/10.2/REF",
                "title": "A Referenced Work",
                "publication_year": 2017,
                "cited_by_count": 1000
            }]
        })))
        .mount(&server)
        .await;

    // 3. Citing works.
    Mock::given(method("GET"))
        .and(path("/works"))
        .and(query_param("filter", "cites:W100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "id": "https://openalex.org/W200",
                "doi": null,
                "title": "A Citing Work",
                "publication_year": 2023,
                "cited_by_count": 5
            }]
        })))
        .mount(&server)
        .await;

    let graph = fetch(&server.uri(), "10.1/abc").await.expect("graph");
    assert_eq!(graph.cited_by_count, 42);
    assert!(!graph.fetch_failed);

    assert_eq!(graph.references.len(), 1);
    assert_eq!(graph.references[0].title, "A Referenced Work");
    // DOI is normalized to a bare lowercase form for matching.
    assert_eq!(graph.references[0].doi.as_deref(), Some("10.2/ref"));
    assert_eq!(graph.references[0].cited_by_count, 1000);
    assert!(graph.references[0].in_library_key.is_none());

    assert_eq!(graph.citations.len(), 1);
    assert_eq!(graph.citations[0].title, "A Citing Work");
    assert_eq!(graph.citations[0].doi, None);
}

#[tokio::test]
async fn fetch_unknown_doi_is_none() {
    let server = MockServer::start().await;
    // No mocks mounted → wiremock 404s everything.
    assert!(fetch(&server.uri(), "10.1/missing").await.is_none());
}

#[tokio::test]
async fn fetch_empty_doi_is_none() {
    // Empty DOI short-circuits without a network call.
    assert!(fetch("http://127.0.0.1:1", "   ").await.is_none());
}
