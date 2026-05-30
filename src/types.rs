//! Types for agentrete memory system.

use serde::{Deserialize, Serialize};

/// A single memory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub session_id: Option<String>,
    pub memory_type: Option<String>, // decision|pattern|bug|architecture|workflow|fact
    pub content: String,
    pub tags: Option<Vec<String>>,
    pub files: Option<Vec<String>>,
    pub project: Option<String>,
    pub importance: i32,
    pub source_file: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Input for creating a new memory.
#[derive(Debug, Clone)]
pub struct NewMemory {
    pub content: String,
    pub memory_type: Option<String>,
    pub tags: Option<Vec<String>>,
    pub files: Option<Vec<String>>,
    pub project: Option<String>,
    pub source_file: Option<String>,
}

/// Search result with relevance score.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub id: String,
    pub memory_type: Option<String>,
    pub content: String,
    pub tags: Option<Vec<String>>,
    pub files: Option<Vec<String>>,
    pub project: Option<String>,
    pub importance: i32,
    pub source_file: Option<String>,
    pub score: f64,
    pub created_at: String,
    /// Raw embedding BLOB for on-the-fly cosine computation
    #[serde(skip)]
    #[allow(dead_code)]
    pub embedding: Option<Vec<u8>>,
}

/// Database statistics.
#[derive(Debug, Clone)]
pub struct DbStats {
    pub memory_count: i64,
    pub with_embedding: i64,
    pub type_counts: Vec<(String, i64)>,
    pub model_info: Option<String>,
    pub session_count: i64,
    pub observation_count: i64,
    pub db_path: String,
    pub db_size_bytes: u64,
}
