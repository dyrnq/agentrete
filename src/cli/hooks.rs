//! Hook installers for AI coding agents.

use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn install(tool: &super::setup_wizard::ToolKind, home: &Path) -> Result<()> {
    match tool {
        super::setup_wizard::ToolKind::Codex => install_codex(home),
        super::setup_wizard::ToolKind::Claude => install_claude(home),
        _ => Ok(()),
    }
}

// ─── Codex CLI ──────────────────────────────────────────────────────────────

fn install_codex(home: &Path) -> Result<()> {
    let plugin_dir = home.join(".codex/plugins/agentrete");
    let scripts_dir = plugin_dir.join("hooks/scripts");
    std::fs::create_dir_all(&scripts_dir)?;

    if cfg!(target_os = "windows") {
        std::fs::write(
            plugin_dir.join("hooks/hooks.codex.json"),
            Windows::hooks_json(),
        )?;
        for (name, content) in Windows::all_scripts() {
            std::fs::write(scripts_dir.join(name), content)?;
        }
    } else {
        std::fs::write(
            plugin_dir.join("hooks/hooks.codex.json"),
            Unix::hooks_json(),
        )?;
        for (name, content) in Unix::all_scripts() {
            let path = scripts_dir.join(name);
            std::fs::write(&path, content)?;
            set_executable(&path)?;
        }
    }

    println!("  ✓ hooks.codex.json + scripts installed");
    Ok(())
}

// ─── Claude Code ────────────────────────────────────────────────────────────

fn install_claude(home: &Path) -> Result<()> {
    let hooks_dir = home.join(".claude/hooks");
    std::fs::create_dir_all(&hooks_dir)?;

    if cfg!(target_os = "windows") {
        std::fs::write(
            hooks_dir.join("agentrete-startup.ps1"),
            Windows::claude_startup(),
        )?;
        std::fs::write(
            hooks_dir.join("agentrete-post-tool.ps1"),
            Windows::claude_post_tool(),
        )?;
    } else {
        std::fs::write(
            hooks_dir.join("agentrete-startup.sh"),
            Unix::claude_startup(),
        )?;
        set_executable(&hooks_dir.join("agentrete-startup.sh"))?;
        std::fs::write(
            hooks_dir.join("agentrete-post-tool.sh"),
            Unix::claude_post_tool(),
        )?;
        set_executable(&hooks_dir.join("agentrete-post-tool.sh"))?;
    }

    let settings_path = home.join(".claude/settings.json");
    let mut settings: serde_json::Value = if settings_path.exists() {
        let raw = std::fs::read_to_string(&settings_path)?;
        if raw.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&raw).unwrap_or(serde_json::json!({}))
        }
    } else {
        serde_json::json!({})
    };

    let (startup_cmd, post_tool_cmd) = if cfg!(target_os = "windows") {
        (
            format!(
                "powershell -NoProfile -File {}",
                hooks_dir.join("agentrete-startup.ps1").display()
            ),
            format!(
                "powershell -NoProfile -File {}",
                hooks_dir.join("agentrete-post-tool.ps1").display()
            ),
        )
    } else {
        (
            format!("sh {}", hooks_dir.join("agentrete-startup.sh").display()),
            format!("sh {}", hooks_dir.join("agentrete-post-tool.sh").display()),
        )
    };

    settings["hooks"] = serde_json::json!({
        "SessionStart": [{"hooks": [{"type": "command", "command": startup_cmd}]}],
        "PostToolUse": [{"matcher": "Edit|Write|Bash", "hooks": [{"type": "command", "command": post_tool_cmd}]}]
    });

    std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
    println!("  ✓ Claude hooks + settings.json installed");
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &PathBuf) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &PathBuf) -> Result<()> {
    Ok(())
}

// ─── Embedded resources ─────────────────────────────────────────────────────

