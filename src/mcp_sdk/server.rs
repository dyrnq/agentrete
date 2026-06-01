//! MCP SDK server — thin adapter layer over `tools/` business logic.
//! Each `#[tool]` method delegates to pure `tools::*` functions and
//! converts `anyhow::Result` into rmcp-native `CallToolResult`.

use crate::storage::Store;
use rmcp::{
    handler::server::wrapper::Parameters, model::*, service::serve_server, tool, tool_handler,
    tool_router, ErrorData as McpError, ServerHandler,
};
use serde::Deserialize;
use std::sync::Arc;

// ── Tool parameter structs ──────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct MemorySearchParams {
    pub query: String,
    pub limit: Option<u8>,
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct MemorySaveParams {
    pub content: String,
    pub r#type: Option<String>,
    pub tags: Option<String>,
    pub source_file: Option<String>,
    pub project: Option<String>,
    pub dry_run: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct MemoryListParams {
    pub limit: Option<u8>,
    pub r#type: Option<String>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct MemoryForgetParams {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct MemoryCompactParams {
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct KgScanParams {
    pub path: String,
    pub force: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct KgQueryParams {
    pub mode: String,
    pub entity: Option<String>,
    pub target: Option<String>,
    pub predicate: Option<String>,
    pub direction: Option<String>,
    pub project: Option<String>,
}

// ── Server state ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AgentreteServer {
    store: Arc<Store>,
}

impl AgentreteServer {
    pub fn new(store: Store) -> Self {
        Self {
            store: Arc::new(store),
        }
    }
}

// ── Tool router block (rmcp generates tool definitions from these) ──────────
#[tool_router]
impl AgentreteServer {
    #[tool(description = "Semantic search with RRF fusion (vec0 KNN + FTS5 BM25)")]
    async fn memory_search(
        &self,
        p: Parameters<MemorySearchParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let params = p.0;
        let result = crate::mcp_sdk::tools::memory::memory_search(
            &self.store,
            &params.query,
            params.limit.unwrap_or(10),
            params.r#type.as_deref(),
        )
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        serde_json::from_value(result).map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(description = "Save with optional dry_run preview, auto-detects project from git")]
    async fn memory_save(
        &self,
        p: Parameters<MemorySaveParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let params = p.0;
        let result = crate::mcp_sdk::tools::memory::memory_save(
            &self.store,
            &params.content,
            params.r#type.as_deref(),
            params.tags.as_deref(),
            params.project.as_deref(),
            params.source_file.as_deref(),
            params.dry_run.unwrap_or(false),
        )
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        serde_json::from_value(result).map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(description = "List memories, optionally filtered by type")]
    async fn memory_list(
        &self,
        p: Parameters<MemoryListParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let params = p.0;
        let result = crate::mcp_sdk::tools::memory::memory_list(
            &self.store,
            params.limit.unwrap_or(20),
            params.r#type.as_deref(),
            params.offset.unwrap_or(0),
        )
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        serde_json::from_value(result).map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(description = "Delete")]
    async fn memory_forget(
        &self,
        p: Parameters<MemoryForgetParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let result = crate::mcp_sdk::tools::memory::memory_forget(&self.store, &p.0.id)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        serde_json::from_value(result).map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(description = "Database statistics")]
    async fn memory_stats(&self) -> std::result::Result<CallToolResult, McpError> {
        let result = crate::mcp_sdk::tools::memory::memory_stats(&self.store)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        serde_json::from_value(result).map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(description = "Deduplicate memories (exact or semantic) and reclaim disk space")]
    async fn memory_compact(
        &self,
        p: Parameters<MemoryCompactParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let mode = p.0.mode.as_deref().unwrap_or("exact");
        let result = crate::mcp_sdk::tools::memory::memory_compact(&self.store, mode)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        serde_json::from_value(result).map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(description = "Scan codebase with optional file watching")]
    async fn kg_scan(
        &self,
        p: Parameters<KgScanParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let params = p.0;
        let result = crate::mcp_sdk::tools::kg::kg_scan(
            &self.store,
            &params.path,
            params.force.unwrap_or(false),
        )
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        serde_json::from_value(result).map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(description = "Check if a background scan is running")]
    async fn kg_scan_status(&self) -> std::result::Result<CallToolResult, McpError> {
        let result = crate::mcp_sdk::tools::kg::kg_scan_status(&self.store)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        serde_json::from_value(result).map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(description = "Query knowledge graph: neighbors, shortest path, or subgraph")]
    async fn kg_query(
        &self,
        p: Parameters<KgQueryParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let params = p.0;
        let result = crate::mcp_sdk::tools::kg::kg_query(
            &self.store,
            &params.mode,
            params.entity.as_deref(),
            params.target.as_deref(),
            params.predicate.as_deref(),
            params.direction.as_deref(),
            params.project.as_deref(),
        )
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        serde_json::from_value(result).map_err(|e| McpError::internal_error(e.to_string(), None))
    }
}

// ── ServerHandler trait implementation ──────────────────────────────────────
#[tool_handler]
impl ServerHandler for AgentreteServer {
    fn get_info(&self) -> ServerInfo {
        let version = env!("CARGO_PKG_VERSION");
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("agentrete", version))
            .with_instructions(include_str!("../mcp/protocol.rs"))
    }
}

// ── Transport runners ───────────────────────────────────────────────────────

pub async fn run(store: Store) -> anyhow::Result<()> {
    log::info!("agentrete MCP (rmcp SDK, stdio)");
    let svc = serve_server(AgentreteServer::new(store), rmcp::transport::io::stdio()).await?;
    svc.waiting().await?;
    Ok(())
}

pub async fn run_http(store: Store, config: &crate::config::Config) -> anyhow::Result<()> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };

    let port = config.port;
    let state = AgentreteServer::new(store);
    log::info!("agentrete MCP (rmcp SDK, http) on 127.0.0.1:{port}");

    let session_manager = Arc::new(LocalSessionManager::default());
    let svc_config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true);

    let service =
        StreamableHttpService::new(move || Ok(state.clone()), session_manager, svc_config);

    let app = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
