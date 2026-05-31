// agentrete — Local-first persistent memory for AI coding agents
// See docs/memory-module-design.md for the full architecture.

use clap::{Parser, Subcommand};

mod cli;
mod config;
mod embed;
mod knowledge_graph;
mod mcp;
mod storage;
mod types;

#[derive(Parser)]
#[command(
    name = "agentrete",
    about = "Local-first persistent memory for AI coding agents",
    version = "0.1.0"
)]
struct Cli {
    /// Path to config file (TOML format)
    #[arg(short = 'c', long, global = true)]
    config: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Save a memory
    Save {
        /// Memory content
        content: String,
        /// Memory type: decision|pattern|bug|architecture|workflow|fact
        #[arg(short, long)]
        r#type: Option<String>,
        /// Tags (comma-separated)
        #[arg(short = 'g', long)]
        tags: Option<String>,
        /// Related files (comma-separated)
        #[arg(short, long)]
        files: Option<String>,
        /// Project name
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Search memories
    Search {
        /// Search query
        query: String,
        /// Maximum results (default: 5)
        #[arg(short, long, default_value = "5")]
        limit: u8,
        /// Memory type filter
        #[arg(short, long)]
        r#type: Option<String>,
    },
    /// List recent memories
    List {
        /// Maximum results (default: 10)
        #[arg(short, long, default_value = "10")]
        limit: u8,
    },
    /// Show memory statistics
    Stats,
    /// Delete a memory by ID
    Forget {
        /// Memory ID to delete
        id: String,
    },
    /// Delete all memories (with confirmation)
    Wipe {
        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },
    /// Seed community rules (idempotent, skips existing)
    Seed,
    /// Initialize project: create data directory and test connection
    Init,
    /// Run diagnostics and health checks
    Doctor,
    /// Interactive setup: auto-detect AI tools and configure MCP + hooks
    Setup,

    /// Install/Uninstall/Status as OS-native background service (systemd/launchd/autostart)
    Daemon {
        /// Action: install, uninstall, or status
        #[arg(value_enum)]
        action: String,
        /// Port for the HTTP server (default: 9092)
        #[arg(short, long, default_value = "9092")]
        port: u16,
        /// Path to the agentrete binary (auto-detected if omitted)
        #[arg(long)]
        binary: Option<String>,
    },

    /// Start MCP server (stdio transport for Codex CLI)
    Mcp {
        /// Optional: run as Streamable HTTP server on this port
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// Scan a codebase and build knowledge graph
    Scan {
        /// Path to scan (default: current directory)
        path: Option<String>,
    },
    /// Download an embedding model from HuggingFace mirror
    InstallModel {
        /// Model ID (e.g. moka-ai/m3e-base)
        #[arg(default_value = "moka-ai/m3e-base")]
        model: String,
        /// HuggingFace endpoint (default: https://hf-mirror.com)
        #[arg(long, default_value = "https://hf-mirror.com")]
        endpoint: String,
        /// Model revision (default: main)
        #[arg(long, default_value = "main")]
        revision: String,
    },
}

fn run_install_model(model_id: &str, endpoint: &str, revision: &str) -> anyhow::Result<()> {
    use std::io::Write;

    let home = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let cache_dir = home
        .join(".cache/huggingface/hub")
        .join(format!("models--{}", model_id.replace('/', "--")))
        .join("snapshots")
        .join(revision);

    std::fs::create_dir_all(&cache_dir)?;

    let client = reqwest::blocking::Client::new();
    let api_url = format!("{}/api/models/{}", endpoint.trim_end_matches('/'), model_id);

    // Get file list
    let json: serde_json::Value = client
        .get(&api_url)
        .header(
            "User-Agent",
            concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")),
        )
        .send()?
        .json()?;

    let siblings = json["siblings"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Invalid HF API response"))?;

    let required = [
        "config.json",
        "tokenizer.json",
        "model.safetensors",
        "tokenizer_config.json",
        "special_tokens_map.json",
        "vocab.txt",
        "modules.json",
        "config_sentence_transformers.json",
        "sentence_bert_config.json",
    ];

    let existing: std::collections::HashSet<&str> = siblings
        .iter()
        .filter_map(|s| s["rfilename"].as_str())
        .collect();

    let mut downloaded = 0u32;
    let mut skipped = 0u32;

    for filename in &required {
        if !existing.contains(filename) {
            println!("  [SKIP] {} (not in repo)", filename);
            skipped += 1;
            continue;
        }

        let target = cache_dir.join(filename);
        if target.exists() && target.metadata().map(|m| m.len() > 50).unwrap_or(false) {
            let size = target.metadata().map(|m| m.len()).unwrap_or(0);
            println!("  [SKIP] {} ({})", filename, human_size(size));
            skipped += 1;
            continue;
        }

        let file_url = format!(
            "{}/{}/resolve/{}/{}",
            endpoint.trim_end_matches('/'),
            model_id,
            revision,
            filename
        );
        print!("  [DOWNLOAD] {} ... ", filename);
        std::io::stdout().flush().ok();

        match client.get(&file_url).send() {
            Ok(resp) => {
                let bytes = resp.bytes()?;
                if bytes.len() < 50 {
                    println!("FAILED (empty)");
                    continue;
                }
                std::fs::write(&target, &bytes)?;
                println!("{}", human_size(bytes.len() as u64));
                downloaded += 1;
            }
            Err(e) => println!("FAILED ({})", e),
        }
    }

    println!();
    println!("Done: {} downloaded, {} skipped", downloaded, skipped);
    println!("Cache: {}", cache_dir.display());
    Ok(())
}

const SEED_RULES: &[(&str, &str, &str)] = &[
    ("Think Before Coding: State assumptions, surface tradeoffs, don't hide confusion", "rule", "karpathy,coding"),
    ("Simplicity First: Minimum code, no speculative features, no unrequested abstractions", "rule", "karpathy,coding"),
    ("Surgical Changes: Minimal edits, preserve existing code style", "rule", "karpathy,coding"),
    ("Goal-Driven Execution: Close open loops, verify each step", "rule", "karpathy,coding"),
    ("Systematic Debugging: Identify root cause before fixing, create minimal reproduction, test hypothesis", "rule", "superpowers,debugging"),
    ("Verification Before Completion: Run tests/checks before claiming work is done, evidence over assertions", "rule", "superpowers,workflow"),
    ("Test-Driven Development: Write failing test first, then implement, ensure all pass before commit", "rule", "superpowers,tdd"),
    ("Writing Plans: Break multi-step work into atomic tasks with clear verification criteria", "rule", "superpowers,planning"),
    ("Surgical Changes Only: Change only what's needed, don't refactor in passing", "rule", "superpowers,coding"),
    ("Subagent-Driven Dev: Fan out parallel work to subagents when tasks are independent", "rule", "superpowers,parallel"),
    // Code modification rules (CRITICAL)
    ("Code Modification: NEVER use sed or python3 to modify source code — high failure rate, silently corrupts code", "rule", "coding,code-modification,CRITICAL"),
    ("Code Modification: Use apply_patch (Unified Diff) as the only legal way to modify source files — context-line validation catches errors", "rule", "coding,code-modification,CRITICAL"),
    ("Code Modification: If apply_patch is unavailable, rewrite the entire file instead of patching", "rule", "coding,code-modification,CRITICAL"),
    ("Doc Paths: Never use private paths (like ~/user or 192.168.x.x) in documentation — use ~ or localhost", "rule", "coding,docs,CRITICAL"),
    ("AST Tools: Prefer ast-grep for code navigation and refactoring — saves tokens vs reading whole files", "rule", "coding,tools"),
    ("Validation: After any code change, run in order: cargo fmt -> cargo clippy --all-targets -- -D warnings -> cargo build", "rule", "coding,validation,CRITICAL"),
    ("Validation: On any validation failure, revert the change immediately before debugging", "rule", "coding,validation,CRITICAL"),
];

fn main() -> anyhow::Result<()> {
    // Initialize tracing (simple stderr logger for migration messages)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    // Catch panics in spawned tasks so they don't abort the MCP server
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("[agentrete] PANIC (non-fatal): {}", info);
        default_hook(info);
    }));

