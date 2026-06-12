//! Wire-format tests for the LLM clients against mock servers: request
//! shape (headers, body fields), response parsing, and error mapping.

use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use zn_core::llm::anthropic::AnthropicClient;
use zn_core::llm::gemini::GeminiClient;
use zn_core::llm::provider::{AuditRequest, ClassifyRequest, SummarizeRequest};
use zn_core::models::{ChatMessage, ChatRole};
use zn_core::Error;

fn chat_history() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: ChatRole::User,
        content: "What is the main contribution?".into(),
    }]
}

fn summarize_req() -> SummarizeRequest {
    SummarizeRequest {
        title: "Denoising Diffusion Probabilistic Models".into(),
        creators: vec!["Jonathan Ho".into()],
        year: Some(2020),
        publication: Some("NeurIPS".into()),
        abstract_text: Some("We present high quality image synthesis.".into()),
        body_excerpt: None,
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

fn audit_req() -> AuditRequest {
    AuditRequest {
        title: "Attention Is All You Need".into(),
        creators: vec!["Ashish Vaswani".into()],
        year: Some(2017),
        publication: Some("NeurIPS".into()),
        abstract_text: None,
        tags: vec![],
        current_paths: vec![vec!["Hardware".into()]],
        existing_paths: vec![vec!["Hardware".into()], vec!["NLP".into()]],
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
async fn gemini_audit_request_shape_and_parsing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-pro:generateContent"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            let text = body["contents"][0]["parts"][0]["text"].as_str().unwrap();
            assert!(text.contains("currently filed in"), "prompt states current filing");
            assert!(text.contains("Hardware"), "prompt contains the current path");
            assert_eq!(
                body["generationConfig"]["responseSchema"]["properties"]["misplaced"]["type"],
                "BOOLEAN"
            );
            ResponseTemplate::new(200).set_body_json(json!({
                "candidates": [{ "content": { "parts": [{
                    "text": "{\"misplaced\": true, \"path\": [\"NLP\"], \"confidence\": 0.85, \"rationale\": \"transformer paper\"}"
                }] } }]
            }))
        })
        .mount(&server)
        .await;

    let client = GeminiClient::new("k".into(), "gemini-2.5-pro".into(), server.uri());
    let resp = client.audit(&audit_req()).await.unwrap();
    assert!(resp.misplaced);
    assert_eq!(resp.path, vec!["NLP"]);
}

#[tokio::test]
async fn gemini_summarize_includes_body_excerpt() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-pro:generateContent"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            let text = body["contents"][0]["parts"][0]["text"].as_str().unwrap();
            assert!(text.contains("UNIQUE_BODY_TOKEN"), "prompt carries the body");
            assert!(text.contains("Full text"), "full-text framing used");
            ResponseTemplate::new(200).set_body_json(json!({
                "candidates": [{ "content": { "parts": [{ "text": "A deep summary." }] } }]
            }))
        })
        .mount(&server)
        .await;

    let client = GeminiClient::new("k".into(), "gemini-2.5-pro".into(), server.uri());
    let mut req = summarize_req();
    req.body_excerpt = Some("UNIQUE_BODY_TOKEN and the rest of the paper".into());
    assert_eq!(client.summarize(&req).await.unwrap(), "A deep summary.");
}

#[tokio::test]
async fn gemini_chat_streams_deltas() {
    let server = MockServer::start().await;
    let sse = concat!(
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"}]}}]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"lo\"}]}}]}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-pro:streamGenerateContent"))
        .and(query_param("alt", "sse"))
        .respond_with(move |req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(
                body["systemInstruction"]["parts"][0]["text"], "SYSTEM",
                "system prompt forwarded"
            );
            assert_eq!(body["contents"][0]["role"], "user");
            ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream")
        })
        .mount(&server)
        .await;

    let client = GeminiClient::new("k".into(), "gemini-2.5-pro".into(), server.uri());
    let mut deltas: Vec<String> = Vec::new();
    let full = client
        .chat_stream("SYSTEM", &chat_history(), &mut |t| deltas.push(t.into()))
        .await
        .unwrap();
    assert_eq!(full, "Hello");
    assert_eq!(deltas, vec!["Hel".to_string(), "lo".to_string()]);
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
async fn anthropic_audit_request_shape_and_parsing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert!(body.get("temperature").is_none());
            assert_eq!(body["output_config"]["format"]["type"], "json_schema");
            assert_eq!(
                body["output_config"]["format"]["schema"]["properties"]["misplaced"]["type"],
                "boolean"
            );
            let content = body["messages"][0]["content"].as_str().unwrap();
            assert!(content.contains("currently filed in"));
            ResponseTemplate::new(200).set_body_json(json!({
                "content": [{ "type": "text",
                    "text": "{\"misplaced\": false, \"path\": [], \"confidence\": 0.9, \"rationale\": \"fits fine\"}" }],
                "stop_reason": "end_turn"
            }))
        })
        .mount(&server)
        .await;

    let client = AnthropicClient::new("k".into(), "claude-opus-4-8".into(), server.uri());
    let resp = client.audit(&audit_req()).await.unwrap();
    assert!(!resp.misplaced);
    assert!(resp.path.is_empty());
}

