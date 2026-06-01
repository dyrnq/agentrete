use crate::storage::Store;
use rmcp::{handler::server::ServerHandler, model::*, service::serve_server};
use std::sync::Arc;

pub struct AgentreteServer {
    pub store: Arc<Store>,
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
impl ServerHandler for AgentreteServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(include_str!("../mcp/protocol.rs"))
    }
}

pub async fn run(store: Store) -> anyhow::Result<()> {
    let server = AgentreteServer {
        store: Arc::new(store),
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
    let svc_config = StreamableHttpServerConfig::default();

    let service = StreamableHttpService::new(
        move || Ok(AgentreteServer {
            store: store.clone(),
        }),
        session_manager,
        svc_config,
    );

    let app = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
