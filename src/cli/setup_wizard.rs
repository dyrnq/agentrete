//! Setup wizard — detects AI tools and configures MCP + hooks.
//! Supports: Codex CLI, Claude Code, Cursor, Zed, OpenCode, Windsurf, Goose, Gemini CLI.

use anyhow::Result;
use std::path::{Path, PathBuf};

use super::hooks;

const DEFAULT_PORT: u16 = 9092;

type ToolCheck = (&'static str, fn(&Path) -> bool, fn(&Path) -> bool, ToolKind);

/// Run the setup wizard.
pub fn run(force: bool) -> Result<()> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let bin = std::env::current_exe()
        .map(|p| p.to_string_lossy().into())
        .unwrap_or_else(|_| env!("CARGO_PKG_NAME").to_string());

    println!("Agentrete Setup Wizard");
    println!("  Binary: {}", bin);
    println!();

    let tools = detect(&home);
    if tools.is_empty() {
        println!("No supported AI tools detected.");
        println!(
            "Supported: Codex, Claude Code, Cursor, Zed, OpenCode, Windsurf, Goose, Gemini CLI"
        );
        return Ok(());
    }

    let mut configured = 0u32;
    for t in &tools {
        let status = if t.is_configured {
            "✓ (configured)"
        } else {
            "○"
        };
        println!("  {}  {}", status, t.name);
    }
    println!();

    for t in &tools {
        if t.is_configured && !force {
            continue;
        }
        configure_tool(t, &home, &bin)?;
        // Also install hooks where supported
        if let Err(e) = hooks::install(&t.kind, &home) {
            eprintln!("  ⚠ hook install failed for {}: {}", t.name, e);
        }
        configured += 1;
    }

    if configured > 0 {
        println!(
            "\n{} tool(s) configured. Restart your AI tools to apply.",
            configured
        );
    }
    Ok(())
}

struct DetectedTool {
    name: &'static str,
    is_configured: bool,
    kind: ToolKind,
}

#[derive(Clone, Copy)]
pub enum ToolKind {
    Codex,
    Claude,
    Cursor,
    Zed,
    OpenCode,
    Windsurf,
    Goose,
    Gemini,
}

fn detect(home: &Path) -> Vec<DetectedTool> {
    let mut tools = Vec::new();

    let checks: &[ToolCheck] = &[
        (
            "Codex CLI",
            |h| h.join(".codex").exists(),
            |h| check_toml(h, ".codex/config.toml", "[mcp_servers.agentrete]"),
            ToolKind::Codex,
        ),
        (
            "Claude Code",
            |h| h.join(".claude").exists(),
            |h| check_json(h, ".claude/.mcp.json", "agentrete"),
            ToolKind::Claude,
        ),
        (
            "Cursor",
            |h| h.join(".cursor").exists(),
            |h| check_json(h, ".cursor/mcp.json", "agentrete"),
            ToolKind::Cursor,
        ),
        (
            "Zed",
            |h| config_dir(h, "zed").exists(),
            |h| check_json(h, ".config/zed/settings.json", "agentrete"),
            ToolKind::Zed,
        ),
        (
            "OpenCode",
            |h| config_dir(h, "opencode").exists(),
            |h| check_json(h, ".config/opencode/opencode.json", "agentrete"),
            ToolKind::OpenCode,
        ),
        (
            "Windsurf",
            |h| h.join(".codeium/windsurf").exists(),
            |h| check_json(h, ".codeium/windsurf/mcp_config.json", "agentrete"),
            ToolKind::Windsurf,
        ),
        (
            "Goose",
            |h| config_dir(h, "goose").exists(),
            |h| check_yaml(h, ".config/goose/config.yaml", "agentrete"),
            ToolKind::Goose,
        ),
        (
            "Gemini CLI",
            |h| h.join(".gemini").exists(),
            |h| check_json(h, ".gemini/settings.json", "agentrete"),
            ToolKind::Gemini,
        ),
    ];

    for (name, installed, configured, kind) in checks {
        if installed(home) {
            tools.push(DetectedTool {
                name,
                is_configured: configured(home),
                kind: *kind,
            });
        }
    }

    tools
}

fn config_dir(home: &Path, app: &str) -> PathBuf {
    if let Some(d) = dirs::config_dir() {
        d.join(app)
    } else {
        home.join(".config").join(app)
    }
}

