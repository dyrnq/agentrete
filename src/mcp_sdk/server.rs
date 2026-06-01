use crate::storage::Store;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters, ServerHandler},
    model::*,
    service::serve_server,
    tool_handler,
};
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

// ── Tool implementations ────────────────────────────────────────────────────
#[rmcp::tool_router(router = tool_router)]
impl AgentreteServer {
    /// Semantic search with RRF fusion (vec0 KNN + FTS5 BM25)
    #[rmcp::tool(name = "memory_search")]
    async fn memory_search(
        &self,
        p: Parameters<MemorySearchParams>,
    ) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
        let params = p.0;
        let results = self
            .store
            .search(
                &params.query,
                params.limit.unwrap_or(10),
                params.r#type.as_deref(),
            )
            .await
            .map_err(|e| {
                rmcp::ErrorData::new(
                    rmcp::model::ErrorCode::INTERNAL_ERROR,
                    format!("Search failed: {e}"),
                    None,
                )
            })?;
        let items: Vec<Content> = results
            .into_iter()
            .map(|x| {
                let meta = format!(
                    "tags:{} imp:{} at:{} src:{} proj:{}",
                    x.tags.as_ref().map(|t| t.join(",")).unwrap_or_default(),
                    x.importance,
                    x.created_at,
                    x.source_file.as_deref().unwrap_or("?"),
                    x.project.as_deref().unwrap_or("?")
                );
                Content::text(format!(
                    "[{}] {} (score={:.2}) id={}  {}",
                    x.memory_type.as_deref().unwrap_or("-"),
                    x.content,
                    x.score,
                    x.id,
                    meta
                ))
            })
            .collect();
        Ok(CallToolResult::success(items))
    }

    /// Save with optional dry_run preview, auto-detects project from git
    #[rmcp::tool(name = "memory_save")]
    async fn memory_save(
        &self,
        p: Parameters<MemorySaveParams>,
    ) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
        let params = p.0;
        if params.dry_run.unwrap_or(false) {
            let preview_id = format!("mem_{}", uuid::Uuid::new_v4());
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "Preview: {} (not saved)",
                preview_id
            ))]));
        }
        let tags = params
            .tags
            .as_ref()
            .map(|s| s.split(",").map(|t| t.trim().to_string()).collect());
        let project = params.project.or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "--show-toplevel"])
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        std::path::Path::new(std::str::from_utf8(&o.stdout).ok()?.trim())
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                    } else {
                        None
                    }
                })
        });
        let id = self
            .store
            .save(crate::types::NewMemory {
                content: params.content,
                memory_type: params.r#type,
                tags,
                files: None,
                project,
                source_file: params.source_file,
            })
            .await
            .map_err(|e| {
                rmcp::ErrorData::new(
                    rmcp::model::ErrorCode::INTERNAL_ERROR,
                    format!("Save failed: {e}"),
                    None,
                )
            })?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Saved: {}",
            id
        ))]))
    }

    /// List memories, optionally filtered by type
    #[rmcp::tool(name = "memory_list")]
    async fn memory_list(
        &self,
        p: Parameters<MemoryListParams>,
    ) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
        let params = p.0;
        let memories = self
            .store
            .list(
                params.limit.unwrap_or(20),
                params.r#type.as_deref(),
                params.offset.unwrap_or(0),
            )
            .await
            .map_err(|e| {
                rmcp::ErrorData::new(
                    rmcp::model::ErrorCode::INTERNAL_ERROR,
                    format!("List failed: {e}"),
                    None,
                )
            })?;
        let items: Vec<Content> = memories
            .into_iter()
            .map(|m| {
                let meta = format!(
                    "tags:{} imp:{} at:{} src:{} proj:{}",
                    m.tags.as_ref().map(|t| t.join(",")).unwrap_or_default(),
                    m.importance,
                    m.created_at,
                    m.source_file.as_deref().unwrap_or("?"),
                    m.project.as_deref().unwrap_or("?")
                );
                Content::text(format!(
                    "[{}] {} id={}  {}",
                    m.memory_type.as_deref().unwrap_or("-"),
                    m.content,
                    m.id,
                    meta
                ))
            })
            .collect();
        Ok(CallToolResult::success(items))
    }

    /// Delete
    #[rmcp::tool(name = "memory_forget")]
    async fn memory_forget(
        &self,
        p: Parameters<MemoryForgetParams>,
    ) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
        let id = p.0.id;
        self.store.forget(&id).await.map_err(|e| {
            rmcp::ErrorData::new(
                rmcp::model::ErrorCode::INTERNAL_ERROR,
                format!("Forget failed: {e}"),
                None,
            )
        })?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Deleted: {}",
            id
        ))]))
    }

    /// Database statistics
    #[rmcp::tool(name = "memory_stats")]
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

    /// Deduplicate memories (exact or semantic) and reclaim disk space
    #[rmcp::tool(name = "memory_compact")]
    async fn memory_compact(
        &self,
        p: Parameters<MemoryCompactParams>,
    ) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
        let mode = p.0.mode.as_deref().unwrap_or("exact");
        let (removed, remaining) = self.store.compact(mode, 0.95).await.map_err(|e| {
            rmcp::ErrorData::new(
                rmcp::model::ErrorCode::INTERNAL_ERROR,
                format!("Compact failed: {e}"),
                None,
            )
        })?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Compacted ({mode}): {} duplicates removed, {} memories remain.",
            removed, remaining
        ))]))
    }

    /// Scan codebase with optional file watching
    #[rmcp::tool(name = "kg_scan")]
    async fn kg_scan(
        &self,
        p: Parameters<KgScanParams>,
    ) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
        let params = p.0;
        if !self.store.graph.is_enabled() {
            return Err(rmcp::ErrorData::new(
                rmcp::model::ErrorCode::INTERNAL_ERROR,
                "Knowledge graph is disabled.",
                None,
            ));
        }
        if self
            .store
            .scan_running
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(rmcp::ErrorData::new(
                rmcp::model::ErrorCode::INTERNAL_ERROR,
                "A scan is already in progress.",
                None,
            ));
        }
        let store = self.store.clone();
        let path = params.path.clone();
        let force = params.force.unwrap_or(false);
        let (task_id, cancel_flag, _notify) = store.tasks.register().await;
        let task_id_clone = task_id.clone();
        let cancel_clone = cancel_flag.clone();
        store
            .scan_running
            .store(true, std::sync::atomic::Ordering::Release);
        tokio::spawn(async move {
            if force {
                let project = crate::storage::detect_project_for_scan(std::path::Path::new(&path));
                let branch = crate::storage::detect_git_branch(std::path::Path::new(&path));
                let _ = store.clear_kg(&project, &branch).await;
            }
            let result = match store.scan_codebase(std::path::Path::new(&path)).await {
                Ok((syms, rels)) => {
                    serde_json::json!({"ok": true, "symbols": syms, "relations": rels, "message": format!("Scanned {} symbols, {} relations", syms, rels)})
                }
                Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
            };
            store
                .scan_running
                .store(false, std::sync::atomic::Ordering::Release);
            if cancel_clone.load(std::sync::atomic::Ordering::Acquire) == 0 {
                store.tasks.complete(&task_id_clone, result).await;
            }
        });
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Task {}: scan started.",
            task_id
        ))]))
    }

    /// Check if a background scan is running
    #[rmcp::tool(name = "kg_scan_status")]
    async fn kg_scan_status(&self) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
        let running = self
            .store
            .scan_running
            .load(std::sync::atomic::Ordering::Acquire);
        let text = if running {
            "Scan is running..."
        } else {
            "No scan running. Run kg_scan to start one."
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    /// Query knowledge graph: neighbors, shortest path, or subgraph
    #[rmcp::tool(name = "kg_query")]
    async fn kg_query(
        &self,
        p: Parameters<KgQueryParams>,
    ) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
        let params = p.0;
        if !self.store.graph.is_enabled() {
            return Err(rmcp::ErrorData::new(
                rmcp::model::ErrorCode::INTERNAL_ERROR,
                "Knowledge graph is disabled.",
                None,
            ));
        }
        let project = params.project.as_deref();
        let direction = params.direction.as_deref().unwrap_or("both");
        let entity = params.entity.as_deref().unwrap_or("");
        let text = match params.mode.as_str() {
            "neighbors" | "subgraph" => {
                if entity.is_empty() {
                    return Err(rmcp::ErrorData::new(
                        rmcp::model::ErrorCode::INVALID_PARAMS,
                        "kg_query mode='neighbors' requires 'entity'",
                        None,
                    ));
                }
                let results = self
                    .store
                    .kg_query_neighbors(entity, params.predicate.as_deref(), direction, project)
                    .await
                    .map_err(|e| {
                        rmcp::ErrorData::new(
                            rmcp::model::ErrorCode::INTERNAL_ERROR,
                            format!("Query failed: {e}"),
                            None,
                        )
                    })?;
                if results.is_empty() {
                    format!("No relations found for '{entity}'")
                } else {
                    let mut s = format!("Relations for '{entity}':\n");
                    for (subj, rel, obj, conf) in &results {
                        s.push_str(&format!("  {subj} --[{rel}]--> {obj} (conf={conf})\n"));
                    }
                    s
                }
            }
            "path" => {
                if entity.is_empty() {
                    return Err(rmcp::ErrorData::new(
                        rmcp::model::ErrorCode::INVALID_PARAMS,
                        "kg_query mode='path' requires 'entity'",
                        None,
                    ));
                }
                let target = params.target.as_deref().unwrap_or("");
                if target.is_empty() {
                    return Err(rmcp::ErrorData::new(
                        rmcp::model::ErrorCode::INVALID_PARAMS,
                        "kg_query mode='path' requires 'target'",
                        None,
                    ));
                }
                match self.store.kg_query_path(entity, target, project).await {
                    Ok(Some(path)) => format!("Shortest path: {}", path.join(" -> ")),
                    Ok(None) => {
                        return Err(rmcp::ErrorData::new(
                            rmcp::model::ErrorCode::INTERNAL_ERROR,
                            format!("No path found between '{entity}' and '{target}'"),
                            None,
                        ))
                    }
                    Err(e) => {
                        return Err(rmcp::ErrorData::new(
                            rmcp::model::ErrorCode::INTERNAL_ERROR,
                            format!("Query failed: {e}"),
                            None,
                        ))
                    }
                }
            }
            other => {
                return Err(rmcp::ErrorData::new(
                    rmcp::model::ErrorCode::INVALID_PARAMS,
                    format!("kg_query: unknown mode '{other}' (try 'neighbors' or 'path')"),
                    None,
                ))
            }
        };
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
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };

    let store = Arc::new(store);
    let port = config.port;
    log::info!("agentrete MCP (rmcp SDK, http) on 127.0.0.1:{port}");

    let session_manager = Arc::new(LocalSessionManager::default());
    let svc_config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true);

    let service = StreamableHttpService::new(
        move || {
            Ok(AgentreteServer {
                store: store.clone(),
                tool_router: AgentreteServer::tool_router(),
            })
        },
        session_manager,
        svc_config,
    );

    let app = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
