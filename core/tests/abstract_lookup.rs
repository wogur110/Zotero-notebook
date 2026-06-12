//! The Crossref → Semantic Scholar → OpenAlex fallback chain against mock
//! servers (all three backends point at the same wiremock instance, on
//! their distinct paths).

use serde_json::json;
use wiremock::matchers::{method, path, path_regex, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use zn_core::abstract_lookup::{lookup, Sources};

fn sources(server: &MockServer) -> Sources {
    Sources {
        crossref: server.uri(),
        semantic_scholar: server.uri(),
        openalex: server.uri(),
    }
}

const DOI: &str = "10.48550/arXiv.2006.11239";

#[tokio::test]
async fn crossref_hit_wins_and_jats_is_stripped() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/works/{DOI}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "message": { "abstract": "<jats:p>We present  <jats:italic>DDPM</jats:italic>, a model.</jats:p>" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let found = lookup(&sources(&server), Some(DOI), "DDPM").await;
    assert_eq!(found.as_deref(), Some("We present DDPM , a model."));
}

#[tokio::test]
async fn falls_back_to_semantic_scholar_then_openalex() {
    let server = MockServer::start().await;
    // Crossref: 404.
    Mock::given(method("GET"))
        .and(path(format!("/works/{DOI}")))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    // Semantic Scholar by DOI: answers with a null abstract → unusable.
    Mock::given(method("GET"))
        .and(path_regex(r"^/graph/v1/paper/DOI:.*$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "abstract": null })))
        .mount(&server)
        .await;
    // OpenAlex: inverted index that must be reconstructed in order.
    Mock::given(method("GET"))
        .and(path_regex(r"^/works/https://doi\.org/.*$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "abstract_inverted_index": { "world": [1], "Hello": [0] }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let found = lookup(&sources(&server), Some(DOI), "ignored title").await;
    assert_eq!(found.as_deref(), Some("Hello world"));
}

#[tokio::test]
async fn no_doi_uses_title_search() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/graph/v1/paper/search"))
        .and(query_param("query", "Some Obscure Paper"))
        .and(query_param("limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "title": "Some Obscure Paper", "abstract": "Found by title." }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let found = lookup(&sources(&server), None, "Some Obscure Paper").await;
    assert_eq!(found.as_deref(), Some("Found by title."));
}

#[tokio::test]
async fn every_backend_failing_yields_none() {
    let server = MockServer::start().await;
    // No mocks mounted → wiremock answers 404 to everything.
    let found = lookup(&sources(&server), Some(DOI), "Some Paper").await;
    assert!(found.is_none());
}
