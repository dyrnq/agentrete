//! Knowledge Graph — SPO triple store with petgraph for graph traversal.
//!
//! SQLite kg_triples 表 ——build_graph()──▶ petgraph DiGraph → 邻居/路径查询

use anyhow::Result;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub mod scanner;

/// Edge data stored in petgraph.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TripleEdge {
    pub predicate: String,
    pub confidence: f32,
    pub source_memory_id: Option<String>,
}

struct GraphInner {
    graph: DiGraph<String, TripleEdge>,
    node_index: HashMap<String, NodeIndex>,
}

/// In-memory knowledge graph backed by SQLite triples table.
/// Thread-safe via RwLock. Build once at startup, query many.
#[derive(Clone)]
pub struct KnowledgeGraph {
    inner: Arc<RwLock<GraphInner>>,
    enabled: bool,
}

impl KnowledgeGraph {
    /// Create a disabled KG (no-op on all operations).
    pub fn disabled() -> Self {
        Self {
            inner: Arc::new(RwLock::new(GraphInner {
                graph: DiGraph::new(),
                node_index: HashMap::new(),
            })),
            enabled: false,
        }
    }

    /// Build from SQLite triples table. Call once at startup.
    pub async fn build(pool: &sqlx::SqlitePool) -> Result<Self> {
        let rows: Vec<(String, String, String, f32, Option<String>)> = sqlx::query_as(
            "SELECT subject, predicate, object, confidence, source_memory_id FROM kg_triples",
        )
        .fetch_all(pool)
        .await?;

        let mut graph = DiGraph::new();
        let mut node_index: HashMap<String, NodeIndex> = HashMap::new();

        for (subject, predicate, object, confidence, source_memory_id) in rows {
            let sub_idx = *node_index
                .entry(subject.clone())
                .or_insert_with(|| graph.add_node(subject));
            let obj_idx = *node_index
                .entry(object.clone())
                .or_insert_with(|| graph.add_node(object));
            graph.add_edge(
                sub_idx,
                obj_idx,
                TripleEdge {
                    predicate,
                    confidence,
                    source_memory_id,
                },
            );
        }

        log::info!(
            "kg: built graph ({} nodes, {} edges)",
            graph.node_count(),
            graph.edge_count()
        );

        Ok(Self {
            inner: Arc::new(RwLock::new(GraphInner { graph, node_index })),
            enabled: true,
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Query neighbors of an entity.
    pub fn query_neighbors(
        &self,
        entity: &str,
        predicate: Option<&str>,
        direction: &str,
    ) -> Vec<(String, String, f32)> {
        let inner = self.inner.read().unwrap();
        let idx = match inner.node_index.get(entity) {
            Some(i) => *i,
            None => return vec![],
        };

        let mut results = Vec::new();

        if direction == "outgoing" || direction == "both" {
            for edge in inner.graph.edges_directed(idx, petgraph::Direction::Outgoing) {
                let target = &inner.graph[edge.target()];
                if predicate.map_or(true, |p| p == edge.weight().predicate) {
                    results.push((
                        target.clone(),
                        edge.weight().predicate.clone(),
                        edge.weight().confidence,
                    ));
                }
            }
        }

        if direction == "incoming" || direction == "both" {
            for edge in inner.graph.edges_directed(idx, petgraph::Direction::Incoming) {
                let source = &inner.graph[edge.source()];
                if direction == "both" && results.iter().any(|(t, _, _)| t == source) {
                    continue;
                }
                if predicate.map_or(true, |p| p == edge.weight().predicate) {
                    results.push((
                        source.clone(),
                        format!("in:{}", edge.weight().predicate),
                        edge.weight().confidence,
                    ));
                }
            }
        }

        results
    }

    /// Shortest path between two entities via Dijkstra.
    pub fn query_path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        use petgraph::algo::dijkstra;

        let inner = self.inner.read().unwrap();
        let from_idx = *inner.node_index.get(from)?;
        let to_idx = *inner.node_index.get(to)?;

        let dist_map = dijkstra(&inner.graph, from_idx, Some(to_idx), |_| 1);
        if !dist_map.contains_key(&to_idx) {
            return None;
        }

        // Walk backwards to reconstruct path
        let mut path = vec![to_idx];
        'outer: loop {
            let current = *path.last()?;
            if current == from_idx {
                break;
            }
            // Find the incoming edge whose source has a smaller distance
            for edge in inner.graph.edges_directed(current, petgraph::Direction::Incoming) {
                let prev = edge.source();
                if dist_map.get(&prev).is_some_and(|d| *d < dist_map[&current]) {
                    path.push(prev);
                    continue 'outer;
                }
            }
            return None;
        }
        path.reverse();
        Some(path.into_iter().map(|n| inner.graph[n].clone()).collect())
    }

    /// Query triples by source_memory_id (for search result enrichment).
    pub async fn query_by_memory_id(
        &self,
        pool: &sqlx::SqlitePool,
        memory_id: &str,
    ) -> Result<Vec<(String, String, String)>> {
        if !self.enabled {
            return Ok(vec![]);
        }
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT subject, predicate, object FROM kg_triples WHERE source_memory_id = ?1",
        )
        .bind(memory_id)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Add a triple to the in-memory graph (called after SQLite insert).
    pub fn add_triple_local(&self, subject: &str, predicate: &str, object: &str, confidence: f32, source_memory_id: Option<String>) {
        if let Ok(mut inner) = self.inner.write() {
            let sub = subject.to_string();
            let obj = object.to_string();
            // Separate mutations: first ensure nodes exist, then add edge
            if !inner.node_index.contains_key(&sub) {
                let idx = inner.graph.add_node(sub.clone());
                inner.node_index.insert(sub.clone(), idx);
            }
            if !inner.node_index.contains_key(&obj) {
                let idx = inner.graph.add_node(obj.clone());
                inner.node_index.insert(obj.clone(), idx);
            }
            let sub_idx = inner.node_index[&sub];
            let obj_idx = inner.node_index[&obj];
            inner.graph.add_edge(sub_idx, obj_idx, TripleEdge {
                predicate: predicate.to_string(),
                confidence,
                source_memory_id,
            });
        }
    }
}
