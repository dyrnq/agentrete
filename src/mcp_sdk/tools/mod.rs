//! Tool business logic for MCP SDK — pure data operations.
//! Each function takes `&Store` and returns `anyhow::Result<serde_json::Value>`.
//! No rmcp dependency here — these are tested independently from MCP transport.

pub mod kg;
pub mod memory;

use std::process::Command;

/// Detect project name from git repo root, falling back to current directory.
pub fn detect_project() -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
                std::path::Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
            } else {
                None
            }
        })
}