fn check_toml(home: &Path, path: &str, key: &str) -> bool {
    home.join(path).exists()
        && std::fs::read_to_string(home.join(path))
            .map(|c| c.contains(key))
            .unwrap_or(false)
}

fn check_json(home: &Path, path: &str, key: &str) -> bool {
    home.join(path).exists()
        && std::fs::read_to_string(home.join(path))
            .map(|c| c.contains(key))
            .unwrap_or(false)
}

fn check_yaml(home: &Path, path: &str, key: &str) -> bool {
    home.join(path).exists()
        && std::fs::read_to_string(home.join(path))
            .map(|c| c.contains(key))
            .unwrap_or(false)
}

fn configure_tool(t: &DetectedTool, home: &Path, bin: &str) -> Result<()> {
    match t.kind {
        ToolKind::Codex => configure_codex(home),
        ToolKind::Claude => configure_json(home, ".claude/.mcp.json", "mcpServers"),
        ToolKind::Cursor => configure_json(home, ".cursor/mcp.json", "mcpServers"),
        ToolKind::Zed => configure_json(home, ".config/zed/settings.json", "context_servers"),
        ToolKind::OpenCode => configure_opencode(home, bin),
        ToolKind::Windsurf => {
            configure_json(home, ".codeium/windsurf/mcp_config.json", "mcpServers")
        }
        ToolKind::Goose => configure_goose(home, bin),
        ToolKind::Gemini => configure_json(home, ".gemini/settings.json", "mcpServers"),
    }?;
    println!("  ✓ {} configured", t.name);
    Ok(())
}

/// Generic JSON-based MCP config (Claude, Cursor, Zed, Windsurf, Gemini).
fn configure_json(home: &Path, config_path: &str, root_key: &str) -> Result<()> {
    let path = home.join(config_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut config: serde_json::Value = if path.exists() {
        let raw = std::fs::read_to_string(&path)?;
        if raw.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}))
        }
    } else {
        serde_json::json!({})
    };

    let url = format!("http://127.0.0.1:{}/", DEFAULT_PORT);

    // Ensure root key and agentrete entry exist
    if config.get(root_key).is_none() || config[root_key].is_null() {
        config[root_key] = serde_json::json!({});
    }
    config[root_key]["agentrete"] = serde_json::json!({
        "type": "http",
        "url": url
    });

    std::fs::write(&path, serde_json::to_string_pretty(&config)?)?;
    Ok(())
}

/// Codex CLI uses TOML format.
fn configure_codex(home: &Path) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(home.join(".codex/config.toml"))?;

    let url = format!("http://127.0.0.1:{}/", DEFAULT_PORT);

    writeln!(f, "\n[mcp_servers.agentrete]")?;
    writeln!(f, "type = \"http\"")?;
    writeln!(f, "url = \"{}\"", url)?;
    Ok(())
}

/// OpenCode uses an array for the command.
fn configure_opencode(home: &Path, _bin: &str) -> Result<()> {
    let path = home.join(".config/opencode/opencode.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut config: serde_json::Value = if path.exists() {
        let raw = std::fs::read_to_string(&path)?;
        if raw.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}))
        }
    } else {
        serde_json::json!({})
    };

    let url = format!("http://127.0.0.1:{}/", DEFAULT_PORT);

    if config["mcpServers"].is_null() {
        config["mcpServers"] = serde_json::json!({});
    }
    config["mcpServers"]["agentrete"] = serde_json::json!({
        "type": "http",
        "url": url
    });

    std::fs::write(&path, serde_json::to_string_pretty(&config)?)?;
    Ok(())
}

/// Goose uses YAML for its config.
fn configure_goose(home: &Path, _bin: &str) -> Result<()> {
    let path = home.join(".config/goose/config.yaml");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let yaml = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        String::from("extensions:\n")
    };

    // Only add if not already present
    if !yaml.contains("agentrete") {
        let url = format!("http://127.0.0.1:{}/", DEFAULT_PORT);
        let leading_newline = if yaml.ends_with('\n') { "" } else { "\n" };
        let entry = format!(
            "{ld}  agentrete:\n    name: agentrete\n    type: http\n    url: \"{url}\"\n    enabled: true\n",
            ld = leading_newline
        );
        std::fs::write(&path, yaml + &entry)?;
    }
    Ok(())
}
