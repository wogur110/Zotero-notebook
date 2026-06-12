//! Contract tests for the Zotero clients against mock HTTP servers.
//! The plugin fixtures mirror docs/PLUGIN_API.md exactly — if these tests
//! pass, the Rust client and the documented wire format agree.

use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use zn_core::models::LinkMode;
use zn_core::zotero::{local_api, plugin_api::check_status, plugin_api::PluginClient};
use zn_core::Error;

fn library_fixture() -> serde_json::Value {
    json!({
        "collections": [
            { "key": "ABCD1234", "name": "Computer Vision", "parentKey": null },
            { "key": "EFGH5678", "name": "Diffusion Models", "parentKey": "ABCD1234" }
        ],
        "items": [
            {
                "key": "ITEM0001",
                "title": "Denoising Diffusion Probabilistic Models",
                "itemType": "conferencePaper",
                "creators": ["Jonathan Ho", "Ajay Jain", "Pieter Abbeel"],
                "year": 2020,
                "publication": "NeurIPS",
                "doi": "10.48550/arXiv.2006.11239",
                "url": "https://arxiv.org/abs/2006.11239",
                "abstractText": "We present high quality image synthesis...",
                "tags": ["diffusion", "generative"],
                "dateAdded": "2024-11-02T09:12:33Z",
                "collectionKeys": ["EFGH5678"],
                "attachment": {
                    "key": "ATTACH01",
                    "title": "Full Text PDF",
                    "filename": "Ho et al. - 2020 - DDPM.pdf",
                    "contentType": "application/pdf",
                    "linkMode": "linked_file",
                    "filePath": "C:\\Users\\me\\papers\\Diffusion Models\\Ho et al. - 2020 - DDPM.pdf"
                }
            }
        ]
    })
}

#[tokio::test]
async fn plugin_ping_returns_versions() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/zotero-notebook/ping"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "version": "0.1.0",
            "zoteroVersion": "7.0.11"
        })))
        .mount(&server)
        .await;

    let client = PluginClient::new(server.uri());
    let (plugin, zotero) = client.ping().await.unwrap();
    assert_eq!(plugin, "0.1.0");
    assert_eq!(zotero, "7.0.11");
}

#[tokio::test]
async fn plugin_library_parses_contract_example() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/zotero-notebook/library"))
        .respond_with(ResponseTemplate::new(200).set_body_json(library_fixture()))
        .mount(&server)
        .await;

    let library = PluginClient::new(server.uri()).fetch_library().await.unwrap();
    assert!(library.writable, "plugin-served libraries are writable");
    assert_eq!(library.collections.len(), 2);
    assert_eq!(
        library.collection_path("EFGH5678").unwrap(),
        vec!["Computer Vision", "Diffusion Models"]
    );
    let item = &library.items[0];
    assert_eq!(item.year, Some(2020));
    assert_eq!(item.creators.len(), 3);
    let att = item.attachment.as_ref().unwrap();
    assert_eq!(att.link_mode, LinkMode::LinkedFile);
    assert!(att.file_path.as_ref().unwrap().ends_with("DDPM.pdf"));
}

#[tokio::test]
async fn plugin_fulltext_happy_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/zotero-notebook/fulltext"))
        .and(query_param("itemKey", "ITEM0001"))
        .and(query_param("maxChars", "80000"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "text": "We present DDPM...",
            "indexed": true,
            "truncated": true,
            "chars": 123456
        })))
        .mount(&server)
        .await;

    let client = PluginClient::new(server.uri());
    let ft = client.fetch_fulltext("ITEM0001", 80000).await.unwrap().unwrap();
    assert_eq!(ft.text, "We present DDPM...");
    assert!(ft.truncated);
    assert_eq!(ft.chars, 123456);
}

#[tokio::test]
async fn plugin_fulltext_absent_cases_are_none() {
    let server = MockServer::start().await;
    // No text extracted (e.g. scanned PDF).
    Mock::given(method("GET"))
        .and(path("/zotero-notebook/fulltext"))
        .and(query_param("itemKey", "NOTEXT01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "text": null, "indexed": false, "truncated": false, "chars": 0
        })))
        .mount(&server)
        .await;
    // Old plugin without the route: Zotero's own non-JSON 404.
    Mock::given(method("GET"))
        .and(path("/zotero-notebook/fulltext"))
        .and(query_param("itemKey", "OLDPLUG1"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&server)
        .await;

    let client = PluginClient::new(server.uri());
    assert!(client.fetch_fulltext("NOTEXT01", 80000).await.unwrap().is_none());
    assert!(client.fetch_fulltext("OLDPLUG1", 80000).await.unwrap().is_none());
}

