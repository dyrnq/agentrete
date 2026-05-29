use crate::storage::Store;
use axum::{extract::State, routing::post, Router};
use serde_json::Value;
use std::sync::Arc;

use super::handlers::handle_rpc;

pub async fn run_http(store: Store, config: &crate::config::Config) -> anyhow::Result<()> {
    let port = config.port;
    eprintln!("agentrete MCP server on http://127.0.0.1:{}", port);
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
    let result = handle_rpc(
        &store,
        request["method"].as_str().unwrap_or(""),
        &request["params"],
    )
    .await;
    axum::Json(result)
}

async fn http_health() -> axum::Json<Value> {
    axum::Json(serde_json::json!({
        "service":"agentrete",
        "status":"ok",
        "version":env!("CARGO_PKG_VERSION")
    }))
}
