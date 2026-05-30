//! Search engine: RRF fusion (vec0 KNN + FTS5 BM25), cosine rerank fallback.
//!
//! Row types and vector math helpers are also defined here as they're
//! tightly coupled to the search paths.

use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;

use crate::embed::embeddings::Embedder;
use crate::types::SearchResult;

// ─── Tunable constants ───────────────────────────────────────────────────────

pub(crate) const MAX_LIMIT: u8 = 100;
pub(crate) const RECALL_MULTIPLIER: u8 = 3;

// ─── Row types ──────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
pub(crate) struct SearchRow {
    pub(crate) id: String,
    #[sqlx(rename = "type")]
    pub(crate) memory_type: Option<String>,
    pub(crate) content: String,
    pub(crate) tags: Option<String>,
    pub(crate) files: Option<String>,
    pub(crate) project: Option<String>,
    pub(crate) source_file: Option<String>,
    pub(crate) importance: Option<i32>,
    pub(crate) created_at: Option<String>,
    pub(crate) embedding: Option<Vec<u8>>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct MemoryRow {
    pub(crate) id: String,
    #[sqlx(rename = "type")]
    pub(crate) memory_type: Option<String>,
    pub(crate) content: String,
    pub(crate) tags: Option<String>,
    pub(crate) files: Option<String>,
    pub(crate) project: Option<String>,
    pub(crate) source_file: Option<String>,
    pub(crate) importance: Option<i32>,
    pub(crate) created_at: Option<String>,
    pub(crate) updated_at: Option<String>,
}

// ─── Vector math ─────────────────────────────────────────────────────────────

pub(crate) fn normalize_l2(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-10 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

pub(crate) fn bytes_to_f32_vec(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.len() % 4 != 0 {
        return None;
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

pub(crate) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (dot, na, nb) = a
        .iter()
        .zip(b.iter())
        .fold((0.0f32, 0.0f32, 0.0f32), |(d, na, nb), (&x, &y)| {
            (d + x * y, na + x * x, nb + y * y)
        });
    let denom = (na.sqrt() * nb.sqrt()).max(1e-10);
    (dot / denom).clamp(-1.0, 1.0)
}

pub(crate) fn parse_json(val: &Option<String>) -> Option<Vec<String>> {
    match val {
        Some(s) if !s.is_empty() => serde_json::from_str(s).ok(),
        _ => None,
    }
}

// ─── RRF (Reciprocal Rank Fusion) ────────────────────────────────────────────

/// Reciprocal Rank Fusion: merge vec0 KNN and FTS5 BM25 ranked lists.
/// RRF score = sum(1 / (K + rank)) across lists, with K=60.
/// Returns top-k results sorted by RRF score descending.
pub(crate) fn rrf_merge(
    vec_results: Vec<SearchResult>,
    fts_results: Vec<SearchResult>,
    k: usize,
    rrf_k: f64,
) -> Vec<SearchResult> {
    use std::collections::HashMap;

    let mut scores: HashMap<&str, f64> = HashMap::new();
    let mut data: HashMap<&str, &SearchResult> = HashMap::new();

    for (rank, r) in vec_results.iter().enumerate() {
        *scores.entry(r.id.as_str()).or_default() += 1.0 / (rrf_k + rank as f64 + 1.0);
        data.entry(r.id.as_str()).or_insert(r);
    }
    for (rank, r) in fts_results.iter().enumerate() {
        *scores.entry(r.id.as_str()).or_default() += 1.0 / (rrf_k + rank as f64 + 1.0);
        data.entry(r.id.as_str()).or_insert(r);
    }

    let mut merged: Vec<(&str, f64)> = scores.into_iter().collect();
    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    merged
        .into_iter()
        .take(k)
        .filter_map(|(id, score)| {
            data.get(id).map(|r| SearchResult {
                id: r.id.clone(),
                memory_type: r.memory_type.clone(),
                content: r.content.clone(),
                tags: r.tags.clone(),
                files: r.files.clone(),
                project: r.project.clone(),
                source_file: r.source_file.clone(),
                importance: r.importance,
                score,
                created_at: r.created_at.clone(),
                embedding: r.embedding.clone(),
            })
        })
        .collect()
}

// ─── Search methods ──────────────────────────────────────────────────────────

/// sqlite-vec KNN search. Falls back to FTS5 if vec extension not loaded.
pub(crate) async fn search_vec(
    pool: &SqlitePool,
    query_vec_orig: &[f32],
    limit: u8,
    memory_type: Option<&str>,
) -> Result<Vec<SearchResult>> {
    let mut query_vec = query_vec_orig.to_vec();
    normalize_l2(&mut query_vec);
    let query_vec = query_vec.as_slice();
    let json_vec: String = serde_json::to_string(&query_vec)?;
    let lim = limit.min(50) as i64;

    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i32>,
        Option<String>,
        f64,
    )> = if let Some(t) = memory_type {
        sqlx::query_as(
                "SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.source_file, m.importance, m.created_at, v.distance                  FROM vec_memories v                  JOIN memories m ON m.rowid = v.rowid WHERE m.deleted_at IS NULL AND m.type = ?4 AND v.embedding MATCH ?1 AND v.k = ?2                  ORDER BY v.distance LIMIT ?3",
            )
            .bind(&json_vec).bind(lim).bind(lim).bind(t)
            .fetch_all(pool)
            .await?
    } else {
        sqlx::query_as(
                "SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.source_file, m.importance, m.created_at, v.distance                  FROM vec_memories v                  JOIN memories m ON m.rowid = v.rowid WHERE m.deleted_at IS NULL AND v.embedding MATCH ?1 AND v.k = ?2                  ORDER BY v.distance LIMIT ?3",
            )
            .bind(&json_vec).bind(lim).bind(lim)
            .fetch_all(pool)
            .await?
    };

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                mt,
                content,
                tags,
                files,
                project,
                source_file,
                importance,
                created_at,
                distance,
            )| {
                SearchResult {
                    id,
                    memory_type: mt,
                    content,
                    tags: parse_json(&tags),
                    files: parse_json(&files),
                    project,
                    source_file,
                    importance: importance.unwrap_or(3),
                    score: (1.0_f64 - distance.powi(2) / 2.0).max(0.0),
                    created_at: created_at.unwrap_or_default(),
                    embedding: None,
                }
            },
        )
        .collect())
}