#[tokio::test]
async fn plugin_move_item_sends_contract_body_and_parses_result() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/zotero-notebook/move-item"))
        .and(body_partial_json(json!({
            "itemKey": "ITEM0001",
            "targetPath": ["Computer Vision", "Diffusion Models"],
            "removeFromCollections": ["UNCL0001"],
            "fileRoot": "/papers"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "collectionKey": "EFGH5678",
            "newFilePath": "/papers/Computer Vision/Diffusion Models/x.pdf"
        })))
        .mount(&server)
        .await;

    let result = PluginClient::new(server.uri())
        .move_item(
            "ITEM0001",
            &["Computer Vision".into(), "Diffusion Models".into()],
            &["UNCL0001".into()],
            Some("/papers"),
        )
        .await
        .unwrap();
    assert!(result.ok);
    assert_eq!(result.collection_key.as_deref(), Some("EFGH5678"));
    assert!(result.new_file_path.unwrap().ends_with("x.pdf"));
}

#[tokio::test]
async fn plugin_move_item_maps_error_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/zotero-notebook/move-item"))
        .respond_with(
            ResponseTemplate::new(500)
                .set_body_json(json!({ "error": "file already exists at destination" })),
        )
        .mount(&server)
        .await;

    let err = PluginClient::new(server.uri())
        .move_item("X", &["A".into()], &[], None)
        .await
        .unwrap_err();
    match err {
        Error::ZoteroRejected { status, message } => {
            assert_eq!(status, 500);
            assert!(message.contains("already exists"));
        }
        other => panic!("expected ZoteroRejected, got {other:?}"),
    }
}

#[tokio::test]
async fn plugin_route_404_is_plugin_missing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/zotero-notebook/library"))
        .respond_with(ResponseTemplate::new(404).set_body_string("No endpoint found"))
        .mount(&server)
        .await;

    let err = PluginClient::new(server.uri()).fetch_library().await.unwrap_err();
    assert!(matches!(err, Error::PluginMissing), "got {err:?}");
}

#[tokio::test]
async fn check_status_full_plugin() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/zotero-notebook/ping"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "version": "0.1.0", "zoteroVersion": "7.0.11"
        })))
        .mount(&server)
        .await;

    let status = check_status(&server.uri()).await;
    assert!(status.running && status.plugin_installed);
    assert_eq!(status.plugin_version.as_deref(), Some("0.1.0"));
    assert!(status.hint.is_none());
}

#[tokio::test]
async fn check_status_zotero_without_plugin() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/zotero-notebook/ping"))
        .respond_with(ResponseTemplate::new(404).set_body_string("No endpoint found"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/connector/ping"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Zotero is running"))
        .mount(&server)
        .await;

    let status = check_status(&server.uri()).await;
    assert!(status.running);
    assert!(!status.plugin_installed);
    assert!(status.hint.unwrap().contains("plugin"));
}

#[tokio::test]
async fn check_status_offline() {
    // Nothing is listening on this port.
    let status = check_status("http://127.0.0.1:1").await;
    assert!(!status.running);
    assert!(!status.plugin_installed);
}

