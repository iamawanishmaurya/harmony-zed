/// All SQL migrations run in order on first open.
/// Uses a `schema_version` table to track which migrations have run.

pub const MIGRATIONS: &[(&str, &str)] = &[
    ("v001_init", V001_INIT),
    ("v002_shadow_diffs", V002_SHADOW_DIFFS),
    ("v003_memory_records", V003_MEMORY_RECORDS),
    ("v004_overlap_events", V004_OVERLAP_EVENTS),
    ("v005_tasks", V005_TASKS),
    ("v006_machine_identity", V006_MACHINE_IDENTITY),
    ("v007_file_sync_events", V007_FILE_SYNC_EVENTS),
];

pub const SCHEMA_VERSION_TABLE: &str = "
CREATE TABLE IF NOT EXISTS schema_version (
    migration_id  TEXT PRIMARY KEY,
    applied_at    TEXT NOT NULL
);";

pub const V001_INIT: &str = "
CREATE TABLE IF NOT EXISTS agents (
    id            TEXT PRIMARY KEY,
    actor_id      TEXT NOT NULL UNIQUE,
    role_name     TEXT NOT NULL,
    role_avatar   TEXT NOT NULL,
    role_desc     TEXT NOT NULL DEFAULT '',
    status        TEXT NOT NULL DEFAULT 'idle',
    mode          TEXT NOT NULL DEFAULT 'shadow',
    task_prompt   TEXT,
    task_id       TEXT,
    memory_health TEXT NOT NULL DEFAULT 'good',
    spawned_at    TEXT NOT NULL,
    acp_endpoint  TEXT,
    session_id    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS provenance_tags (
    id            TEXT PRIMARY KEY,
    actor_id      TEXT NOT NULL,
    actor_kind    TEXT NOT NULL,
    task_id       TEXT,
    task_prompt   TEXT,
    timestamp     TEXT NOT NULL,
    file_path     TEXT NOT NULL,
    region_start_line  INTEGER NOT NULL,
    region_end_line    INTEGER NOT NULL,
    region_start_col   INTEGER NOT NULL DEFAULT 0,
    region_end_col     INTEGER NOT NULL DEFAULT 0,
    mode          TEXT NOT NULL DEFAULT 'shadow',
    diff_unified  TEXT NOT NULL DEFAULT '',
    session_id    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_prov_file ON provenance_tags(file_path);
CREATE INDEX IF NOT EXISTS idx_prov_actor ON provenance_tags(actor_id);
CREATE INDEX IF NOT EXISTS idx_prov_ts ON provenance_tags(timestamp);
";

pub const V002_SHADOW_DIFFS: &str = "
CREATE TABLE IF NOT EXISTS shadow_diffs (
    id            TEXT PRIMARY KEY,
    agent_id      TEXT NOT NULL REFERENCES agents(id),
    file_path     TEXT NOT NULL,
    diff_unified  TEXT NOT NULL,
    base_hash     TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending'
);

CREATE INDEX IF NOT EXISTS idx_shadow_agent ON shadow_diffs(agent_id);
CREATE INDEX IF NOT EXISTS idx_shadow_file ON shadow_diffs(file_path);
CREATE INDEX IF NOT EXISTS idx_shadow_status ON shadow_diffs(status);
";

pub const V003_MEMORY_RECORDS: &str = "
CREATE TABLE IF NOT EXISTS memory_records (
    id            TEXT PRIMARY KEY,
    content       TEXT NOT NULL,
    embedding     BLOB NOT NULL,
    namespace     TEXT NOT NULL,
    tags          TEXT NOT NULL,
    provenance_id TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mem_namespace ON memory_records(namespace);
";

pub const V004_OVERLAP_EVENTS: &str = "
CREATE TABLE IF NOT EXISTS overlap_events (
    id               TEXT PRIMARY KEY,
    file_path        TEXT NOT NULL,
    region_a_start   INTEGER NOT NULL,
    region_a_end     INTEGER NOT NULL,
    region_b_start   INTEGER NOT NULL,
    region_b_end     INTEGER NOT NULL,
    change_a_id      TEXT NOT NULL REFERENCES provenance_tags(id),
    change_b_id      TEXT NOT NULL REFERENCES provenance_tags(id),
    detected_at      TEXT NOT NULL,
    status           TEXT NOT NULL DEFAULT 'pending',
    impact_summary   TEXT,
    impact_complexity TEXT,
    resolution_kind  TEXT,
    resolved_at      TEXT,
    session_id       TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_overlap_status ON overlap_events(status);
CREATE INDEX IF NOT EXISTS idx_overlap_file ON overlap_events(file_path);
";

pub const V005_TASKS: &str = "
CREATE TABLE IF NOT EXISTS tasks (
    id              TEXT PRIMARY KEY,
    prompt          TEXT NOT NULL,
    agent_ids       TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'queued',
    error_message   TEXT,
    session_id      TEXT NOT NULL
);
";

pub const V006_MACHINE_IDENTITY: &str = "
ALTER TABLE provenance_tags ADD COLUMN machine_name TEXT NOT NULL DEFAULT 'local';
ALTER TABLE provenance_tags ADD COLUMN machine_ip TEXT NOT NULL DEFAULT '127.0.0.1';
ALTER TABLE agents ADD COLUMN machine_name TEXT NOT NULL DEFAULT 'local';
ALTER TABLE agents ADD COLUMN machine_ip TEXT NOT NULL DEFAULT '127.0.0.1';

CREATE INDEX IF NOT EXISTS idx_prov_machine ON provenance_tags(machine_name, machine_ip);
CREATE INDEX IF NOT EXISTS idx_agents_machine ON agents(machine_name, machine_ip);
";

pub const V007_FILE_SYNC_EVENTS: &str = "
CREATE TABLE IF NOT EXISTS file_sync_events (
    seq            INTEGER PRIMARY KEY AUTOINCREMENT,
    id             TEXT NOT NULL UNIQUE,
    relative_path  TEXT NOT NULL,
    entry_kind     TEXT NOT NULL,
    change_kind    TEXT NOT NULL,
    content_base64 TEXT,
    content_sha256 TEXT,
    size_bytes     INTEGER NOT NULL DEFAULT 0,
    actor_id       TEXT NOT NULL,
    machine_name   TEXT NOT NULL,
    machine_ip     TEXT NOT NULL,
    detected_at    TEXT NOT NULL,
    impact_summary TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_sync_seq ON file_sync_events(seq);
CREATE INDEX IF NOT EXISTS idx_sync_path ON file_sync_events(relative_path);
CREATE INDEX IF NOT EXISTS idx_sync_detected_at ON file_sync_events(detected_at);
";

/// SQLite connection settings (apply on every connection open)
pub const PRAGMAS: &str = "
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA foreign_keys=ON;
PRAGMA temp_store=memory;
PRAGMA cache_size=-64000;
";