/// Hybrid search with Reciprocal Rank Fusion (RRF).
/// Runs vec0 KNN and FTS5 BM25 concurrently, then merges scores via RRF (k=60).
pub(crate) async fn search_rrf(
    pool: &SqlitePool,
    embedder: &Option<Arc<Embedder>>,
    vec_enabled: bool,
    query: &str,
    limit: u8,
    memory_type: Option<&str>,
    rrf_k: f64,
    decay: impl Fn(f64, &str) -> f64,
) -> Result<Vec<SearchResult>> {
    let k = limit.min(MAX_LIMIT) as usize;

    // Get query embedding upfront (needed for vec0, may be used for fallback)
    let qv = if vec_enabled {
        if let Some(ref emb) = embedder {
            emb.embed_one(query).await.ok()
        } else {
            None
        }
    } else {
        None
    };

    // Run both search paths concurrently
    let (mut vec_results, fts_results) = if let Some(ref qv) = qv {
        let vec_fut = search_vec(pool, qv, limit, memory_type);
        let fts_fut = search_fts(pool, query, limit.min(MAX_LIMIT), memory_type);
        let (vr, fr) = tokio::join!(vec_fut, fts_fut);
        let vec_r = vr.unwrap_or_default();
        let fts_r = fr?;
        (vec_r, fts_r)
    } else {
        let fts_r = search_fts(pool, query, limit.min(MAX_LIMIT), memory_type).await?;
        (vec![], fts_r)
    };

    if vec_results.is_empty() {
        if !fts_results.is_empty() {
            let mut fts_results = fts_results;
            for r in &mut fts_results {
                r.score = decay(r.score, &r.created_at);
            }
            log::info!("rrf: FTS5-only ({} results)", fts_results.len());
            return Ok(fts_results);
        }
        if let Some(ref emb) = embedder {
            if qv.is_some() {
                let hybrid = search_hybrid(pool, emb, query, limit, memory_type).await?;
                log::info!(
                    "rrf: cosine rerank fallback ({} results, top={:.3})",
                    hybrid.len(),
                    hybrid.first().map(|r| r.score).unwrap_or(0.0)
                );
                return Ok(hybrid);
            }
        }
        return Ok(vec![]);
    }

    // Apply temporal decay to vec_results before merge
    for r in &mut vec_results {
        r.score = decay(r.score, &r.created_at);
    }

    let merged = rrf_merge(vec_results, fts_results, k, rrf_k);
    log::info!("rrf: merged {} results", merged.len());
    Ok(merged)
}

