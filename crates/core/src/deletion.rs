//! Account deletion: eight ordered steps, each reported (tech-prd §08.3).
//!
//! The report is the product: "deleted" without a per-step result is a claim,
//! not evidence. System-level backups are explicitly out of scope.

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::ports::Secrets;
use crate::store::Store;

#[derive(Debug, Clone)]
pub struct DeletionPaths {
    pub memory_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub log_dir: PathBuf,
    pub db_file: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    pub step: &'static str,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub steps: Vec<StepResult>,
    pub out_of_scope: Vec<&'static str>,
}

impl Report {
    pub fn all_ok(&self) -> bool {
        self.steps.iter().all(|s| s.ok)
    }
    pub fn failures(&self) -> Vec<String> {
        self.steps
            .iter()
            .filter(|s| !s.ok)
            .map(|s| format!("{}: {}", s.step, s.detail))
            .collect()
    }
}

fn record(report: &mut Report, step: &'static str, result: Result<()>) {
    match result {
        Ok(()) => report.steps.push(StepResult {
            step,
            ok: true,
            detail: String::new(),
        }),
        Err(e) => report.steps.push(StepResult {
            step,
            ok: false,
            detail: e.user_message(),
        }),
    }
}

fn remove_dir(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(crate::error::Error::Io(format!("{}: {e}", path.display()))),
    }
}

fn remove_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(crate::error::Error::Io(format!("{}: {e}", path.display()))),
    }
}

