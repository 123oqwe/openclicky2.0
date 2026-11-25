//! G3 第 8 条: the migration and recovery drill. A backup nobody restored is a
//! file, not a recovery path.

use std::path::Path;

use chrono::Utc;
use openclicky_core::store::{migrations, Store};

fn v1_database(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(migrations::M001).unwrap();
    conn.pragma_update(None, "user_version", 1i64).unwrap();
    conn.execute_batch(
        "INSERT INTO sessions (id, created_at, kind)
             VALUES ('s1','2026-01-01T00:00:00+00:00','ask');
         INSERT INTO messages (id, session_id, role, content, created_at)
             VALUES ('m1','s1','user','the quarterly spreadsheet export','2026-01-01T00:00:00+00:00');
         INSERT INTO tasks (id,name,goal,state,checkpoint_json,created_at,updated_at)
             VALUES ('t1','n','g','completed','{}','2026-01-01T00:00:00+00:00','2026-01-01T00:00:00+00:00');",
    )
    .unwrap();
}

fn user_version(path: &Path) -> i32 {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn the_drill_migrates_v1_and_the_backup_restores_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    v1_database(&path);

    let store = Store::open(&path).unwrap();
    assert_eq!(store.version().unwrap(), migrations::LATEST);
    let kept = store.recent_messages("s1", 10).unwrap();
    assert_eq!(kept.len(), 1, "a migration must keep existing rows");
    assert_eq!(store.list_tasks(None).unwrap().len(), 1);
    assert_eq!(
        store.search_messages("quarterly", 10).unwrap().len(),
        1,
        "the v3 backfill must index rows that were written before v3"
    );
    store.close().unwrap();

    let backup = dir.path().join("app.db.bak");
    assert!(backup.exists(), "no backup, no rollback");
    assert_eq!(
        user_version(&backup),
        1,
        "the backup has to be the pre-migration file"
    );

    let restored = dir.path().join("restored.db");
    std::fs::copy(&backup, &restored).unwrap();
    let again = Store::open(&restored).unwrap();
    assert_eq!(again.version().unwrap(), migrations::LATEST);
    assert_eq!(again.recent_messages("s1", 10).unwrap().len(), 1);
    assert_eq!(again.list_tasks(None).unwrap().len(), 1);
}

#[test]
fn the_backup_stays_the_pre_migration_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    v1_database(&path);
    Store::open(&path).unwrap().close().unwrap();
    Store::open(&path).unwrap().close().unwrap();

    assert_eq!(
        user_version(&dir.path().join("app.db.bak")),
        1,
        "a second open must not overwrite the only pre-migration copy"
    );
}

#[test]
fn a_newer_database_opens_read_only_instead_of_bricking() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    Store::open(&path).unwrap().close().unwrap();
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", (migrations::LATEST + 1) as i64)
            .unwrap();
    }

    let store = Store::open(&path).expect("a downgrade must still be able to read");
    assert!(store.is_read_only());
    assert_eq!(store.version().unwrap(), migrations::LATEST + 1);
    assert!(store.list_tasks(None).unwrap().is_empty());

    let err = store.insert_session("ask", Utc::now()).unwrap_err();
    assert!(
        err.user_message().contains("更新版本"),
        "the refusal has to name the cause, got: {}",
        err.user_message()
    );
}
