//! SQLite persistence. The database is the authority for messages, tasks,
//! usage and audit records (ADR-0008).

pub mod migrations;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use crate::error::{Error, Result};
use crate::security;

#[derive(Debug, Clone, PartialEq)]
pub struct MessageRow {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub model: Option<String>,
    pub created_at: DateTime<Utc>,
    pub meta_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageRow {
    pub kind: String,
    pub units: f64,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
    pub waived: bool,
    pub created_at: DateTime<Utc>,
    pub meta_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskRow {
    pub id: String,
    pub name: String,
    pub goal: String,
    pub state: String,
    pub checkpoint_json: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub epoch: i64,
    pub units: f64,
    pub steps_done: i64,
    pub result: Option<String>,
    pub error_code: Option<String>,
    pub owner: String,
    pub retries: i64,
    pub prompt_version: Option<String>,
    pub tool_schema_version: Option<String>,
    pub waiting_since: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepRecord {
    pub task_id: String,
    pub step: i64,
    pub kind: String,
    pub tool: Option<String>,
    pub request_json: Option<String>,
    pub result_json: Option<String>,
    pub ok: bool,
    pub code: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuditRow {
    pub task_id: Option<String>,
    pub tool: String,
    pub risk: String,
    pub args_digest: String,
    pub outcome: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoutineRow {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub enabled: bool,
    pub next_run_at: Option<DateTime<Utc>>,
    pub run_count: i64,
    pub failures: i64,
    pub last_result: Option<String>,
    pub task_template: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskStateUpdate {
    pub task_id: String,
    pub state: String,
    pub checkpoint_json: String,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub units: Option<f64>,
    pub steps_done: Option<i64>,
    pub result: Option<String>,
    pub error_code: Option<String>,
    pub expected_epoch: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Turn {
    pub session_id: String,
    pub messages: Vec<MessageRow>,
    pub usage: Option<UsageRow>,
    pub task_update: Option<TaskStateUpdate>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeletionReport {
    pub steps: Vec<(String, bool, String)>,
}

impl DeletionReport {
    pub fn all_ok(&self) -> bool {
        self.steps.iter().all(|(_, ok, _)| *ok)
    }
    pub fn failures(&self) -> Vec<String> {
        self.steps
            .iter()
            .filter(|(_, ok, _)| !*ok)
            .map(|(n, _, e)| format!("{n}: {e}"))
            .collect()
    }
}

pub struct Store {
    conn: Mutex<Option<Connection>>,
    path: Option<std::path::PathBuf>,
}

/// Held connection. Exists so callers never see the `Option` that `close()`
/// empties, and so a poisoned mutex surfaces as an error instead of a panic
/// that would take every later store call down with it.
struct Conn<'a>(MutexGuard<'a, Option<Connection>>);

impl Deref for Conn<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.0
            .as_ref()
            .expect("conn_guard() rejects a closed store")
    }
}

impl DerefMut for Conn<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        self.0
            .as_mut()
            .expect("conn_guard() rejects a closed store")
    }
}

impl Store {
    fn conn_guard(&self) -> Result<Conn<'_>> {
        let guard = self
            .conn
            .lock()
            .map_err(|_| Error::Store("数据库连接已被污染，请重启应用".into()))?;
        if guard.is_none() {
            return Err(Error::Store("数据库已关闭".into()));
        }
        Ok(Conn(guard))
    }

    /// Release the SQLite handle. Account deletion has to call this before it
    /// removes the files, because Windows refuses to unlink an open handle
    /// (os error 32). Idempotent: closing twice is a no-op.
    pub fn close(&self) -> Result<()> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| Error::Store("数据库连接已被污染，请重启应用".into()))?;
        match guard.take() {
            Some(conn) => conn.close().map_err(|(_, e)| map_err(e)),
            None => Ok(()),
        }
    }

