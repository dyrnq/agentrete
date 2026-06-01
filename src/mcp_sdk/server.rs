use crate::storage::Store;
use rmcp::{handler::server::{router::tool::ToolRouter, ServerHandler}, model::*, service::serve_server, tool, tool_handler, tool_router};
use std::sync::Arc;


// ── Tool parameter structs ──────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct MemorySearchParams {
    #[schemars(description = "Search query")]
    pub query: String,
    #[schemars(description = "Max results, default 10")]
    pub limit: Option<u8>,
    #[schemars(description = "Filter by memory type")]
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct MemorySaveParams {
    #[schemars(description = "Memory content")]
    pub content: String,
    #[schemars(description = "Type: rule/decision/pattern/bug/fact")]
    pub r#type: Option<String>,
    #[schemars(description = "Comma-separated tags")]
    pub tags: Option<String>,
    #[schemars(description = "Source file path")]
    pub source_file: Option<String>,
    #[schemars(description = "Project name (auto-detected if empty)")]
    pub project: Option<String>,
    #[schemars(description = "Preview only, don't save")]
    pub dry_run: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct MemoryListParams {
    #[schemars(description = "Max results")]
    pub limit: Option<u8>,
    #[schemars(description = "Filter by type")]
    pub r#type: Option<String>,
    #[schemars(description = "Offset for pagination")]
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct MemoryForgetParams {
    #[schemars(description = "Memory ID")]
    pub id: String,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct MemoryCompactParams {
    #[schemars(description = "Mode: exact or semantic")]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct KgScanParams {
    #[schemars(description = "Project path")]
    pub path: String,
    #[schemars(description = "Force re-scan (clear cache)")]
    pub force: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct KgQueryParams {
    #[schemars(description = "Mode: neighbors, path, subgraph")]
    pub mode: String,
    #[schemars(description = "Entity to query")]
    pub entity: Option<String>,
    #[schemars(description = "Target entity (for path mode)")]
    pub target: Option<String>,
    #[schemars(description = "Filter by predicate")]
    pub predicate: Option<String>,
    #[schemars(description = "Direction: outgoing, incoming, both")]
    pub direction: Option<String>,
    #[schemars(description = "Filter by project")]
    pub project: Option<String>,
}


pub struct AgentreteServer {
    pub store: Arc<Store>,
    tool_router: ToolRouter<Self>,
}

// ── Tool: memory_stats ──────────────────────────────────────────────────────
#[rmcp::tool_router(router = tool_router)]
impl AgentreteServer {
    #[rmcp::tool(name = "memory_stats", description = "Database statistics")]
    async fn memory_stats(&self) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
        let stats = self.store.stats().await.map_err(|e| {
            rmcp::ErrorData::new(
                rmcp::model::ErrorCode::INTERNAL_ERROR,
                format!("Stats failed: {e}"),
                None,
            )
        })?;
        let text = format!(
            "Memories: {} ({})\nModel: {}\nVec0: {}\nSessions: {}\nObs: {}\nDB: {:.1}MB",
            stats.memory_count,
            stats.with_embedding,
            stats.model_info.as_deref().unwrap_or("none"),
            if stats.vec0_enabled {
                "enabled"
            } else {
                "disabled"
            },
            stats.session_count,
            stats.observation_count,
            stats.db_size_bytes as f64 / (1024.0 * 1024.0),
        );
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}

// ── ServerHandler ───────────────────────────────────────────────────────────
#[tool_handler(router = self.tool_router)]
impl ServerHandler for AgentreteServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(include_str!("../mcp/protocol.rs"))
    }
}

pub async fn run(store: Store) -> anyhow::Result<()> {
    let server = AgentreteServer {
        store: Arc::new(store),
        tool_router: AgentreteServer::tool_router(),
    };
    log::info!("agentrete MCP (rmcp SDK, stdio)");
    let svc = serve_server(server, rmcp::transport::io::stdio()).await?;
    svc.waiting().await?;
    Ok(())
}

pub async fn run_http(store: Store, config: &crate::config::Config) -> anyhow::Result<()> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig,
        StreamableHttpService,
    };
    use std::sync::Arc;

    let store = Arc::new(store);
    let port = config.port;
    log::info!("agentrete MCP (rmcp SDK, http) on 127.0.0.1:{}", port);

    // Session manager: in-memory (stateless for our use case)
    let session_manager = Arc::new(LocalSessionManager::default());
    let svc_config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true);

    let service = StreamableHttpService::new(
        move || Ok(AgentreteServer {
            store: store.clone(),
            tool_router: AgentreteServer::tool_router(),
        }),
        session_manager,
        svc_config,
    );

    let app = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
