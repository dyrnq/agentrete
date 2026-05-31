#![allow(dead_code)]
const SEED_RULES: &[(&str, &str, &str)] = &[
    (
        "Think Before Coding: State assumptions, surface tradeoffs",
        "rule",
        "karpathy,coding",
    ),
    (
        "Simplicity First: Minimum code, no speculative features",
        "rule",
        "karpathy,coding",
    ),
    (
        "Surgical Changes: Minimal edits, preserve existing code style",
        "rule",
        "karpathy,coding",
    ),
    (
        "Goal-Driven Execution: Close open loops, verify each step",
        "rule",
        "karpathy,coding",
    ),
    (
        "Systematic Debugging: Identify root cause, create minimal reproduction",
        "rule",
        "superpowers,debugging",
    ),
    (
        "Test-Driven Development: Write failing test first, then implement",
        "rule",
        "superpowers,tdd",
    ),
    (
        "Code Modification: NEVER use sed or python3 to modify source code",
        "rule",
        "coding,CRITICAL",
    ),
    (
        "Code Modification: Use apply_patch (Unified Diff) as the only legal way",
        "rule",
        "coding,CRITICAL",
    ),
    (
        "Doc Paths: Never use private paths (like ~ or 192.168.x.x) in documentation",
        "rule",
        "coding,docs,CRITICAL",
    ),
    (
        "Validation: After code change: cargo fmt -> clippy -D warnings -> build",
        "rule",
        "coding,validation,CRITICAL",
    ),
    (
        "Validation: On failure, revert immediately before debugging",
        "rule",
        "coding,validation,CRITICAL",
    ),
];

use crate::storage::Store;
use crate::types;
use anyhow::Result;

pub(crate) async fn cmd_seed(store: &Store) -> Result<()> {
    let rules = SEED_RULES;
    let mut new_count = 0u32;
    let mut skip_count = 0u32;
    for (content, mem_type, tags) in rules {
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM memories WHERE content = ?1 AND type = ?2 AND deleted_at IS NULL",
        )
        .bind(content)
        .bind(mem_type)
        .fetch_optional(&store.pool)
        .await?;
        if existing.is_some() {
            skip_count += 1;
            println!("  SKIP {}", &content[..content.len().min(60)]);
            continue;
        }
        store
            .save(types::NewMemory {
                content: content.to_string(),
                memory_type: Some(mem_type.to_string()),
                tags: Some(tags.split(',').map(|s| s.trim().to_string()).collect()),
                files: None,
                project: None,
                source_file: None,
            })
            .await?;
        new_count += 1;
        println!("  NEW  {}", &content[..content.len().min(60)]);
    }
    println!("Done: {} new, {} skipped.", new_count, skip_count);
    Ok(())
}
