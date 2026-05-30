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


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_test_graph() -> KnowledgeGraph {
        let inner = Arc::new(RwLock::new(GraphInner {
            graph: DiGraph::new(),
            node_index: HashMap::new(),
        }));
        KnowledgeGraph { inner, enabled: true }
    }

    #[test]
    fn test_basic_neighbors() {
        let kg = make_test_graph();
        kg.add_triple_local("agentrete", "uses", "sqlx", 1.0, None);
        kg.add_triple_local("agentrete", "uses", "axum", 0.9, None);
        kg.add_triple_local("agentrete", "deprecated", "rusqlite", 0.8, None);

        // All outgoing
        let n = kg.query_neighbors("agentrete", None, "outgoing");
        assert_eq!(n.len(), 3);
        assert!(n.iter().any(|(t, _, _)| t == "sqlx"));
        assert!(n.iter().any(|(t, _, _)| t == "axum"));
        assert!(n.iter().any(|(t, _, _)| t == "rusqlite"));

        // Filter by predicate
        let n = kg.query_neighbors("agentrete", Some("uses"), "outgoing");
        assert_eq!(n.len(), 2);
        assert!(n.iter().all(|(_, r, _)| r == "uses"));

        // Direction filter
        let n = kg.query_neighbors("sqlx", None, "incoming");
        assert_eq!(n.len(), 1);
        assert!(n[0].0 == "agentrete");
    }

    #[test]
    fn test_no_relations() {
        let kg = make_test_graph();
        let n = kg.query_neighbors("nonexistent", None, "outgoing");
        assert!(n.is_empty());
    }

    #[test]
    fn test_query_path() {
        let kg = make_test_graph();
        kg.add_triple_local("a", "knows", "b", 1.0, None);
        kg.add_triple_local("b", "knows", "c", 1.0, None);

        let path = kg.query_path("a", "c");
        assert!(path.is_some());
        let p = path.unwrap();
        assert_eq!(p, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_path_no_connection() {
        let kg = make_test_graph();
        kg.add_triple_local("a", "knows", "b", 1.0, None);
        kg.add_triple_local("c", "knows", "d", 1.0, None);

        let path = kg.query_path("a", "d");
        assert!(path.is_none());
    }

    #[test]
    fn test_path_same_node() {
        let kg = make_test_graph();
        kg.add_triple_local("a", "knows", "b", 1.0, None);

        let path = kg.query_path("a", "a");
        assert!(path.is_some());
        assert_eq!(path.unwrap(), vec!["a"]);
    }

    #[test]
    fn test_disabled_graph() {
        let kg = KnowledgeGraph::disabled();
        assert!(!kg.is_enabled());
        let n = kg.query_neighbors("anything", None, "outgoing");
        assert!(n.is_empty());
    }

    #[test]
    fn test_add_twice_same_nodes() {
        let kg = make_test_graph();
        kg.add_triple_local("node", "rel1", "target", 1.0, None);
        kg.add_triple_local("node", "rel2", "target", 1.0, None);

        let n = kg.query_neighbors("node", None, "outgoing");
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].0, "target");
        assert_eq!(n[1].0, "target");
    }

    #[test]
    fn test_confidence_and_source() {
        let kg = make_test_graph();
        kg.add_triple_local("x", "uses", "y", 0.5, Some("mem_123".into()));

        let inner = kg.inner.read().unwrap();
        let idx = inner.node_index.get("x").unwrap();
        let edge = inner.graph.edges_directed(*idx, petgraph::Direction::Outgoing)
            .next().unwrap();
        assert!((edge.weight().confidence - 0.5).abs() < 1e-6);
        assert_eq!(edge.weight().source_memory_id, Some("mem_123".to_string()));
    }

    #[test]
    fn test_extract_name() {
        let cases = vec![
            ("fn main() {", "main"),
            ("pub fn foo()", "foo"),
            ("async fn bar()", "bar"),
            ("pub async fn baz()", "baz"),
            ("struct Foo {", "Foo"),
            ("struct Foo<T> {", "Foo"),
            ("pub struct Bar", "Bar"),
            ("enum Color {", "Color"),
            ("trait Into {", "Into"),
            ("class Hello {", "Hello"),
            ("def hello():", "hello"),
            ("pub(crate) fn inside()", "inside"),
            ("pub unsafe fn danger()", "danger"),
            ("const MAX: usize = 100;", "MAX"),
            ("static NAME: &str = 'x';", "NAME"),
        ];
        for (input, expected) in cases {
            let result = super::scanner::extract_name(input);
            assert_eq!(result, expected, "extract_name({:?}) should be {:?}", input, expected);
        }
    }

    #[test]
    fn test_extract_import_target() {
        let cases: Vec<(&str, &str, &str)> = vec![
            ("use std::collections::HashMap", "rust", "std"),
            ("import os", "python", "os"),
            ("from pathlib import Path", "python", "pathlib"),
            ("import java.util.List", "java", "java"),
            ("import { x } from 'react'", "typescript", "react"),
            ("import \"fmt\"", "go", "fmt"),
        ];
        for (text, lang, expected) in cases {
            let result = super::scanner::extract_import_target(text, lang);
            assert_eq!(result, expected, "extract_import_target({:?}, {:?}) should be {:?}", text, lang, expected);
        }
    }

    #[test]
    fn test_kind_to_symbol_kind() {
        assert_eq!(super::scanner::kind_to_symbol_kind("struct_item"), "struct");
        assert_eq!(super::scanner::kind_to_symbol_kind("function_item"), "function");
        assert_eq!(super::scanner::kind_to_symbol_kind("class_declaration"), "class");
        assert_eq!(super::scanner::kind_to_symbol_kind("unknown_thing"), "unknown_thing");
    }
}
