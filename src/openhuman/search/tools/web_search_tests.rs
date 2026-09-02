use super::*;
use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};

fn tool() -> WebSearchTool {
    WebSearchTool::new(None, 5, 15)
}

async fn start_mock_backend(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://127.0.0.1:{}", addr.port())
}

#[test]
fn test_tool_name() {
    assert_eq!(tool().name(), "web_search_tool");
}

#[test]
fn test_tool_description() {
    assert!(tool().description().contains("backend search proxy"));
}

#[test]
fn test_parameters_schema() {
    let schema = tool().parameters_schema();
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"]["query"].is_object());
}

#[test]
fn test_parse_parallel_results_empty() {
    let result = tool()
        .parse_parallel_results(&[], "test query", "Exa")
        .unwrap();
    assert!(result.contains("No results found"));
    // A completed empty search is still attributed, so the timeline labels
    // the row instead of leaving it as in-progress (#5136).
    assert!(result.trim_end().ends_with("(via Exa)"));
}

#[test]
fn test_render_markdown_empty_carries_provider() {
    // The markdown rendering is what production shows, so its empty form
    // needs the marker too — and it must sit at the end of the line, where
    // the timeline parser looks for it.
    let result = tool().render_results_markdown(&[], "test query", "Exa");
    assert!(result.contains("No results"));
    assert!(result.trim_end().ends_with("(via Exa)"));
}

/// A minimal `SearchResponse` carrying only the provider under test, so
/// the resolution cases read without result/cost noise.
fn response_with_provider(provider: Option<&str>) -> SearchResponse {
    SearchResponse {
        search_id: "search-1".into(),
        results: vec![],
        cost_usd: 0.0,
        provider: provider.map(str::to_string),
    }
}

#[test]
fn test_resolve_managed_provider_defaults_to_exa() {
    // Backend omits the provider → fall back to the managed default.
    assert_eq!(
        resolve_managed_provider(&response_with_provider(None)),
        "Exa"
    );
    // Blank / whitespace-only provider is treated as absent.
    assert_eq!(
        resolve_managed_provider(&response_with_provider(Some("   "))),
        "Exa"
    );
}

#[test]
fn test_resolve_managed_provider_uses_backend_value() {
    // A provider named by the backend wins over the default and is trimmed,
    // so a future routing change surfaces without a code edit.
    assert_eq!(
        resolve_managed_provider(&response_with_provider(Some("  Brave  "))),
        "Brave"
    );
}

#[test]
fn test_parse_parallel_results_attribution_is_dynamic() {
    let results = vec![SearchResultItem {
        title: "T".into(),
        url: "https://t.com".into(),
        publish_date: None,
        excerpts: vec![],
    }];
    let exa = tool().parse_parallel_results(&results, "q", "Exa").unwrap();
    assert!(exa.contains("(via Exa)"));
    assert!(!exa.contains("via backend Parallel"));
    let brave = tool()
        .parse_parallel_results(&results, "q", "Brave")
        .unwrap();
    assert!(brave.contains("(via Brave)"));
}

#[test]
fn test_parse_parallel_results_with_data() {
    let results = vec![
        SearchResultItem {
            title: "Parallel AI Docs".into(),
            url: "https://docs.parallel.ai/home".into(),
            publish_date: None,
            excerpts: vec!["Parallel provides infrastructure for AI web search.".into()],
        },
        SearchResultItem {
            title: "Parallel Search Quickstart".into(),
            url: "https://docs.parallel.ai/search".into(),
            publish_date: Some("2024-01-01".into()),
            excerpts: vec!["Use POST /v1beta/search to retrieve results.".into()],
        },
    ];

    let result = tool()
        .parse_parallel_results(&results, "parallel ai", "Exa")
        .unwrap();
    assert!(result.contains("(via Exa)"));
    assert!(result.contains("Parallel AI Docs"));
    assert!(result.contains("https://docs.parallel.ai/home"));
    assert!(result.contains("Parallel Search Quickstart"));
    assert!(result.contains("Published: 2024-01-01"));
}

#[test]
fn test_parse_parallel_results_respects_max_results() {
    let tool = WebSearchTool::new(None, 2, 15);
    let results = vec![
        SearchResultItem {
            title: "Result 1".into(),
            url: "https://a.com".into(),
            publish_date: None,
            excerpts: vec![],
        },
        SearchResultItem {
            title: "Result 2".into(),
            url: "https://b.com".into(),
            publish_date: None,
            excerpts: vec![],
        },
        SearchResultItem {
            title: "Result 3".into(),
            url: "https://c.com".into(),
            publish_date: None,
            excerpts: vec![],
        },
    ];
    let result = tool.parse_parallel_results(&results, "q", "Exa").unwrap();
    assert!(result.contains("Result 1"));
    assert!(result.contains("Result 2"));
    assert!(!result.contains("Result 3"));
}

#[test]
fn test_parse_parallel_results_truncates_long_excerpt() {
    let long_excerpt = "x".repeat(600);
    let results = vec![SearchResultItem {
        title: "T".into(),
        url: "https://t.com".into(),
        publish_date: None,
        excerpts: vec![long_excerpt],
    }];
    let result = tool().parse_parallel_results(&results, "q", "Exa").unwrap();
    assert!(result.contains("..."));
    let excerpt_line = result.lines().find(|l| l.trim().starts_with('x')).unwrap();
    assert!(excerpt_line.trim().len() <= 503);
}

