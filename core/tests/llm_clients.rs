//! Wire-format tests for the LLM clients against mock servers: request
//! shape (headers, body fields), response parsing, and error mapping.

use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use zn_core::llm::anthropic::AnthropicClient;
use zn_core::llm::gemini::GeminiClient;
use zn_core::llm::provider::{ClassifyRequest, SummarizeRequest};
use zn_core::Error;

fn summarize_req() -> SummarizeRequest {
    SummarizeRequest {
        title: "Denoising Diffusion Probabilistic Models".into(),
        creators: vec!["Jonathan Ho".into()],
        year: Some(2020),
        publication: Some("NeurIPS".into()),
        abstract_text: Some("We present high quality image synthesis.".into()),
    }
}

fn classify_req() -> ClassifyRequest {
    ClassifyRequest {
        title: "DDPM".into(),
        creators: vec!["Jonathan Ho".into()],
        year: Some(2020),
        publication: None,
        abstract_text: None,
        tags: vec!["diffusion".into()],
        existing_paths: vec![vec!["Computer Vision".into()]],
    }
}

// --- Gemini -----------------------------------------------------------

#[tokio::test]
async fn gemini_summarize_request_shape_and_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-pro:generateContent"))
        .and(query_param("key", "k-123"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            let text = body["contents"][0]["parts"][0]["text"].as_str().unwrap();
            assert!(text.contains("Denoising Diffusion"), "prompt must contain the title");
            assert!(
                body.get("generationConfig").is_none(),
                "summarize must not constrain output format"
            );
            ResponseTemplate::new(200).set_body_json(json!({
                "candidates": [{ "content": { "parts": [{ "text": "A clear summary." }] } }]
            }))
        })
        .mount(&server)
        .await;

    let client = GeminiClient::new("k-123".into(), "gemini-2.5-pro".into(), server.uri());
    let summary = client.summarize(&summarize_req()).await.unwrap();
    assert_eq!(summary, "A clear summary.");
}

#[tokio::test]
async fn gemini_classify_uses_response_schema_and_parses_json() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-pro:generateContent"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(
                body["generationConfig"]["responseMimeType"], "application/json",
                "classify must request JSON output"
            );
            assert_eq!(body["generationConfig"]["responseSchema"]["type"], "OBJECT");
            ResponseTemplate::new(200).set_body_json(json!({
                "candidates": [{ "content": { "parts": [{
                    "text": "{\"path\": [\"Computer Vision\"], \"is_new\": false, \"confidence\": 0.9, \"rationale\": \"fits\"}"
                }] } }]
            }))
        })
        .mount(&server)
        .await;

    let client = GeminiClient::new("k".into(), "gemini-2.5-pro".into(), server.uri());
    let resp = client.classify(&classify_req()).await.unwrap();
    assert_eq!(resp.path, vec!["Computer Vision"]);
    assert!(!resp.is_new);
    assert!((resp.confidence - 0.9).abs() < 1e-9);
}

#[tokio::test]
async fn gemini_invalid_key_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-flash:generateContent"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "status": "INVALID_ARGUMENT", "message": "API key not valid", "details": [{"reason": "API_KEY_INVALID"}] }
        })))
        .mount(&server)
        .await;

    let client = GeminiClient::new("bad".into(), "gemini-2.5-pro".into(), server.uri());
    let err = client.test_key().await.unwrap_err();
    assert!(err.to_string().contains("Invalid Gemini API key"), "{err}");
}

#[tokio::test]
async fn gemini_block_reason_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-pro:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "promptFeedback": { "blockReason": "SAFETY" }
        })))
        .mount(&server)
        .await;

    let client = GeminiClient::new("k".into(), "gemini-2.5-pro".into(), server.uri());
    let err = client.summarize(&summarize_req()).await.unwrap_err();
    assert!(err.to_string().contains("SAFETY"), "{err}");
}

// --- Anthropic --------------------------------------------------------

#[tokio::test]
async fn anthropic_summarize_request_shape_and_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "sk-ant-test"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["model"], "claude-opus-4-8");
            assert!(body.get("temperature").is_none(), "no sampling params on Opus 4.7+");
            assert!(body.get("top_p").is_none());
            let content = body["messages"][0]["content"].as_str().unwrap();
            assert!(content.contains("Denoising Diffusion"));
            ResponseTemplate::new(200).set_body_json(json!({
                "content": [{ "type": "text", "text": "A clear summary." }],
                "stop_reason": "end_turn"
            }))
        })
        .mount(&server)
        .await;

    let client = AnthropicClient::new(
        "sk-ant-test".into(),
        "claude-opus-4-8".into(),
        server.uri(),
    );
    let summary = client.summarize(&summarize_req()).await.unwrap();
    assert_eq!(summary, "A clear summary.");
}

#[tokio::test]
async fn anthropic_classify_uses_output_config_json_schema() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["output_config"]["format"]["type"], "json_schema");
            assert_eq!(
                body["output_config"]["format"]["schema"]["properties"]["path"]["type"],
                "array"
            );
            ResponseTemplate::new(200).set_body_json(json!({
                "content": [{ "type": "text",
                    "text": "{\"path\": [\"Computer Vision\", \"Diffusion Models\"], \"is_new\": true, \"confidence\": 0.7, \"rationale\": \"new subtopic\"}" }],
                "stop_reason": "end_turn"
            }))
        })
        .mount(&server)
        .await;

    let client = AnthropicClient::new("k".into(), "claude-opus-4-8".into(), server.uri());
    let resp = client.classify(&classify_req()).await.unwrap();
    assert_eq!(resp.path.len(), 2);
    assert!(resp.is_new);
}

#[tokio::test]
async fn anthropic_invalid_key_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "type": "error",
            "error": { "type": "authentication_error", "message": "invalid x-api-key" }
        })))
        .mount(&server)
        .await;

    let client = AnthropicClient::new("bad".into(), "claude-opus-4-8".into(), server.uri());
    let err = client.test_key().await.unwrap_err();
    assert!(err.to_string().contains("Invalid Anthropic API key"), "{err}");
}

#[tokio::test]
async fn anthropic_refusal_stop_reason_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [],
            "stop_reason": "refusal",
            "stop_details": { "category": null }
        })))
        .mount(&server)
        .await;

    let client = AnthropicClient::new("k".into(), "claude-opus-4-8".into(), server.uri());
    let err = client.summarize(&summarize_req()).await.unwrap_err();
    match &err {
        Error::Llm { provider, message } => {
            assert_eq!(provider, "anthropic");
            assert!(message.contains("declined"), "{message}");
        }
        other => panic!("expected Llm error, got {other:?}"),
    }
}

#[tokio::test]
async fn anthropic_truncated_classify_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{ "type": "text", "text": "{\"path\": [\"Comp" }],
            "stop_reason": "max_tokens"
        })))
        .mount(&server)
        .await;

    let client = AnthropicClient::new("k".into(), "claude-opus-4-8".into(), server.uri());
    let err = client.classify(&classify_req()).await.unwrap_err();
    assert!(err.to_string().contains("truncated"), "{err}");
}
