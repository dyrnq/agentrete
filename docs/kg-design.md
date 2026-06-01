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
    ├── git history 自动提取（commit/message/author/file 关系）
    └── file watcher（notify crate，扫描后可选启动）
```

## MCP 协议

### capabilities
```json
{
  "tools": {"listChanged": false},
  "tasks": {}
}
```

### Tools（9 个）

| 工具 | 功能 |
|------|------|
| `memory_search` | 核心记忆搜索（BM25 + 向量混合） |
| `memory_save` | 保存记忆 |
| `memory_list` | 列出记忆 |
| `memory_forget` | 删除记忆 |
| `memory_stats` | 统计信息 |
| `memory_compact` | 去重 + 压缩 |
| `kg_query` | 图查询：邻居遍历、最短路径 |
| `kg_scan` | **扫描代码 + 可选 watch**（`watch=true/false`） |
| `kg_scan_status` | 检查后台扫描状态 |

### Tasks（3 个方法）

| 方法 | 功能 |
|------|------|
| `tasks/send` | 创建 task（当前支持 `kg_scan`） |
| `tasks/cancel` | 取消运行中的 task |
| `tasks/status` | 查询单个或全部 task 状态 |

## 数据模型

### SQLite: kg_triples

```sql
CREATE TABLE IF NOT EXISTS kg_triples (
    id TEXT PRIMARY KEY,
    subject TEXT NOT NULL,
    predicate TEXT NOT NULL,
    object TEXT NOT NULL,
    confidence REAL DEFAULT 1.0,
    source_memory_id TEXT,       -- 关联记忆或 '_scan_' 前缀（扫描数据）
    project TEXT,                -- 项目名，自动从 git 检测，用于隔离
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_kg_triples_spo 
    ON kg_triples(subject, predicate, object, project);
```

### SQLite: kg_scan_cache（增量缓存）

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
```

## 数据来源

| 来源 | 方式 | source_memory_id |
|------|------|------------------|
| **代码扫描** | `ast-grep (sg)` CLI，支持 16 种语言 | `'_scan_' + file_stem` |
| **Git 历史** | 扫描时自动执行 `git log --name-only` | `'_scan_git_'` |
| **手动添加** | `add_triple()` 方法（内部调用，不通过 MCP 暴露） | 自定义 |

## 项目隔离

- 自动从 `git rev-parse --show-toplevel` 检测项目名
- `kg_triples.project` 字段 + `UNIQUE(subject, predicate, object, project)` 索引
- 不同项目的数据互不干扰

## 增量扫描

- `kg_scan_cache` 表记录文件 path → (hash, size, mtime)
- 文件未变则跳过，只扫描变更文件
- `watch=true` 时扫描完成后启动 `notify` 文件监控

## 依赖

```toml
petgraph = { version = "0.8.3", features = ["serde-1"] }
notify = "7.0.0"
futures = "0.3.32"       # catch_unwind 用于 task 保护
```

**外部工具**：`ast-grep (sg)` — `cargo install ast-grep`

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

# 启动 MCP 服务（HTTP 模式）
agentrete mcp --port 9092

# 扫描 + 文件监控
curl http://127.0.0.1:9092/ -d '{"method":"tasks/send","params":{"name":"kg_scan","arguments":{"path":".","watch":true}}}'

# 查询 task 状态
curl http://127.0.0.1:9092/ -d '{"method":"tasks/status","params":{"id":"task_0001"}}'

# 取消 task
curl http://127.0.0.1:9092/ -d '{"method":"tasks/cancel","params":{"id":"task_0001"}}'
```

## 实现历程

1. 初始：手动 `kg_triple_add` MCP 工具（已移除，意义不大）
2. 代码扫描：Tree-sitter 硬编码 → `ast-grep` CLI（零维护成本）
3. 图引擎：petgraph 替代直接 SQL 查询
4. 增量扫描：文件 hash + mtime 缓存，未变文件跳过
5. 后台任务：`tokio::spawn` + `AtomicBool` 防并发
6. 文件监控：`kg_watch` 合并到 `kg_scan watch=true`
7. 项目隔离：自动从 git 检测 project 名，SPO+project 唯一索引
8. Git 历史：扫描时自动提取 commit/message/author/file 关系
9. MCP Task 标准：`tasks/send/cancel/status` + `capabilities.tasks`
10. 进程保护：`catch_unwind` panic hook 确保后台 task 崩溃不杀死 MCP 服务