/// Execute the deletion plan in order. Stops recording only on panic; every step
/// is attempted so the report shows exactly what remains.
pub fn execute(paths: &DeletionPaths, store: &Store, secrets: &dyn Secrets) -> Report {
    let mut report = Report {
        out_of_scope: vec![
            "系统级备份（Time Machine / Windows 备份）",
            "磁盘上已释放区块的残留",
        ],
        ..Default::default()
    };

    let rows = store.delete_everything().and_then(|r| match r.all_ok() {
        true => Ok(()),
        false => Err(crate::error::Error::Store(r.failures().join("; "))),
    });
    let closed = store.close();
    record(&mut report, "1/8 sqlite rows and handle", rows.and(closed));
    record(
        &mut report,
        "2/8 memory files",
        remove_dir(&paths.memory_dir),
    );
    record(
        &mut report,
        "3/8 workspace and task artifacts",
        remove_dir(&paths.workspace_dir),
    );
    record(&mut report, "4/8 logs", remove_dir(&paths.log_dir));
    record(
        &mut report,
        "5/8 database file",
        remove_file(&paths.db_file),
    );
    record(
        &mut report,
        "6/8 sqlite side files",
        remove_file(&paths.db_file.with_extension("db-wal"))
            .and_then(|_| remove_file(&paths.db_file.with_extension("db-shm"))),
    );
    record(
        &mut report,
        "7/8 migration backup",
        remove_file(&paths.db_file.with_extension("db.bak")),
    );
    let mut secret_result = Ok(());
    for key in ["provider/default", "provider/openai", "provider/anthropic"] {
        if let Err(e) = secrets.delete(key) {
            secret_result = Err(e);
        }
    }
    record(&mut report, "8/8 keychain entries", secret_result);
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::SecretString;
    use std::sync::Mutex;

    struct FakeSecrets {
        keys: Mutex<Vec<String>>,
    }

    impl Secrets for FakeSecrets {
        fn get(&self, _k: &str) -> Result<Option<SecretString>> {
            Ok(None)
        }
        fn set(&self, k: &str, _v: &SecretString) -> Result<()> {
            self.keys.lock().unwrap().push(k.to_string());
            Ok(())
        }
        fn delete(&self, k: &str) -> Result<()> {
            self.keys.lock().unwrap().retain(|x| x != k);
            Ok(())
        }
    }

    #[test]
    fn removes_every_local_artifact_and_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let memory = dir.path().join("memory");
        let workspace = dir.path().join("workspace");
        let logs = dir.path().join("logs");
        for d in [&memory, &workspace, &logs] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("x.txt"), "data").unwrap();
        }
        let db = dir.path().join("app.db");
        let store = Store::open(&db).unwrap();
        std::fs::write(dir.path().join("app.db-wal"), "wal").unwrap();
        std::fs::write(dir.path().join("app.db.bak"), "bak").unwrap();

        let secrets = FakeSecrets {
            keys: Mutex::new(vec!["provider/default".into()]),
        };
        let report = execute(
            &DeletionPaths {
                memory_dir: memory.clone(),
                workspace_dir: workspace.clone(),
                log_dir: logs.clone(),
                db_file: db.clone(),
            },
            &store,
            &secrets,
        );

        assert!(report.all_ok(), "failures: {:?}", report.failures());
        assert!(!memory.exists());
        assert!(!workspace.exists());
        assert!(!logs.exists());
        assert!(!db.exists());
        assert!(!dir.path().join("app.db-wal").exists());
        assert!(!dir.path().join("app.db.bak").exists());
        assert!(secrets.keys.lock().unwrap().is_empty());
        assert_eq!(report.steps.len(), 8);
        assert!(report.out_of_scope.iter().any(|s| s.contains("备份")));
    }

    #[test]
    fn missing_paths_are_not_failures() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("app.db")).unwrap();
        let secrets = FakeSecrets {
            keys: Mutex::new(vec![]),
        };
        let report = execute(
            &DeletionPaths {
                memory_dir: dir.path().join("nope"),
                workspace_dir: dir.path().join("nope2"),
                log_dir: dir.path().join("nope3"),
                db_file: dir.path().join("app.db"),
            },
            &store,
            &secrets,
        );
        assert!(report.all_ok(), "failures: {:?}", report.failures());
    }

    #[test]
    fn deletion_releases_the_database_handle_before_removing_files() {
        struct NoSecrets;
        impl Secrets for NoSecrets {
            fn get(&self, _k: &str) -> Result<Option<SecretString>> {
                Ok(None)
            }
            fn set(&self, _k: &str, _v: &SecretString) -> Result<()> {
                Ok(())
            }
            fn delete(&self, _k: &str) -> Result<()> {
                Ok(())
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("app.db");
        let store = Store::open(&db).unwrap();
        let report = execute(
            &DeletionPaths {
                memory_dir: dir.path().join("memory"),
                workspace_dir: dir.path().join("workspace"),
                log_dir: dir.path().join("logs"),
                db_file: db.clone(),
            },
            &store,
            &NoSecrets,
        );

        assert!(report.all_ok(), "failures: {:?}", report.failures());
        assert!(
            store.version().is_err(),
            "the connection must be closed before the file is unlinked; Windows refuses otherwise"
        );
        assert!(store.close().is_ok(), "closing twice stays a no-op");
        assert!(!db.exists());
    }

    #[test]
    fn a_table_that_refuses_to_empty_fails_step_one() {
        struct NoSecrets;
        impl Secrets for NoSecrets {
            fn get(&self, _k: &str) -> Result<Option<SecretString>> {
                Ok(None)
            }
            fn set(&self, _k: &str, _v: &SecretString) -> Result<()> {
                Ok(())
            }
            fn delete(&self, _k: &str) -> Result<()> {
                Ok(())
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("app.db");
        let store = Store::open(&db).unwrap();
        store.drop_table_for_test("messages").unwrap();
        let report = execute(
            &DeletionPaths {
                memory_dir: dir.path().join("memory"),
                workspace_dir: dir.path().join("workspace"),
                log_dir: dir.path().join("logs"),
                db_file: db.clone(),
            },
            &store,
            &NoSecrets,
        );

        assert!(
            !report.all_ok(),
            "a failed table must show up in the report"
        );
        let first = &report.failures()[0];
        assert!(first.starts_with("1/8"), "{first}");
        assert!(first.contains("messages"), "{first}");
        assert!(
            !db.exists(),
            "the file still goes, even when the row wipe reports a failure"
        );
    }
}
