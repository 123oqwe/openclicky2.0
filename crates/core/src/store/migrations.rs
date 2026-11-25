//! Versioned, idempotent schema migrations.
//!
//! `user_version` is the single source of truth. Each migration runs inside a
//! transaction and the whole batch is preceded by a file backup (see
//! `Store::open`).

pub const LATEST: i32 = 3;

pub const M001: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL,
  kind TEXT NOT NULL DEFAULT 'ask'
);

CREATE TABLE IF NOT EXISTS messages (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  role TEXT NOT NULL,
  content TEXT NOT NULL,
  model TEXT,
  created_at TEXT NOT NULL,
  meta_json TEXT
);
CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, created_at);

CREATE TABLE IF NOT EXISTS tasks (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  goal TEXT NOT NULL,
  state TEXT NOT NULL,
  checkpoint_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  heartbeat_at TEXT,
  epoch INTEGER NOT NULL DEFAULT 1,
  units REAL NOT NULL DEFAULT 0,
  steps_done INTEGER NOT NULL DEFAULT 0,
  parent_session_id TEXT,
  result TEXT,
  error_code TEXT
);
CREATE INDEX IF NOT EXISTS idx_tasks_state ON tasks(state, updated_at);

CREATE TABLE IF NOT EXISTS task_steps (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  step INTEGER NOT NULL,
  kind TEXT NOT NULL,
  tool TEXT,
  request_json TEXT,
  result_json TEXT,
  ok INTEGER NOT NULL,
  code TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS usage_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,
  units REAL NOT NULL,
  task_id TEXT,
  session_id TEXT,
  waived INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  meta_json TEXT
);
CREATE INDEX IF NOT EXISTS idx_usage_created ON usage_events(created_at);

CREATE TABLE IF NOT EXISTS audit_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id TEXT,
  tool TEXT NOT NULL,
  risk TEXT NOT NULL,
  args_digest TEXT NOT NULL,
  outcome TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS metrics_daily (
  day TEXT NOT NULL,
  name TEXT NOT NULL,
  value REAL NOT NULL,
  PRIMARY KEY(day, name)
);

CREATE TABLE IF NOT EXISTS routines (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  schedule TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  next_run_at TEXT,
  run_count INTEGER NOT NULL DEFAULT 0,
  failures INTEGER NOT NULL DEFAULT 0,
  last_result TEXT,
  task_template TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS dict_entries (
  heard TEXT PRIMARY KEY,
  corrected TEXT NOT NULL,
  learned_at TEXT NOT NULL
);
"#;

/// M002: the columns the accepted decisions already promised, plus the
/// constraints that make the append-only and uniqueness claims true.
pub const M002: &str = r#"
ALTER TABLE tasks ADD COLUMN owner TEXT NOT NULL DEFAULT 'local';
ALTER TABLE tasks ADD COLUMN retries INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN prompt_version TEXT;
ALTER TABLE tasks ADD COLUMN tool_schema_version TEXT;
ALTER TABLE tasks ADD COLUMN waiting_since TEXT;

DELETE FROM task_steps
 WHERE id NOT IN (SELECT MIN(id) FROM task_steps GROUP BY task_id, step);
CREATE UNIQUE INDEX IF NOT EXISTS idx_task_steps_unique ON task_steps(task_id, step);

CREATE INDEX IF NOT EXISTS idx_routines_next ON routines(next_run_at);
CREATE INDEX IF NOT EXISTS idx_usage_task ON usage_events(task_id);

CREATE TRIGGER IF NOT EXISTS audit_log_no_update
BEFORE UPDATE ON audit_log
BEGIN
  SELECT RAISE(ABORT, 'audit_log is append-only');
END;
"#;

/// M003: the approval ledger, artifact ownership, and full-text search. All
/// three are things the UI has to answer after a restart, so they cannot live
/// only in the task checkpoint.
pub const M003: &str = r#"
ALTER TABLE tasks ADD COLUMN pause_reason TEXT;

CREATE TABLE IF NOT EXISTS approvals (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  step INTEGER NOT NULL,
  tool TEXT NOT NULL,
  risk TEXT NOT NULL,
  args_digest TEXT NOT NULL,
  prompt_json TEXT NOT NULL,
  decision TEXT NOT NULL DEFAULT 'pending',
  created_at TEXT NOT NULL,
  decided_at TEXT,
  consumed_at TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_approvals_step ON approvals(task_id, step);
CREATE INDEX IF NOT EXISTS idx_approvals_open ON approvals(decision, created_at);

CREATE TABLE IF NOT EXISTS artifacts (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  kind TEXT NOT NULL,
  bytes INTEGER NOT NULL,
  sha256 TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_artifacts_task ON artifacts(task_id);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
  content, content='messages', content_rowid='rowid'
);
CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
END;
CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
END;
CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
  INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
END;

INSERT INTO messages_fts(rowid, content) SELECT rowid, content FROM messages;
"#;

pub fn script(version: i32) -> Option<&'static str> {
    match version {
        1 => Some(M001),
        2 => Some(M002),
        3 => Some(M003),
        _ => None,
    }
}
