use crate::storage::Store;

use serde_json::Value;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::handlers::{handle_rpc, jsonrpc_err};

pub async fn run_stdio(store: Store) -> anyhow::Result<()> {
    log::info!("agentrete MCP server (stdio)");
    let store = Arc::new(store);
    let stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut lines = stdin.lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let rq: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let err = jsonrpc_err(&Value::Null, -32700, &format!("Parse error: {}", e));
                let _ = stdout
                    .write_all(serde_json::to_string(&err).unwrap_or_default().as_bytes())
                    .await;
                let _ = stdout.write_all(b"\n").await;
                let _ = stdout.flush().await;
                continue;
            }
        };
        let result = handle_rpc(&store, rq["method"].as_str().unwrap_or(""), &rq["params"]).await;
        let _ = stdout
            .write_all(
                serde_json::to_string(&result)
                    .unwrap_or_default()
                    .as_bytes(),
            )
            .await;
        let _ = stdout.write_all(b"\n").await;
        let _ = stdout.flush().await;
    }
    Ok(())
}