    let cli = Cli::parse();
    // Extract MCP port if present (needed for Config before store init)
    let mcp_port = if let Commands::Mcp { port } = &cli.command {
        *port
    } else {
        None
    };
    let cfg = crate::config::Config::load(mcp_port, cli.config.as_deref());

    // Commands that don't need the database
    if let Commands::Setup = &cli.command {
        return cli::setup_wizard::run();
    }

    if let Commands::Daemon {
        action,
        port,
        binary,
    } = &cli.command
    {
        let bin = binary.clone().unwrap_or_else(|| {
            std::env::current_exe()
                .map(|p| p.to_string_lossy().into())
                .unwrap_or_else(|_| env!("CARGO_PKG_NAME").into())
        });
        return cli::daemon::run(action, *port, &bin);
    }

    if let Commands::InstallModel {
        model,
        endpoint,
        revision,
    } = &cli.command
    {
        return run_install_model(model, endpoint, revision);
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main(cli, cfg))
}

async fn async_main(cli: Cli, cfg: crate::config::Config) -> anyhow::Result<()> {
    let embedder = if cfg.embedding.backend != crate::config::EmbeddingBackend::None {
        eprintln!(
            "Loading embedding model (backend={:?})...",
            cfg.embedding.backend
        );
        let emb = crate::embed::embeddings::Embedder::from_config(&cfg.embedding);
        match emb {
            Ok(e) => {
                eprintln!("Embedding model loaded.");
                Some(e)
            }
            Err(e) => {
                eprintln!("Warning: embedding model failed: {e}");
                None
            }
        }
    } else {
        None
    };
    let store = storage::Store::open(&cfg, embedder).await?;

    // Warn if kg is enabled but ast-grep (sg) is not installed
    if cfg.knowledge_graph.enabled
        && std::process::Command::new("sg")
            .arg("--version")
            .output()
            .is_err()
    {
        log::warn!(
            "knowledge_graph is enabled but ast-grep (sg) not found. Run: cargo install ast-grep"
        );
    }

    match cli.command {
        Commands::Save {
            ref content,
            ref r#type,
            ref tags,
            ref files,
            ref project,
        } => {
            cli::memory::cmd_save(
                &store,
                content.clone(),
                r#type.clone(),
                tags.clone(),
                files.clone(),
                project.clone(),
            )
            .await?;
        }
        Commands::Search {
            ref query,
            limit,
            ref r#type,
        } => {
            cli::memory::cmd_search(&store, query.clone(), limit, r#type.clone()).await?;
        }
        Commands::List { limit } => {
            cli::memory::cmd_list(&store, limit).await?;
        }
        Commands::Stats => {
            cli::memory::cmd_stats(&store).await?;
        }
        Commands::Init => {
            cli::scan::cmd_init(&store).await?;
        }
        Commands::Scan { ref path } => {
            cli::scan::cmd_scan(&store, path.clone().unwrap_or_default()).await?;
        }
        Commands::Doctor => {
            cli::scan::cmd_doctor(&store).await?;
        }
        Commands::Forget { ref id } => {
            cli::memory::cmd_forget(&store, id.clone()).await?;
        }
        Commands::Wipe { force } => {
            cli::memory::cmd_wipe(&store, force).await?;
        }
        Commands::Seed => {
            cli::seed::cmd_seed(&store).await?;
        }
        Commands::Mcp { port } => {
            let is_http = port.is_some();
            let store_for_shutdown = store.clone();
            let embed_handle =
                if is_http && cfg.embedding.backend != crate::config::EmbeddingBackend::None {
                    let embedder = crate::embed::embeddings::Embedder::from_config(&cfg.embedding)?;
                    let store2 = store.clone();
                    let (model, dims) = match cfg.embedding.backend {
                        crate::config::EmbeddingBackend::Remote => (
                            cfg.embedding
                                .remote
                                .model
                                .clone()
                                .unwrap_or_else(|| "unknown".into()),
                            cfg.embedding.remote.dims.unwrap_or(768) as usize,
                        ),
                        crate::config::EmbeddingBackend::Model2Vec => (
                            format!(
                                "model2vec:{}:{}d",
                                cfg.embedding.model2vec.model, cfg.embedding.model2vec.dims
                            ),
                            cfg.embedding.model2vec.dims as usize,
                        ),
                        crate::config::EmbeddingBackend::None => (String::new(), 0),
                    };
                    Some(tokio::spawn(async move {
                        log::info!("embed-worker: started (identifier={model}, dims={dims})");
                        loop {
                            match store2
                                .embed_pending(&embedder, &model, dims, cfg.search.embed_batch)
                                .await
                            {
                                Ok(0) => {
                                    tokio::time::sleep(tokio::time::Duration::from_secs(
                                        cfg.search.embed_poll_secs,
                                    ))
                                    .await;
                                }
                                Ok(n) => {
                                    log::info!("embed-worker: flushed {n} vectors");
                                }
                                Err(e) => {
                                    log::info!("embed-worker: error flushing: {e}");
                                    tokio::time::sleep(tokio::time::Duration::from_secs(
                                        cfg.search.embed_retry_secs,
                                    ))
                                    .await;
                                }
                            }
                        }
                    }))
                } else {
                    None
                };

            let result = match port {
                Some(_) => crate::mcp::run_http(store, &cfg).await,
                None => {
                    log::info!(
                        "agentrete: stdio mode (embed worker disabled, use HTTP for embeddings)"
                    );
                    crate::mcp::run_stdio(store).await
                }
            };

            if let Some(h) = embed_handle {
                h.abort();
            }

            result?;
            store_for_shutdown.shutdown().await;
            return Ok(());
        }
        Commands::Daemon { .. } | Commands::Setup | Commands::InstallModel { .. } => {
            unreachable!("handled before store open")
        }
    }
    Ok(())
}

fn ask_confirmation(prompt: &str) -> bool {
    use std::io::Write;
    print!("{} [y/N] ", prompt);
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_ok() {
        matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
    } else {
        false
    }
}

fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{}B", bytes);
    }
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        return format!("{:.1}KB", kb);
    }
    let mb = kb / 1024.0;
    if mb < 1024.0 {
        return format!("{:.1}MB", mb);
    }
    format!("{:.2}GB", mb / 1024.0)
}
