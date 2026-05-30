# Knowledge Graph 模块设计

> **状态**: 设计阶段，未实现
> **目的**: 在 agentrete 中新增一个可选的 SPO 知识图谱层，与核心记忆引擎并列

## 动机

当前 agentrete 的记忆是扁平的——每条记忆是独立的文本块，靠 vec0 语义搜索和 FTS5 全文搜索检索。这种模式对"搜索一条规则/决策"很高效，但无法回答**关系型问题**：

- "这个项目用了哪些库？"
- "这条决策影响了哪些模块？"
- "哪些已知 bug 和 sqlx 相关？"

这些问题需要的是**结构化关系**而非文本语义匹配。tag 搜索可以近似回答，但随着记忆增长，精度和效率都会下降。

## 设计原则

1. **与 Memory Engine 解耦** — KG 是独立的存储层，不是记忆的附属品。记忆可以没有图，图可以没有记忆
2. **纯 Rust，零外部依赖** — 延续 agentrete 的单二进制理念
3. **按需启用** — 默认不开启，用户需要时才配置/启用
4. **SQLite 持久化 + petgraph 计算** — 三元组存 SQLite，图算法跑在 petgraph

## 架构

```
agentrete
├── Memory Engine（核心）
│   └── SQLite + vec0 + FTS5 → rules/decisions/patterns/bugs
│
└── Knowledge Graph（新模块，可选）
    ├── SQLite triples 表（单一数据源，持久化）
    └── petgraph DiGraph（内存，做图算法，build_graph() 从 SQLite 构建）
```

持久化与计算分离：

```
kg_triple_add
    │
    ▼
SQLite triples 表 ◄── 唯一写入目标，事务保证
    │
    │ (启动时 / 按需)
    ▼
build_graph() ──▶ petgraph DiGraph ──▶ 邻居遍历 / 最短路径 / 社区检测
    ▲
    │ (dirty 标记后重建)
    └──────────────────────────┘
```

## 数据模型

### SQLite 表

与 `memories` 同 DB，使用已有的 sqlx pool：

```sql
-- 三元组表（SPO）
CREATE TABLE IF NOT EXISTS kg_triples (
    id TEXT PRIMARY KEY,             -- uuid
    subject TEXT NOT NULL,           -- 实体 ID
    predicate TEXT NOT NULL,         -- 关系类型
    object TEXT NOT NULL,            -- 实体 ID
    confidence REAL DEFAULT 1.0,     -- 0.0-1.0
    source_memory_id TEXT,           -- 可选，关联到 memories.id（无外键约束）
    project TEXT,                    -- 项目名，和 memories.project 对齐
    created_at TEXT NOT NULL
);

-- 实体表（可选，用于描述实体属性）
CREATE TABLE IF NOT EXISTS kg_entities (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    labels TEXT NOT NULL DEFAULT '[]',   -- JSON 数组：["project","crate"]
    description TEXT,
    created_at TEXT NOT NULL
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_kg_triples_subject ON kg_triples(subject);
CREATE INDEX IF NOT EXISTS idx_kg_triples_object ON kg_triples(object);
CREATE INDEX IF NOT EXISTS idx_kg_triples_predicate ON kg_triples(predicate);
CREATE INDEX IF NOT EXISTS idx_kg_triples_memory ON kg_triples(source_memory_id);
CREATE INDEX IF NOT EXISTS idx_kg_triples_project ON kg_triples(project);
```

### petgraph 运行时结构

```rust
use petgraph::graph::DiGraph;
use std::sync::RwLock;

pub struct KnowledgeGraph {
    /// petgraph 有向图（从 SQLite 构建）
    graph: RwLock<DiGraph<Entity, TripleEdge>>,
    /// 实体 ID → NodeIndex 映射
    node_index: RwLock<HashMap<String, NodeIndex>>,
    /// SQLite 中数据是否比 petgraph 新
    dirty: AtomicBool,
    enabled: bool,
}

// 图中节点
pub struct Entity {
    pub id: String,
    pub name: String,
    pub labels: Vec<String>,
}

// 图中边
pub struct TripleEdge {
    pub predicate: String,
    pub confidence: f32,
    pub source_memory_id: Option<String>,
}
```

### 构建流程

```rust
impl KnowledgeGraph {
    /// 从 SQLite triples 表构建 petgraph
    pub fn build_graph(pool: &SqlitePool) -> Result<Self> {
        // 1. 查询所有实体
        // 2. 查询所有三元组
        // 3. 构建 DiGraph
        // 4. 构建 node_index HashMap
    }

    /// 增删改后标记 dirty，下次查询前自动重建
    pub fn mark_dirty(&self) { ... }
}
```

