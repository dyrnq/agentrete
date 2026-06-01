use crate::storage::Store;
use axum::{extract::State, routing::post, Router};

use serde_json::Value;
use std::sync::Arc;
use std::sync::OnceLock;

use super::handlers::handle_rpc;

static START_TIME: OnceLock<std::time::Instant> = OnceLock::new();

pub async fn run_http(store: Store, config: &crate::config::Config) -> anyhow::Result<()> {
    let port = config.port;
    log::info!("agentrete MCP server on http://127.0.0.1:{}", port);
    let state = Arc::new(store);
    let app = Router::new()
        .route("/", post(http_mcp_handler).get(http_health))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn http_mcp_handler(
    State(store): State<Arc<Store>>,
    body: String,
) -> impl axum::response::IntoResponse {
    let request: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    // Inject top-level id into params so handle_rpc can return it
    let mut req_params = request.get("params").cloned().unwrap_or(serde_json::Value::Null);
    if let Some(id) = request.get("id") {
        if !id.is_null() {
            if let Some(obj) = req_params.as_object_mut() {
                obj.insert("id".to_string(), id.clone());
            } else {
                let mut obj = serde_json::Map::new();
                obj.insert("id".to_string(), id.clone());
                obj.insert("params".to_string(), req_params);
                req_params = serde_json::Value::Object(obj);
            }
        }
    }
    let result = handle_rpc(
        &store,
        request["method"].as_str().unwrap_or(""),
        &req_params,
    )
    .await;
    axum::Json(result)
}

async fn http_health() -> axum::Json<Value> {
    let start = START_TIME.get_or_init(std::time::Instant::now);
    let uptime = start.elapsed().as_secs();
    axum::Json(serde_json::json!({
        "service": "agentrete",
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "transport": "http",
        "pid": std::process::id(),
        "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        "uptime_secs": uptime
    }))
}
