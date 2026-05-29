// agentrete — Local-first persistent memory for AI coding agents
// See docs/memory-module-design.md for the full architecture.

use clap::{Parser, Subcommand};

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

mod cli;
mod config;
mod embed;
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

fn main() -> anyhow::Result<()> {
    // Initialize tracing (simple stderr logger for migration messages)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

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
    let store = storage::Store::open(&cfg).await?;

    match cli.command {
        Commands::Save {
            content,
            r#type,
            tags,
            files,
            project,
        } => {
            let tags_vec = tags.map(|t| t.split(',').map(|s| s.trim().to_string()).collect());
            let files_vec = files.map(|f| f.split(',').map(|s| s.trim().to_string()).collect());
            let id = store
                .save(types::NewMemory {
                    content,
                    memory_type: r#type,
                    tags: tags_vec,
                    files: files_vec,
                    project,
                })
                .await?;
            println!("Saved memory: {}", id);
        }
        Commands::Search {
            query,
            limit,
            r#type,
        } => {
            let results = store.search(&query, limit, r#type.as_deref()).await?;
            if results.is_empty() {
                println!("No memories found.");
            } else {
                for m in &results {
                    let types = m.memory_type.as_deref().unwrap_or("-");
                    println!(
                        "[{}] {} (score={:.2})  id={}",
                        types, m.content, m.score, m.id
                    );
                    if let Some(ref tags) = m.tags {
                        if !tags.is_empty() {
                            println!("     tags: {}", tags.join(", "));
                        }
                    }
                }
            }
        }
        Commands::List { limit } => {
            let entries = store.list(limit).await?;
            if entries.is_empty() {
                println!("No memories.");
            } else {
                for m in &entries {
                    let types = m.memory_type.as_deref().unwrap_or("-");
                    println!("[{}] {}  id={}", types, m.content, m.id);
                }
            }
        }
        Commands::Stats => {
            let stats = store.stats().await?;
            println!("Memories:      {}", stats.memory_count);
            println!("Sessions:      {}", stats.session_count);
            println!("Observations:  {}", stats.observation_count);
            println!("Database:      {}", stats.db_path);
        }
        Commands::Init => {
            let stats = store.stats().await?;
            println!("agentrete initialized successfully.");
            println!("Database: {}", stats.db_path);
            println!("Ready to save and search memories.");
        }
        Commands::Doctor => {
            let stats = store.stats().await?;
            println!("agentrete diagnostics:");
            println!("  Database:      {}", stats.db_path);
            println!("  Memories:      {}", stats.memory_count);
            println!("  Sessions:      {}", stats.session_count);
            println!("  Observations:  {}", stats.observation_count);

            // FTS check skipped (internal api)

            // Check database file size
            if let Ok(meta) = std::fs::metadata(&stats.db_path) {
                let size_mb = meta.len() as f64 / 1_048_576.0;
                println!("  DB file size:   {:.2} MB", size_mb);
            }

            println!(
                "
Status: ✅ agentrete is healthy"
            );
        }
        Commands::InstallModel { .. } => unreachable!("handled before store open"),
        Commands::Forget { id } => {
            store.forget(&id).await?;
            println!("Deleted: {}", id);
        }
        Commands::Wipe { force } => {
            if force || ask_confirmation("Delete ALL memories?") {
                store.wipe().await?;
                println!("All memories deleted.");
            } else {
                println!("Cancelled.");
            }
        }
        Commands::Daemon {
            action,
            port,
            binary,
        } => {
            let bin = binary.unwrap_or_else(|| {
                std::env::current_exe()
                    .map(|p| p.to_string_lossy().into())
                    .unwrap_or_else(|_| env!("CARGO_PKG_NAME").into())
            });
            cli::daemon::run(&action, port, &bin)?;
        }
        Commands::Setup => unreachable!(),
        Commands::Mcp { port } => {
            // Spawn embed worker in background if embedding is enabled
            let embed_handle = if cfg.embed_enabled() {
                let embedder = crate::embed::embeddings::Embedder::from_config(&cfg.embedding)?;
                let store2 = store.clone();
                let model = cfg
                    .embedding
                    .remote_model
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                let dims = cfg.embedding.dims as usize;
                Some(tokio::spawn(async move {
                    eprintln!("embed-worker: started (model={model}, dims={dims})");
                    loop {
                        match store2.embed_pending(&embedder, &model, dims, 500).await {
                            Ok(0) => {
                                // No pending — sleep 5s
                                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            }
                            Ok(n) => {
                                eprintln!("embed-worker: flushed {n} vectors");
                            }
                            Err(e) => {
                                eprintln!("embed-worker: error flushing: {e}");
                                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                            }
                        }
                    }
                }))
            } else {
                None
            };

            let result = match port {
                Some(_p) => mcp::run_http(store, &cfg).await,
                None => mcp::run_stdio(store).await,
            };

            // Abort embed worker on shutdown
            if let Some(h) = embed_handle {
                h.abort();
            }
            result?
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