#[tokio::test]
async fn local_api_fetch_library_paginates_and_resolves_children() {
    let server = MockServer::start().await;

    // Collections: one page.
    Mock::given(method("GET"))
        .and(path("/api/users/0/collections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "key": "COL1", "data": { "name": "ML", "parentCollection": false } },
            { "key": "COL2", "data": { "name": "Vision", "parentCollection": "COL1" } }
        ])))
        .mount(&server)
        .await;

    // Items: two pages (first full at limit=100 — simulate with start param).
    let full_page: Vec<serde_json::Value> = (0..100)
        .map(|i| {
            json!({
                "key": format!("IT{i:04}"),
                "data": {
                    "itemType": "journalArticle",
                    "title": format!("Paper {i}"),
                    "date": "2021-05-01",
                    "creators": [{ "firstName": "A", "lastName": "B" }],
                    "collections": ["COL1"],
                    "tags": []
                }
            })
        })
        .collect();
    Mock::given(method("GET"))
        .and(path("/api/users/0/items/top"))
        .and(query_param("start", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&full_page))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/users/0/items/top"))
        .and(query_param("start", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "key": "LAST1",
                "data": {
                    "itemType": "journalArticle",
                    "title": "Last paper",
                    "date": "March 2019",
                    "creators": [{ "name": "Some Org" }],
                    "collections": [],
                    "tags": [{ "tag": "survey" }],
                    "publicationTitle": "TPAMI",
                    "DOI": "10.1/xyz",
                    "abstractNote": "An abstract."
                }
            }
        ])))
        .mount(&server)
        .await;

    // Children: only LAST1 has a PDF; everything else has none.
    Mock::given(method("GET"))
        .and(path("/api/users/0/items/LAST1/children"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "key": "NOTE1",
                "data": { "itemType": "note" }
            },
            {
                "key": "ATT1",
                "data": {
                    "itemType": "attachment",
                    "title": "Full Text PDF",
                    "contentType": "application/pdf",
                    "linkMode": "linked_file",
                    "path": "/home/me/papers/last.pdf",
                    "filename": "last.pdf"
                }
            }
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(wiremock::matchers::path_regex(
            r"^/api/users/0/items/IT\d+/children$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let library = local_api::fetch_library(&server.uri()).await.unwrap();
    assert!(!library.writable);
    assert_eq!(library.collections.len(), 2);
    assert_eq!(library.collections[1].parent_key.as_deref(), Some("COL1"));
    assert_eq!(library.items.len(), 101);

    let last = library.items.iter().find(|i| i.key == "LAST1").unwrap();
    assert_eq!(last.year, Some(2019));
    assert_eq!(last.publication.as_deref(), Some("TPAMI"));
    assert_eq!(last.creators, vec!["Some Org"]);
    let att = last.attachment.as_ref().unwrap();
    assert_eq!(att.file_path.as_deref(), Some("/home/me/papers/last.pdf"));
}

#[tokio::test]
async fn local_api_403_explains_allow_other_applications() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/users/0/collections"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let err = local_api::fetch_library(&server.uri()).await.unwrap_err();
    match err {
        Error::ZoteroRejected { status, message } => {
            assert_eq!(status, 403);
            assert!(message.contains("Allow other applications"));
        }
        other => panic!("expected ZoteroRejected, got {other:?}"),
    }
}

#[tokio::test]
async fn local_api_linked_file_relative_path_is_unresolvable() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/users/0/collections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/users/0/items/top"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "key": "I1", "data": { "itemType": "journalArticle", "title": "T", "collections": [] } }
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/users/0/items/I1/children"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "key": "A1",
                "data": {
                    "itemType": "attachment",
                    "contentType": "application/pdf",
                    "linkMode": "linked_file",
                    "path": "attachments:papers/x.pdf",
                    "filename": "x.pdf"
                }
            }
        ])))
        .mount(&server)
        .await;

    let library = local_api::fetch_library(&server.uri()).await.unwrap();
    let att = library.items[0].attachment.as_ref().unwrap();
    assert_eq!(att.file_path, None, "attachments: paths cannot be resolved in fallback mode");
    assert_eq!(att.filename.as_deref(), Some("x.pdf"));
}

#[tokio::test]
async fn plugin_update_item_sends_contract_body_and_parses_result() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/zotero-notebook/update-item"))
        .and(body_partial_json(json!({
            "itemKey": "ITEM0001",
            "abstractIfEmpty": "An abstract.",
            "addTags": ["diffusion models"],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "wroteAbstract": true,
            "addedTags": ["diffusion models"],
            "noteKey": null
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = PluginClient::new(server.uri());
    let result = client
        .update_item(
            "ITEM0001",
            Some("An abstract."),
            &["diffusion models".to_string()],
            None,
        )
        .await
        .unwrap();
    assert!(result.wrote_abstract);
    assert_eq!(result.added_tags, vec!["diffusion models"]);
    assert!(result.note_key.is_none());
}

#[tokio::test]
async fn plugin_update_item_missing_route_is_plugin_missing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/zotero-notebook/update-item"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&server)
        .await;

    let client = PluginClient::new(server.uri());
    let err = client
        .update_item("ITEM0001", None, &[], Some("<h2>AI Summary — Zotero Notebook</h2>"))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::PluginMissing), "got: {err}");
}