#[test]
fn test_web_search_truncation_utf8() {
    let excerpt = "🦀".repeat(600);
    let results = vec![SearchResultItem {
        title: "T".into(),
        url: "https://t.com".into(),
        publish_date: None,
        excerpts: vec![excerpt],
    }];
    let result = tool().parse_parallel_results(&results, "q", "Exa").unwrap();
    assert!(result.contains("..."));
    // Should have 500 crabs + "..."
    let excerpt_line = result.lines().find(|l| l.contains('🦀')).unwrap();
    assert_eq!(
        excerpt_line.trim().chars().filter(|c| *c == '🦀').count(),
        500
    );
}

#[tokio::test]
async fn test_execute_missing_query() {
    let result = tool().execute(json!({})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_execute_empty_query() {
    let result = tool().execute(json!({"query": ""})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_execute_without_backend_client() {
    let result = tool().execute(json!({"query": "test"})).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("backend session token"));
}

#[tokio::test]
async fn test_execute_posts_to_backend_and_renders_results() {
    #[derive(Clone)]
    struct MockState {
        called: Arc<AtomicBool>,
    }

    let state = MockState {
        called: Arc::new(AtomicBool::new(false)),
    };
    let called = Arc::clone(&state.called);
    let app = Router::new()
        .route(
            "/agent-integrations/parallel/search",
            post(
                |State(state): State<MockState>, Json(body): Json<Value>| async move {
                    state.called.store(true, Ordering::SeqCst);
                    assert_eq!(body["objective"], "test success");
                    assert_eq!(body["searchQueries"][0], "test success");
                    Json(json!({
                        "success": true,
                        "data": {
                            "searchId": "search-123",
                            "results": [
                                {
                                    "url": "https://example.com/result",
                                    "title": "Backend Search Result",
                                    "publish_date": "2026-04-20",
                                    "excerpts": ["Rendered excerpt from backend search."]
                                }
                            ],
                            "costUsd": 0.01
                        }
                    }))
                },
            ),
        )
        .with_state(state);

    let base_url = start_mock_backend(app).await;
    let client = Arc::new(IntegrationClient::new(base_url, "test-token".into()));
    let result = WebSearchTool::new(Some(client), 5, 15)
        .execute(json!({"query": "test success"}))
        .await
        .expect("execute() should return rendered backend results");

    assert!(called.load(Ordering::SeqCst));
    assert!(result.output().contains("Backend Search Result"));
    assert!(result.output().contains("https://example.com/result"));
    assert!(result
        .output()
        .contains("Rendered excerpt from backend search."));
    // Backend omitted a provider → attribution falls back to the managed
    // default (Exa) rather than the legacy "backend Parallel" wording.
    assert!(result.output().contains("(via Exa)"));
    assert!(!result.output().contains("backend Parallel"));
}

#[tokio::test]
async fn test_execute_attributes_backend_reported_provider() {
    // When the backend names the resolved provider, the tool result echoes
    // it verbatim — the attribution is dynamic, not a hardcoded "Exa".
    let app = Router::new().route(
        "/agent-integrations/parallel/search",
        post(|Json(_body): Json<Value>| async move {
            Json(json!({
                "success": true,
                "data": {
                    "searchId": "search-xyz",
                    "provider": "Brave",
                    "results": [
                        {
                            "url": "https://example.com/r",
                            "title": "Result",
                            "excerpts": ["Excerpt."]
                        }
                    ],
                    "costUsd": 0.01
                }
            }))
        }),
    );

    let base_url = start_mock_backend(app).await;
    let client = Arc::new(IntegrationClient::new(base_url, "test-token".into()));
    let result = WebSearchTool::new(Some(client), 5, 15)
        .execute(json!({"query": "anything"}))
        .await
        .expect("execute() should render backend results");

    assert!(result.output().contains("(via Brave)"));
    assert!(!result.output().contains("(via Exa)"));
}

#[tokio::test]
async fn test_execute_uses_direct_search_api_when_configured() {
    #[derive(Clone)]
    struct MockState {
        called: Arc<AtomicBool>,
    }

    let state = MockState {
        called: Arc::new(AtomicBool::new(false)),
    };
    let called = Arc::clone(&state.called);
    let app =
        Router::new()
            .route(
                "/search",
                post(
                    |State(state): State<MockState>,
                     headers: HeaderMap,
                     Json(body): Json<Value>| async move {
                        state.called.store(true, Ordering::SeqCst);
                        assert_eq!(
                            headers.get("x-api-key").and_then(|v| v.to_str().ok()),
                            Some("test-key")
                        );
                        assert_eq!(body["query"], "direct search");
                        Json(json!({
                            "documents": [
                                {
                                    "url": "https://example.com/direct",
                                    "title": "Direct Search Result",
                                    "content": "Rendered excerpt from direct search.",
                                    "published_date": "2026-04-21"
                                }
                            ]
                        }))
                    },
                ),
            )
            .with_state(state);

    let base_url = start_mock_backend(app).await;
    let result = WebSearchTool::new(None, 5, 15)
        .with_direct_search(Some(SeltzSearchTool::new(
            Some("test-key".into()),
            Some(base_url),
            5,
            15,
        )))
        .execute(json!({"query": "direct search"}))
        .await
        .expect("execute() should return rendered direct search results");

    assert!(called.load(Ordering::SeqCst));
    assert!(result.output().contains("via Seltz"));
    assert!(result.output().contains("Direct Search Result"));
    assert!(result.output().contains("https://example.com/direct"));
}