struct Unix;
impl Unix {
    fn hooks_json() -> &'static str {
        include_str!("../../hooks/unix/hooks.codex.json")
    }
    fn all_scripts() -> Vec<(&'static str, &'static str)> {
        vec![
            ("session-start.sh", Self::session_start()),
            ("prompt-submit.sh", Self::prompt_submit()),
            ("pre-tool-use.sh", Self::pre_tool_use()),
            ("post-tool-use.sh", Self::post_tool_use()),
            ("pre-compact.sh", Self::pre_compact()),
            ("post-compact.sh", Self::post_compact()),
            ("subagent-start.sh", Self::subagent_start()),
            ("subagent-stop.sh", Self::subagent_stop()),
            ("stop.sh", Self::stop()),
        ]
    }
    fn session_start() -> &'static str {
        include_str!("../../hooks/unix/session-start.sh")
    }
    fn prompt_submit() -> &'static str {
        include_str!("../../hooks/unix/prompt-submit.sh")
    }
    fn pre_tool_use() -> &'static str {
        include_str!("../../hooks/unix/pre-tool-use.sh")
    }
    fn post_tool_use() -> &'static str {
        include_str!("../../hooks/unix/post-tool-use.sh")
    }
    fn pre_compact() -> &'static str {
        include_str!("../../hooks/unix/pre-compact.sh")
    }
    fn post_compact() -> &'static str {
        include_str!("../../hooks/unix/post-compact.sh")
    }
    fn subagent_start() -> &'static str {
        include_str!("../../hooks/unix/subagent-start.sh")
    }
    fn subagent_stop() -> &'static str {
        include_str!("../../hooks/unix/subagent-stop.sh")
    }
    fn stop() -> &'static str {
        include_str!("../../hooks/unix/stop.sh")
    }
    fn claude_startup() -> &'static str {
        include_str!("../../hooks/unix/claude-startup.sh")
    }
    fn claude_post_tool() -> &'static str {
        include_str!("../../hooks/unix/claude-post-tool.sh")
    }
}

struct Windows;
impl Windows {
    fn hooks_json() -> &'static str {
        include_str!("../../hooks/windows/hooks.codex.json")
    }
    fn all_scripts() -> Vec<(&'static str, &'static str)> {
        vec![
            ("session-start.ps1", Self::session_start()),
            ("prompt-submit.ps1", Self::prompt_submit()),
            ("pre-tool-use.ps1", Self::pre_tool_use()),
            ("post-tool-use.ps1", Self::post_tool_use()),
            ("pre-compact.ps1", Self::pre_compact()),
            ("post-compact.ps1", Self::post_compact()),
            ("subagent-start.ps1", Self::subagent_start()),
            ("subagent-stop.ps1", Self::subagent_stop()),
            ("stop.ps1", Self::stop()),
        ]
    }
    fn session_start() -> &'static str {
        include_str!("../../hooks/windows/session-start.ps1")
    }
    fn prompt_submit() -> &'static str {
        include_str!("../../hooks/windows/prompt-submit.ps1")
    }
    fn pre_tool_use() -> &'static str {
        include_str!("../../hooks/windows/pre-tool-use.ps1")
    }
    fn post_tool_use() -> &'static str {
        include_str!("../../hooks/windows/post-tool-use.ps1")
    }
    fn pre_compact() -> &'static str {
        include_str!("../../hooks/windows/pre-compact.ps1")
    }
    fn post_compact() -> &'static str {
        include_str!("../../hooks/windows/post-compact.ps1")
    }
    fn subagent_start() -> &'static str {
        include_str!("../../hooks/windows/subagent-start.ps1")
    }
    fn subagent_stop() -> &'static str {
        include_str!("../../hooks/windows/subagent-stop.ps1")
    }
    fn stop() -> &'static str {
        include_str!("../../hooks/windows/stop.ps1")
    }
    fn claude_startup() -> &'static str {
        include_str!("../../hooks/windows/claude-startup.ps1")
    }
    fn claude_post_tool() -> &'static str {
        include_str!("../../hooks/windows/claude-post-tool.ps1")
    }
}