### 查询效率

| 查询 | SQLite 直接查 | petgraph 查 |
|------|-------------|------------|
| "agentrete 的 uses 邻居" | `SELECT object FROM kg_triples WHERE subject='agentrete' AND predicate='uses'` | `graph.neighbors(node_idx)` |
| "哪些实体引用了 sqlx" | `SELECT subject FROM kg_triples WHERE object='sqlx'` | 反向邻居遍历 |
| "agentrete 和 sqlx 最短路径" | ❌ 需要递归 CTE | ✅ `petgraph::algo::dijkstra` |
| "所有标签为 crate 的实体" | `SELECT * FROM kg_entities WHERE labels LIKE '%crate%'` | ❌ 需遍历所有节点 |
| 100 条三元组 | 0.1ms | 0.01ms |
| 10000 条三元组 | 2ms | 0.1ms |

**混合策略**：精确 ID 查询走 SQLite（不用全量加载），图遍历/路径/社区检测走 petgraph（需 build_graph）。

## MCP 工具

新增 2 个：

### `kg_query`

| 参数 | 类型 | 说明 |
|------|------|------|
| `mode` | `neighbors` / `path` / `subgraph` | 查询模式 |
| `entity` | string | 起始实体 ID |
| `target` | string | 目标实体（path 模式） |
| `predicate` | string | 过滤关系类型 |
| `direction` | `outgoing` / `incoming` / `both` | 边的方向 |
| `project` | string | 项目过滤（可选，不传则查全局） |

返回 JSON：

```json
// neighbors 模式
{
  "entity": "agentrete",
  "relations": [
    {"direction": "out", "predicate": "uses", "target": "sqlx", "confidence": 1.0},
    {"direction": "out", "predicate": "deprecated", "target": "rusqlite", "confidence": 0.9}
  ]
}

// path 模式
{
  "path": ["agentrete", "uses", "sqlx"],
  "length": 1
}
```

### `kg_triple_add`

| 参数 | 类型 | 说明 |
|------|------|------|
| `subject` | string | 主体实体 ID |
| `predicate` | string | 关系类型 |
| `object` | string | 客体实体 ID |
| `confidence` | number | 置信度（默认 1.0） |
| `source_memory_id` | string | 来源记忆 ID（可选） |
| `project` | string | 项目名（可选，自动从 git 检测） |

### 搜索联动

`memory_search` 返回结果时，如果结果记忆有相关的三元组（`source_memory_id` 匹配），在文本结果尾部追加图上下文：

```
[decision] Use sqlx instead of rusqlite (score=0.92)
  ├─ tags:tech-stack  imp:4  at:2026-05-30  proj:agentrete
  └─ kg: agentrete → uses → sqlx
         agentrete → deprecated → rusqlite
```

## 配置

新增配置段，默认关闭：

```toml
[knowledge_graph]
enabled = false
```

## 实现计划

### Phase 1 — 存储层

- [ ] 添加 `petgraph = { version = "0.8.3", features = ["serde-1"] }` 依赖
- [ ] 实现 `KnowledgeGraph` struct
- [ ] `initialize()` 创建 `kg_triples` + `kg_entities` 表
- [ ] `add_triple()` → INSERT INTO kg_triples + mark_dirty
- [ ] `build_graph()` → 从 SQLite 全量加载 → petgraph DiGraph
- [ ] `query_neighbors(entity, predicate, direction)` → petgraph 遍历
- [ ] `query_path(from, to)` → petgraph dijkstra

### Phase 2 — MCP 工具

- [ ] `kg_query` MCP 工具（neighbors / path / subgraph）
- [ ] `kg_triple_add` MCP 工具
- [ ] `memory_search` 联动输出图上下文
- [ ] 配置 `[knowledge_graph] enabled`

### Phase 3 — 增强（可选）

- [ ] `kg_query --mode community` — Leiden 社区检测（通过 `mnem-graphrag`）
- [ ] `kg_query --mode subgraph --format mermaid` — ASCII 图可视化
- [ ] `kg_entity_merge` — 实体去重/合并
- [ ] `kg_forget` — 按实体/关系删除

## 开放问题

1. **删除策略** — `source_memory_id` 关联的记忆被删除时，三元组是否自动清理？暂定：不级联，提供 `kg_cleanup` 手动清理孤立的 memory 引用
2. **并发写安全** — petgraph 用 `RwLock` 保护，单次写加写锁。HTTP MCP 模式下请求交替，不会死锁
3. **实体自动创建** — `kg_triple_add` 时如果 subject/object 在 `kg_entities` 中不存在，是否自动创建？暂定：不自动创建，建议先 `kg_entity_add` 再 `kg_triple_add`