/// FTS5-only keyword search.
pub(crate) async fn search_fts(
    pool: &SqlitePool,
    query: &str,
    limit: u8,
    memory_type: Option<&str>,
) -> Result<Vec<SearchResult>> {
    let lim = limit.min(MAX_LIMIT) as i64;
    let rows: Vec<SearchRow> = if let Some(t) = memory_type {
        sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.source_file, m.importance, m.created_at, m.embedding FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 AND m.type=?2 AND m.deleted_at IS NULL ORDER BY rank LIMIT ?3")
            .bind(query).bind(t).bind(lim).fetch_all(pool).await?
    } else {
        sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.source_file, m.importance, m.created_at, m.embedding FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 AND m.deleted_at IS NULL ORDER BY rank LIMIT ?2")
            .bind(query).bind(lim).fetch_all(pool).await?
    };
    Ok(rows
        .into_iter()
        .map(|r| SearchResult {
            id: r.id,
            memory_type: r.memory_type,
            content: r.content,
            tags: parse_json(&r.tags),
            files: parse_json(&r.files),
            project: r.project,
            source_file: r.source_file,
            importance: r.importance.unwrap_or(3),
            score: 0.5,
            created_at: r.created_at.unwrap_or_default(),
            embedding: r.embedding,
        })
        .collect())
}

/// Hybrid search: FTS5 recall + cosine rerank with query embedding.
pub(crate) async fn search_hybrid(
    pool: &SqlitePool,
    embedder: &Embedder,
    query: &str,
    limit: u8,
    memory_type: Option<&str>,
) -> Result<Vec<SearchResult>> {
    // Step 1: Get query embedding from local/remote model
    let query_vec = embedder.embed_one(query).await?;

    // Step 2: FTS5 recall (wider window for reranking)
    let recall_limit = (limit as i64 * RECALL_MULTIPLIER as i64).min(MAX_LIMIT as i64);
    let rows: Vec<SearchRow> = if let Some(t) = memory_type {
        sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.source_file, m.importance, m.created_at, m.embedding FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 AND m.type=?2 AND m.deleted_at IS NULL ORDER BY rank LIMIT ?3")
            .bind(query).bind(t).bind(recall_limit).fetch_all(pool).await?
    } else {
        sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.source_file, m.importance, m.created_at, m.embedding FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 AND m.deleted_at IS NULL ORDER BY rank LIMIT ?2")
            .bind(query).bind(recall_limit).fetch_all(pool).await?
    };

    // Step 3: Cosine rerank (only rows with embeddings)
    let mut results: Vec<SearchResult> = Vec::new();
    let mut bm25_only: Vec<SearchResult> = Vec::new();

    for r in rows {
        if let Some(ref emb) = r.embedding {
            if let Some(emb_vec) = bytes_to_f32_vec(emb) {
                let cosine = cosine_similarity(&query_vec, &emb_vec);
                results.push(SearchResult {
                    id: r.id,
                    memory_type: r.memory_type,
                    content: r.content,
                    tags: parse_json(&r.tags),
                    files: parse_json(&r.files),
                    project: r.project,
                    source_file: r.source_file.clone(),
                    importance: r.importance.unwrap_or(3),
                    score: cosine as f64,
                    created_at: r.created_at.unwrap_or_default(),
                    embedding: Some(emb.clone()),
                });
                continue;
            }
        }
        // Fallback: BM25-only (no embedding yet)
        bm25_only.push(SearchResult {
            id: r.id,
            memory_type: r.memory_type,
            content: r.content,
            tags: parse_json(&r.tags),
            files: parse_json(&r.files),
            project: r.project,
            source_file: r.source_file.clone(),
            importance: r.importance.unwrap_or(3),
            score: 0.5,
            created_at: r.created_at.unwrap_or_default(),
            embedding: r.embedding.clone(),
        });
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit as usize);

    // Append BM25-only results for padding
    if results.len() < limit as usize {
        let remaining = limit as usize - results.len();
        results.extend(bm25_only.into_iter().take(remaining));
    }

    Ok(results)
}
