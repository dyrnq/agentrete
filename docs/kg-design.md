# Knowledge Graph 模块

> **状态**: 已实现，v0.1
> **入口**: `docs/kg-design.md`

## 架构

```
agentrete
├── Memory Engine（核心）
│   └── SQLite + vec0 + FTS5 → rules/decisions/patterns/bugs
│
└── Knowledge Graph（可选，config 开关）
    ├── SQLite kg_triples 表（持久化）
    ├── petgraph DiGraph（内存，图算法）
    ├── ast-grep 代码扫描（16 种语言）
    └── file watcher（notify crate，自动增量更新）
```

## MCP 工具

| 工具 | 功能 |
|------|------|
| `kg_query` | 图查询：邻居遍历、最短路径、按 predicate/direction/project 过滤 |
| `kg_scan` | 触发后台代码扫描（增量：文件 hash + mtime 缓存，重复可幂等） |
| `kg_scan_status` | 查询扫描进度 |
| `kg_watch` | 启动/停止文件监控，文件变化自动触发增量扫描 |

## 数据模型

### SQLite: kg_triples

```sql
CREATE TABLE IF NOT EXISTS kg_triples (
    id TEXT PRIMARY KEY,
    subject TEXT NOT NULL,
    predicate TEXT NOT NULL,
    object TEXT NOT NULL,
    confidence REAL DEFAULT 1.0,
    source_memory_id TEXT,       -- 关联记忆 ID（手动关联）或 '_scan_' 前缀（扫描数据）
    project TEXT,                -- 项目名，自动从 git 检测，用于隔离
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_kg_triples_spo ON kg_triples(subject, predicate, object, project);
```

### SQLite: kg_scan_cache

```sql
CREATE TABLE IF NOT EXISTS kg_scan_cache (
    file_path TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    modified_at INTEGER NOT NULL
);
```

### petgraph 运行时

```rust
pub struct KnowledgeGraph {
    inner: Arc<RwLock<GraphInner>>,  // DiGraph<String, TripleEdge>
    enabled: bool,
}

pub struct TripleEdge {
    pub predicate: String,
    pub confidence: f32,
    pub source_memory_id: Option<String>,
}
```

## 代码来源

- **手动添加**：通过 `add_triple` 方法（不通过 MCP 暴露，仅内部调用），`source_memory_id` 可关联到记忆
- **代码扫描**：`kg_scan` 调用 `ast-grep` CLI，支持 16 种语言，`source_memory_id = '_scan_' + file_stem`
- **增量更新**：基于 `kg_scan_cache` 表的文件 size + mtime 判断，变化文件才重新扫描

## 依赖

```toml
petgraph = { version = "0.8.3", features = ["serde-1"] }  # 图数据结构 + 算法
notify = "7.0.0"                                           # 文件监控（kg_watch）
```

外部工具：`ast-grep (sg)` — 用于代码结构提取（`cargo install ast-grep`）

## 配置

```toml
# ~/.agentrete/config.toml
[knowledge_graph]
enabled = true
```

## CLI

```bash
# 扫描项目代码并构建知识图谱
agentrete scan /path/to/project

# 启动 MCP 服务后可通过 kg_scan/kg_query/kg_watch 交互
agentrete mcp --port 9092
```

## 实现历程

1. 初始：手动 `kg_triple_add` MCP 工具（已移除，意义不大）
2. 代码扫描：Tree-sitter 硬编码（已被 ast-grep 替代，零维护成本）
3. 图引擎：petgraph 替代直接 SQL 查询
4. 增量扫描：文件 hash + mtime 缓存，未变文件跳过
5. 后台任务：`kg_scan` 异步 spawn + `kg_scan_status`
6. 文件监控：`kg_watch start/stop` 基于 notify crate
7. 项目隔离：自动从 git 检测 project 名，SPO+project 唯一索引