    /// Test seam: a table that refuses to empty is the only way to prove the
    /// deletion report tells the truth about a failed step.
    #[cfg(test)]
    pub fn drop_table_for_test(&self, table: &str) -> Result<()> {
        self.conn_guard()?
            .execute_batch(&format!("DROP TABLE {table}"))
            .map_err(map_err)
    }

    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::Io(e.to_string()))?;
        }
        let backup = path.with_extension("db.bak");
        if path.exists() {
            let _ = std::fs::copy(path, &backup);
        }
        let conn = Connection::open(path).map_err(map_err)?;
        let store = Self {
            conn: Mutex::new(Some(conn)),
            path: Some(path.to_path_buf()),
        };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(map_err)?;
        let store = Self {
            conn: Mutex::new(Some(conn)),
            path: None,
        };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn configure(&self) -> Result<()> {
        let conn = self.conn_guard()?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(map_err)?;
        if self.path.is_some() {
            conn.pragma_update(None, "journal_mode", "WAL")
                .map_err(map_err)?;
        }
        conn.busy_timeout(std::time::Duration::from_millis(2000))
            .map_err(map_err)?;
        Ok(())
    }

    pub fn version(&self) -> Result<i32> {
        let conn = self.conn_guard()?;
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(map_err)
    }

    pub fn migrate(&self) -> Result<()> {
        let mut conn = self.conn_guard()?;
        let current: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(map_err)?;
        if current > migrations::LATEST {
            return Err(Error::Store(format!(
                "数据库由更新版本创建（v{current} > v{}），本版本只能以只读方式使用",
                migrations::LATEST
            )));
        }
        for v in (current + 1)..=migrations::LATEST {
            let Some(script) = migrations::script(v) else {
                return Err(Error::Store(format!("缺少迁移脚本 v{v}")));
            };
            let tx = conn.transaction().map_err(map_err)?;
            tx.execute_batch(script).map_err(map_err)?;
            tx.pragma_update(None, "user_version", v).map_err(map_err)?;
            tx.commit().map_err(map_err)?;
        }
        Ok(())
    }

    pub fn insert_session(&self, kind: &str, now: DateTime<Utc>) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let conn = self.conn_guard()?;
        conn.execute(
            "INSERT INTO sessions (id, created_at, kind) VALUES (?1, ?2, ?3)",
            params![id, now.to_rfc3339(), kind],
        )
        .map_err(map_err)?;
        Ok(id)
    }

    pub fn append_message(&self, m: &MessageRow) -> Result<()> {
        let conn = self.conn_guard()?;
        exec_message(&conn, m)
    }

    /// Single transaction: messages + usage + task state. Prevents "reply saved
    /// but usage not recorded" (INV-D2).
    pub fn commit_turn(&self, turn: &Turn) -> Result<()> {
        let mut conn = self.conn_guard()?;
        let tx = conn.transaction().map_err(map_err)?;
        for m in &turn.messages {
            exec_message(&tx, m)?;
        }
        if let Some(u) = &turn.usage {
            tx.execute(
                "INSERT INTO usage_events (kind, units, task_id, session_id, waived, created_at, meta_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![u.kind, u.units, u.task_id, u.session_id, u.waived as i64, u.created_at.to_rfc3339(), u.meta_json],
            )
            .map_err(map_err)?;
        }
        if let Some(t) = &turn.task_update {
            let changed = tx
                .execute(
                    "UPDATE tasks SET state=?2, checkpoint_json=?3, heartbeat_at=?4,
                        units=COALESCE(?5, units), steps_done=COALESCE(?6, steps_done),
                        result=?7, error_code=?8, updated_at=?9
                     WHERE id=?1 AND epoch=?10",
                    params![
                        t.task_id,
                        t.state,
                        t.checkpoint_json,
                        t.heartbeat_at.map(|h| h.to_rfc3339()),
                        t.units,
                        t.steps_done,
                        t.result,
                        t.error_code,
                        Utc::now().to_rfc3339(),
                        t.expected_epoch
                    ],
                )
                .map_err(map_err)?;
            if changed == 0 {
                return Err(Error::State("任务所有权已失效（epoch 过期）".into()));
            }
        }
        tx.commit().map_err(map_err)
    }

    pub fn recent_messages(&self, session_id: &str, limit: usize) -> Result<Vec<MessageRow>> {
        let conn = self.conn_guard()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, role, content, model, created_at, meta_json
                 FROM messages WHERE session_id=?1 ORDER BY created_at DESC, rowid DESC LIMIT ?2",
            )
            .map_err(map_err)?;
        let rows = stmt
            .query_map(params![session_id, limit as i64], |r| {
                Ok(MessageRow {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    role: r.get(2)?,
                    content: r.get(3)?,
                    model: r.get(4)?,
                    created_at: parse_time(r.get::<_, String>(5)?),
                    meta_json: r.get(6)?,
                })
            })
            .map_err(map_err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(map_err)?);
        }
        out.reverse();
        Ok(out)
    }

    pub fn upsert_task(&self, t: &TaskRow) -> Result<()> {
        let conn = self.conn_guard()?;
        conn.execute(
            "INSERT INTO tasks (id, name, goal, state, checkpoint_json, created_at, updated_at,
                                heartbeat_at, epoch, units, steps_done, result, error_code, owner, retries, prompt_version, tool_schema_version, waiting_since)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
             ON CONFLICT(id) DO UPDATE SET
                name=excluded.name, goal=excluded.goal, state=excluded.state,
                checkpoint_json=excluded.checkpoint_json, updated_at=excluded.updated_at,
                heartbeat_at=excluded.heartbeat_at, epoch=excluded.epoch, units=excluded.units,
                steps_done=excluded.steps_done, result=excluded.result, error_code=excluded.error_code,
                retries=excluded.retries, prompt_version=excluded.prompt_version,
                tool_schema_version=excluded.tool_schema_version,
                waiting_since=excluded.waiting_since",
            params![
                t.id, t.name, t.goal, t.state, t.checkpoint_json,
                t.created_at.to_rfc3339(), t.updated_at.to_rfc3339(),
                t.heartbeat_at.map(|h| h.to_rfc3339()), t.epoch, t.units, t.steps_done,
                t.result, t.error_code, t.owner, t.retries, t.prompt_version,
                t.tool_schema_version, t.waiting_since.map(|w| w.to_rfc3339())
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    pub fn task(&self, id: &str) -> Result<Option<TaskRow>> {
        let conn = self.conn_guard()?;
        conn.query_row(
            "SELECT id, name, goal, state, checkpoint_json, created_at, updated_at, heartbeat_at,
                    epoch, units, steps_done, result, error_code, owner, retries, prompt_version, tool_schema_version, waiting_since FROM tasks WHERE id=?1",
            params![id],
            row_to_task,
        )
        .optional()
        .map_err(map_err)
    }

    pub fn list_tasks(&self, state: Option<&str>) -> Result<Vec<TaskRow>> {
        let conn = self.conn_guard()?;
        let sql = match state {
            Some(_) => "SELECT id, name, goal, state, checkpoint_json, created_at, updated_at, heartbeat_at, epoch, units, steps_done, result, error_code, owner, retries, prompt_version, tool_schema_version, waiting_since FROM tasks WHERE state=?1 ORDER BY updated_at DESC",
            None => "SELECT id, name, goal, state, checkpoint_json, created_at, updated_at, heartbeat_at, epoch, units, steps_done, result, error_code, owner, retries, prompt_version, tool_schema_version, waiting_since FROM tasks ORDER BY updated_at DESC",
        };
        let mut stmt = conn.prepare(sql).map_err(map_err)?;
        let mut out = Vec::new();
        if let Some(s) = state {
            let rows = stmt.query_map(params![s], row_to_task).map_err(map_err)?;
            for r in rows {
                out.push(r.map_err(map_err)?);
            }
        } else {
            let rows = stmt.query_map([], row_to_task).map_err(map_err)?;
            for r in rows {
                out.push(r.map_err(map_err)?);
            }
        }
        Ok(out)
    }

    pub fn record_step(&self, s: &StepRecord) -> Result<()> {
        let conn = self.conn_guard()?;
        conn.execute(
            "INSERT INTO task_steps (task_id, step, kind, tool, request_json, result_json, ok, code, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![s.task_id, s.step, s.kind, s.tool, s.request_json, s.result_json, s.ok as i64, s.code, s.created_at.to_rfc3339()],
        )
        .map_err(map_err)?;
        Ok(())
    }

    pub fn steps(&self, task_id: &str) -> Result<Vec<StepRecord>> {
        let conn = self.conn_guard()?;
        let mut stmt = conn
            .prepare("SELECT task_id, step, kind, tool, request_json, result_json, ok, code, created_at FROM task_steps WHERE task_id=?1 ORDER BY id")
            .map_err(map_err)?;
        let rows = stmt
            .query_map(params![task_id], |r| {
                Ok(StepRecord {
                    task_id: r.get(0)?,
                    step: r.get(1)?,
                    kind: r.get(2)?,
                    tool: r.get(3)?,
                    request_json: r.get(4)?,
                    result_json: r.get(5)?,
                    ok: r.get::<_, i64>(6)? != 0,
                    code: r.get(7)?,
                    created_at: parse_time(r.get::<_, String>(8)?),
                })
            })
            .map_err(map_err)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(map_err)?);
        }
        Ok(out)
    }

    pub fn audit(&self, a: &AuditRow) -> Result<()> {
        let conn = self.conn_guard()?;
        conn.execute(
            "INSERT INTO audit_log (task_id, tool, risk, args_digest, outcome, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![a.task_id, a.tool, a.risk, a.args_digest, a.outcome, a.created_at.to_rfc3339()],
        )
        .map_err(map_err)?;
        Ok(())
    }

    pub fn usage_between(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<UsageRow>> {
        let conn = self.conn_guard()?;
        let mut stmt = conn
            .prepare("SELECT kind, units, task_id, session_id, waived, created_at, meta_json FROM usage_events WHERE created_at >= ?1 AND created_at <= ?2")
            .map_err(map_err)?;
        let rows = stmt
            .query_map(params![from.to_rfc3339(), to.to_rfc3339()], |r| {
                Ok(UsageRow {
                    kind: r.get(0)?,
                    units: r.get(1)?,
                    task_id: r.get(2)?,
                    session_id: r.get(3)?,
                    waived: r.get::<_, i64>(4)? != 0,
                    created_at: parse_time(r.get::<_, String>(5)?),
                    meta_json: r.get(6)?,
                })
            })
            .map_err(map_err)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(map_err)?);
        }
        Ok(out)
    }

    pub fn bump_metric(&self, day: &str, name: &str, delta: f64) -> Result<()> {
        let conn = self.conn_guard()?;
        conn.execute(
            "INSERT INTO metrics_daily (day, name, value) VALUES (?1,?2,?3)
             ON CONFLICT(day, name) DO UPDATE SET value = value + excluded.value",
            params![day, name, delta],
        )
        .map_err(map_err)?;
        Ok(())
    }

    pub fn metric(&self, day: &str, name: &str) -> Result<f64> {
        let conn = self.conn_guard()?;
        let v: Option<f64> = conn
            .query_row(
                "SELECT value FROM metrics_daily WHERE day=?1 AND name=?2",
                params![day, name],
                |r| r.get(0),
            )
            .optional()
            .map_err(map_err)?;
        Ok(v.unwrap_or(0.0))
    }

    pub fn upsert_dict(&self, heard: &str, corrected: &str, now: DateTime<Utc>) -> Result<()> {
        let conn = self.conn_guard()?;
        conn.execute(
            "INSERT INTO dict_entries (heard, corrected, learned_at) VALUES (?1,?2,?3)
             ON CONFLICT(heard) DO UPDATE SET corrected=excluded.corrected, learned_at=excluded.learned_at",
            params![heard, corrected, now.to_rfc3339()],
        )
        .map_err(map_err)?;
        Ok(())
    }

    pub fn dict(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn_guard()?;
        let mut stmt = conn
            .prepare("SELECT heard, corrected FROM dict_entries ORDER BY learned_at DESC")
            .map_err(map_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(map_err)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(map_err)?);
        }
        Ok(out)
    }

    pub fn upsert_routine(&self, r: &RoutineRow) -> Result<()> {
        let conn = self.conn_guard()?;
        conn.execute(
            "INSERT INTO routines (id, name, schedule, enabled, next_run_at, run_count, failures, last_result, task_template)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, schedule=excluded.schedule,
               enabled=excluded.enabled, next_run_at=excluded.next_run_at, run_count=excluded.run_count,
               failures=excluded.failures, last_result=excluded.last_result, task_template=excluded.task_template",
            params![
                r.id, r.name, r.schedule, r.enabled as i64,
                r.next_run_at.map(|t| t.to_rfc3339()), r.run_count, r.failures, r.last_result, r.task_template
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    pub fn routines(&self) -> Result<Vec<RoutineRow>> {
        let conn = self.conn_guard()?;
        let mut stmt = conn
            .prepare("SELECT id, name, schedule, enabled, next_run_at, run_count, failures, last_result, task_template FROM routines ORDER BY name")
            .map_err(map_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(RoutineRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    schedule: r.get(2)?,
                    enabled: r.get::<_, i64>(3)? != 0,
                    next_run_at: r.get::<_, Option<String>>(4)?.map(parse_time),
                    run_count: r.get(5)?,
                    failures: r.get(6)?,
                    last_result: r.get(7)?,
                    task_template: r.get(8)?,
                })
            })
            .map_err(map_err)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(map_err)?);
        }
        Ok(out)
    }

    /// Delete every row, then report per-table counts so the caller can prove it.
    pub fn delete_everything(&self) -> Result<DeletionReport> {
        let mut report = DeletionReport::default();
        let tables = [
            "messages",
            "sessions",
            "task_steps",
            "tasks",
            "usage_events",
            "audit_log",
            "metrics_daily",
            "routines",
            "dict_entries",
        ];
        let mut conn = self.conn_guard()?;
        let tx = conn.transaction().map_err(map_err)?;
        for t in tables {
            match tx.execute(&format!("DELETE FROM {t}"), []) {
                Ok(_) => report.steps.push((t.to_string(), true, String::new())),
                Err(e) => report.steps.push((t.to_string(), false, e.to_string())),
            }
        }
        tx.commit().map_err(map_err)?;
        Ok(report)
    }

    /// Record when a task entered `waiting_approval`/`waiting_user` so the
    /// watchdog can expire it instead of parking forever.
    pub fn mark_waiting(&self, id: &str, epoch: i64, since: DateTime<Utc>) -> Result<()> {
        let conn = self.conn_guard()?;
        let changed = conn
            .execute(
                "UPDATE tasks SET waiting_since=?3, updated_at=?3 WHERE id=?1 AND epoch=?2",
                params![id, epoch, since.to_rfc3339()],
            )
            .map_err(map_err)?;
        if changed == 0 {
            return Err(Error::State("任务所有权已失效（epoch 过期）".into()));
        }
        Ok(())
    }

    /// Clear the waiting marker whenever the task leaves a waiting state.
    pub fn clear_waiting(&self, id: &str) -> Result<()> {
        let conn = self.conn_guard()?;
        conn.execute(
            "UPDATE tasks SET waiting_since=NULL WHERE id=?1",
            params![id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    /// Tasks that have been waiting longer than the cutoff. Used by the
    /// watchdog to move them to `Paused(waiting_timeout)`.
    pub fn stale_waiting(&self, cutoff: DateTime<Utc>) -> Result<Vec<TaskRow>> {
        let conn = self.conn_guard()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, name, goal, state, checkpoint_json, created_at, updated_at, heartbeat_at,
                        epoch, units, steps_done, result, error_code, owner, retries, prompt_version,
                        tool_schema_version, waiting_since
                 FROM tasks WHERE waiting_since IS NOT NULL AND waiting_since < ?1",
            )
            .map_err(map_err)?;
        let rows = stmt
            .query_map(params![cutoff.to_rfc3339()], row_to_task)
            .map_err(map_err)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(map_err)?);
        }
        Ok(out)
    }

    pub fn checkpoint_wal(&self) -> Result<()> {
        let conn = self.conn_guard()?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(map_err)
    }
}

