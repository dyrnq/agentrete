//! BM25 search utilities for agentrete.
//!
//! Uses DuckDB's FTS extension for full-text search scoring.

use anyhow::Result;
use duckdb::Connection;

use crate::types::SearchResult;

/// Search memories using hybrid approach: BM25 + vector similarity.
/// Falls back to LIKE + vector if BM25 FTS is not available.
pub fn search_fts(
    conn: &Connection,
    query: &str,
    limit: u8,
    memory_type: Option<&str>,
    query_embedding: Option<Vec<f32>>,
) -> Result<Vec<SearchResult>> {
    let limit = limit.min(50) as i64;

    // Phase 1: BM25 FTS text search
    let mut fts_results = match try_fts_search(conn, query, limit, memory_type) {
        Ok(r) => r,
        Err(_) => fallback_like_search(conn, query, limit, memory_type)?,
    };

    // Phase 2: Vector similarity search (if query embedding available)
    if let Some(qvec) = query_embedding {
        eprintln!("search_fts: vector triggered");
        if let Ok(vec_results) = search_vector(conn, &qvec, limit, memory_type) {
            // Merge: RRF-style, prefer vector results over text but keep FTS scores
            let fts_ids: std::collections::HashSet<String> =
                fts_results.iter().map(|r| r.id.clone()).collect();
            for vr in vec_results {
                eprintln!(
                    "search_fts: merging vector result id={} score={}",
                    vr.id, vr.score
                );
                if !fts_ids.contains(&vr.id) {
                    fts_results.push(vr);
                }
            }
        }
    }

    // Sort by score descending and limit
    fts_results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    eprintln!("search_fts final: {} results", fts_results.len());
    fts_results.truncate(limit as usize);
    Ok(fts_results)
}

fn try_fts_search(
    conn: &Connection,
    query: &str,
    limit: i64,
    memory_type: Option<&str>,
) -> Result<Vec<SearchResult>> {
    // Ensure FTS view exists
    conn.execute_batch(
        "CREATE OR REPLACE VIEW memories_fts_v AS
         SELECT id, type, content, tags::VARCHAR as tags_str,
                files::VARCHAR as files_str, project, importance, created_at
         FROM memories",
    )?;

    let sql = match memory_type {
        Some(_t) => "SELECT id, type, content, tags_str, files_str, project, importance, created_at::VARCHAR as created_at,
                    fts_score('memories_fts_v', ?1) as score
             FROM memories_fts_v
             WHERE content MATCH ?1 AND type = ?2
             ORDER BY score DESC LIMIT ?3"
            .to_string(),
        None => "SELECT id, type, content, tags_str, files_str, project, importance, created_at::VARCHAR as created_at,
                    fts_score('memories_fts_v', ?1) as score
             FROM memories_fts_v
             WHERE content MATCH ?1
             ORDER BY score DESC LIMIT ?2"
            .to_string(),
    };

    let mut stmt = conn.prepare(&sql)?;
    let rows = match memory_type {
        Some(t) => stmt.query_map(duckdb::params![query, t, limit], map_row)?,
        None => stmt.query_map(duckdb::params![query, limit], map_row)?,
    };

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

fn fallback_like_search(
    conn: &Connection,
    query: &str,
    limit: i64,
    memory_type: Option<&str>,
) -> Result<Vec<SearchResult>> {
    let pattern = format!("%{}%", query);

    let sql = match memory_type {
        Some(_t) => "SELECT id, type, content, tags::VARCHAR, files::VARCHAR,
                    project, importance, created_at::VARCHAR as created_at, 0.5 as score
             FROM memories
             WHERE content LIKE ?1 AND type = ?2
             ORDER BY created_at DESC LIMIT ?3"
            .to_string(),
        None => "SELECT id, type, content, tags::VARCHAR, files::VARCHAR,
                    project, importance, created_at::VARCHAR as created_at, 0.5 as score
             FROM memories
             WHERE content LIKE ?1
             ORDER BY created_at DESC LIMIT ?2"
            .to_string(),
    };

    let mut stmt = conn.prepare(&sql)?;
    let rows = match memory_type {
        Some(t) => stmt.query_map(duckdb::params![&pattern, t, limit], map_row)?,
        None => stmt.query_map(duckdb::params![&pattern, limit], map_row)?,
    };

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

fn map_row(row: &duckdb::Row) -> duckdb::Result<SearchResult> {
    Ok(SearchResult {
        id: row.get(0)?,
        memory_type: row.get(1)?,
        content: row.get(2)?,
        tags: parse_json_array(&row.get::<_, Option<String>>(3)?),
        files: parse_json_array(&row.get::<_, Option<String>>(4)?),
        project: row.get(5)?,
        importance: row.get(6)?,
        score: row.get(8)?,
        created_at: row.get(7)?,
    })
}

fn parse_json_array(val: &Option<String>) -> Option<Vec<String>> {
    match val {
        Some(s) if !s.is_empty() => serde_json::from_str(s).ok(),
        _ => None,
    }
}

/// Vector similarity search using DuckDB's built-in array_cosine_similarity.
/// Uses the stored embedding (FLOAT[]) to find semantically similar memories.
/// Falls back gracefully if no embeddings exist for queried records.
fn search_vector(
    conn: &Connection,
    query_embedding: &[f32],
    limit: i64,
    memory_type: Option<&str>,
) -> Result<Vec<SearchResult>> {
    let array_values: String = query_embedding
        .iter()
        .map(|v| format!("{}::FLOAT", v))
        .collect::<Vec<_>>()
        .join(",");
    let array_expr = format!("array_value({})", array_values);
    let score_expr = format!(
        "(list_cosine_similarity(embedding, {0}) + 1.0) / 2.0",
        array_expr
    );

    let sql = match memory_type {
        Some(_t) => format!(
            "SELECT id, type, content, tags::VARCHAR, files::VARCHAR,
                    project, importance, created_at::VARCHAR as created_at,
                    {score} as score
             FROM memories
             WHERE embedding IS NOT NULL AND type = ?1
             ORDER BY score DESC
             LIMIT ?2",
            score = score_expr
        ),
        None => format!(
            "SELECT id, type, content, tags::VARCHAR, files::VARCHAR,
                    project, importance, created_at::VARCHAR as created_at,
                    {score} as score
             FROM memories
             WHERE embedding IS NOT NULL
             ORDER BY score DESC
             LIMIT ?1",
            score = score_expr
        ),
    };

    eprintln!("search_vector SQL: {}", sql);
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        eprintln!("search_vector prepare error: {}", e);
        e
    })?;
    let rows = match memory_type {
        Some(t) => stmt
            .query_map(duckdb::params![t, limit], map_row)
            .map_err(|e| {
                eprintln!("search_vector query_map error: {}", e);
                e
            })?,
        None => stmt
            .query_map(duckdb::params![limit], map_row)
            .map_err(|e| {
                eprintln!("search_vector query_map error: {}", e);
                e
            })?,
    };

    let mut results = Vec::new();
    for row in rows {
        match row {
            Ok(r) => results.push(r),
            Err(e) => eprintln!("search_vector row error: {}", e),
        }
    }
    eprintln!("search_vector: {} results", results.len());
    Ok(results)
}
