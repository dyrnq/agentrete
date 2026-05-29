-- agentrete migration v1: initial schema
-- Creates the core tables: memories, sessions, observations

CREATE TABLE IF NOT EXISTS memories (
    id              VARCHAR PRIMARY KEY,
    session_id      VARCHAR,
    type            VARCHAR,
    content         TEXT NOT NULL,
    tags            VARCHAR[],
    files           VARCHAR[],
    project         VARCHAR,
    importance      FLOAT DEFAULT 0.5,
    embedding       FLOAT[],             -- variable-length, any model dimension
    embedding_model VARCHAR,             -- e.g. "bge-m3", "qwen3-embedding"
    embedding_dims  INTEGER,             -- actual dimension (e.g. 1024)
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(type);
CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project);
CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at);
CREATE INDEX IF NOT EXISTS idx_memories_embed_model ON memories(embedding_model);

CREATE TABLE IF NOT EXISTS sessions (
    id          VARCHAR PRIMARY KEY,
    project     VARCHAR,
    turn_count  INTEGER DEFAULT 0,
    started_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    ended_at    TIMESTAMP
);

CREATE TABLE IF NOT EXISTS observations (
    id          VARCHAR PRIMARY KEY,
    session_id  VARCHAR,
    seq         INTEGER,
    tool        VARCHAR,
    input       TEXT,
    output      TEXT,
    status      INTEGER,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_obs_session ON observations(session_id);
CREATE INDEX IF NOT EXISTS idx_obs_created ON observations(created_at);

-- Schema version tracking with crate version
CREATE TABLE IF NOT EXISTS _schema_version (
    version       INTEGER PRIMARY KEY,
    crate_version VARCHAR NOT NULL,
    migrated_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO _schema_version (version, crate_version) VALUES (1, '0.1.0');