fn map_err(e: rusqlite::Error) -> Error {
    Error::Store(e.to_string())
}

fn parse_time(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn exec_message(conn: &Connection, m: &MessageRow) -> Result<()> {
    conn.execute(
        "INSERT INTO messages (id, session_id, role, content, model, created_at, meta_json) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![m.id, m.session_id, m.role, m.content, m.model, m.created_at.to_rfc3339(), m.meta_json],
    )
    .map_err(map_err)?;
    Ok(())
}

fn row_to_task(r: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRow> {
    Ok(TaskRow {
        id: r.get(0)?,
        name: r.get(1)?,
        goal: r.get(2)?,
        state: r.get(3)?,
        checkpoint_json: r.get(4)?,
        created_at: parse_time(r.get::<_, String>(5)?),
        updated_at: parse_time(r.get::<_, String>(6)?),
        heartbeat_at: r.get::<_, Option<String>>(7)?.map(parse_time),
        epoch: r.get(8)?,
        units: r.get(9)?,
        steps_done: r.get(10)?,
        result: r.get(11)?,
        error_code: r.get(12)?,
        owner: r.get(13)?,
        retries: r.get(14)?,
        prompt_version: r.get(15)?,
        tool_schema_version: r.get(16)?,
        waiting_since: r.get::<_, Option<String>>(17)?.map(parse_time),
    })
}

/// Digest helper so audit rows never store raw arguments (ADR-0009).
pub fn args_digest(args: &serde_json::Value) -> String {
    security::digest_hex(&serde_json::to_string(args).unwrap_or_default())
}

impl Store {
    pub fn insert_usage(&self, u: &UsageRow) -> Result<()> {
        let conn = self.conn_guard()?;
        conn.execute(
            "INSERT INTO usage_events (kind, units, task_id, session_id, waived, created_at, meta_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![u.kind, u.units, u.task_id, u.session_id, u.waived as i64, u.created_at.to_rfc3339(), u.meta_json],
        )
        .map_err(map_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Store {
        Store::open_memory().unwrap()
    }

    fn task_row(id: &str) -> TaskRow {
        let now = Utc::now();
        TaskRow {
            id: id.to_string(),
            name: "n".into(),
            goal: "g".into(),
            state: "queued".into(),
            checkpoint_json: "{}".into(),
            created_at: now,
            updated_at: now,
            heartbeat_at: None,
            epoch: 1,
            units: 0.0,
            steps_done: 0,
            result: None,
            error_code: None,
            owner: "local".into(),
            retries: 0,
            prompt_version: None,
            tool_schema_version: None,
            waiting_since: None,
        }
    }

    fn step(task: &str, n: i64) -> StepRecord {
        StepRecord {
            task_id: task.to_string(),
            step: n,
            kind: "tool".into(),
            tool: Some("fs_read".into()),
            request_json: None,
            result_json: None,
            ok: true,
            code: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn migration_adds_the_columns_the_decisions_promised() {
        let s = mem();
        assert_eq!(s.version().unwrap(), 2);
        let conn = s.conn_guard().unwrap();
        let mut stmt = conn.prepare("PRAGMA table_info(tasks)").unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for c in [
            "owner",
            "retries",
            "prompt_version",
            "tool_schema_version",
            "waiting_since",
        ] {
            assert!(cols.iter().any(|x| x == c), "{c} missing from tasks");
        }
    }

    #[test]
    fn duplicate_step_rows_are_rejected() {
        let s = mem();
        s.upsert_task(&task_row("t1")).unwrap();
        s.record_step(&step("t1", 1)).unwrap();
        assert!(s.record_step(&step("t1", 1)).is_err());
        s.record_step(&step("t1", 2)).unwrap();
    }

    #[test]
    fn audit_log_rejects_updates_but_deletion_flow_still_runs() {
        let s = mem();
        s.audit(&AuditRow {
            task_id: None,
            tool: "fs_read".into(),
            risk: "read".into(),
            args_digest: "abc".into(),
            outcome: "ok".into(),
            created_at: Utc::now(),
        })
        .unwrap();
        {
            let conn = s.conn_guard().unwrap();
            let err = conn
                .execute("UPDATE audit_log SET outcome='tampered'", [])
                .unwrap_err();
            assert!(err.to_string().contains("append-only"), "{err}");
        }
        assert!(s.delete_everything().unwrap().all_ok());
    }

    #[test]
    fn waiting_marker_round_trips_and_expires() {
        let s = mem();
        s.upsert_task(&task_row("t1")).unwrap();
        let now = Utc::now();
        s.mark_waiting("t1", 1, now).unwrap();
        assert_eq!(s.task("t1").unwrap().unwrap().waiting_since, Some(now));
        assert!(s
            .stale_waiting(now + chrono::Duration::hours(25))
            .unwrap()
            .iter()
            .any(|t| t.id == "t1"));
        assert!(s
            .stale_waiting(now - chrono::Duration::minutes(1))
            .unwrap()
            .is_empty());
        s.clear_waiting("t1").unwrap();
        assert_eq!(s.task("t1").unwrap().unwrap().waiting_since, None);
    }
}