#[tokio::test]
async fn anthropic_chat_streams_deltas() {
    let server = MockServer::start().await;
    let sse = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\"}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"The \"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"answer.\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(move |req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["stream"], true);
            assert_eq!(body["system"], "SYSTEM");
            assert!(body.get("temperature").is_none(), "no sampling params");
            assert_eq!(body["messages"][0]["role"], "user");
            ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream")
        })
        .mount(&server)
        .await;

    let client = AnthropicClient::new("k".into(), "claude-opus-4-8".into(), server.uri());
    let mut deltas: Vec<String> = Vec::new();
    let full = client
        .chat_stream("SYSTEM", &chat_history(), &mut |t| deltas.push(t.into()))
        .await
        .unwrap();
    assert_eq!(full, "The answer.");
    assert_eq!(deltas.len(), 2);
}

#[tokio::test]
async fn anthropic_chat_stream_refusal_is_an_error() {
    let server = MockServer::start().await;
    let sse = concat!(
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"part\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\"}}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .mount(&server)
        .await;

    let client = AnthropicClient::new("k".into(), "claude-opus-4-8".into(), server.uri());
    let err = client
        .chat_stream("SYSTEM", &chat_history(), &mut |_| {})
        .await
        .unwrap_err();
    assert!(err.to_string().contains("safety"), "got: {err}");
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

// --- Local (OpenAI-compatible) ------------------------------------------

use zn_core::llm::openai_compat::OpenAiCompatClient;

#[tokio::test]
async fn local_summarize_happy_path_without_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(|req: &Request| {
            assert!(
                req.headers.get("authorization").is_none(),
                "no Bearer header when no key is configured"
            );
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["model"], "llama3.1:8b");
            let text = body["messages"][0]["content"].as_str().unwrap();
            assert!(text.contains("Denoising Diffusion"));
            ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{ "message": { "role": "assistant", "content": "A local summary." } }]
            }))
        })
        .mount(&server)
        .await;

    let client = OpenAiCompatClient::new(
        None,
        "llama3.1:8b".into(),
        format!("{}/v1", server.uri()),
    );
    assert_eq!(client.summarize(&summarize_req()).await.unwrap(), "A local summary.");
}

#[tokio::test]
async fn local_classify_parses_fenced_json() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["response_format"]["type"], "json_schema");
            assert!(
                body["messages"][0]["content"].as_str().unwrap().contains("JSON Schema"),
                "schema is also embedded in the prompt for servers that ignore response_format"
            );
            ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{ "message": { "role": "assistant",
                    "content": "```json\n{\"path\": [\"NLP\"], \"is_new\": false, \"confidence\": 0.7, \"rationale\": \"fits\"}\n```" } }]
            }))
        })
        .mount(&server)
        .await;

    let client = OpenAiCompatClient::new(
        Some("secret".into()),
        "llama3.1:8b".into(),
        format!("{}/v1", server.uri()),
    );
    let resp = client.classify(&classify_req()).await.unwrap();
    assert_eq!(resp.path, vec!["NLP"]);
}

#[tokio::test]
async fn local_classify_retries_without_response_format() {
    let server = MockServer::start().await;
    // Older servers reject the response_format parameter outright.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(json!({"response_format": {"type": "json_schema"}})))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "message": "unknown field: response_format" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant",
                "content": "{\"path\": [\"NLP\"], \"is_new\": false, \"confidence\": 0.6, \"rationale\": \"ok\"}" } }]
        })))
        .mount(&server)
        .await;

    let client = OpenAiCompatClient::new(
        None,
        "llama3.1:8b".into(),
        format!("{}/v1", server.uri()),
    );
    let resp = client.classify(&classify_req()).await.unwrap();
    assert_eq!(resp.path, vec!["NLP"]);
}

#[tokio::test]
async fn local_chat_streams_deltas_and_done() {
    let server = MockServer::start().await;
    let sse = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Loc\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"al!\"}}]}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["stream"], true);
            assert_eq!(body["messages"][0]["role"], "system");
            ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream")
        })
        .mount(&server)
        .await;

    let client = OpenAiCompatClient::new(
        None,
        "llama3.1:8b".into(),
        format!("{}/v1", server.uri()),
    );
    let mut deltas: Vec<String> = Vec::new();
    let full = client
        .chat_stream("SYSTEM", &chat_history(), &mut |t| deltas.push(t.into()))
        .await
        .unwrap();
    assert_eq!(full, "Local!");
    assert_eq!(deltas, vec!["Loc".to_string(), "al!".to_string()]);
}

#[tokio::test]
async fn local_unknown_model_hints_ollama_pull() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error": { "message": "model 'nope' not found" }
        })))
        .mount(&server)
        .await;

    let client = OpenAiCompatClient::new(None, "nope".into(), format!("{}/v1", server.uri()));
    let err = client.test_key().await.unwrap_err();
    assert!(err.to_string().contains("ollama pull"), "got: {err}");
}
