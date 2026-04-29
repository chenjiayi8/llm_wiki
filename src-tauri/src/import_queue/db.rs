use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, types::Type, Connection};

use crate::import_queue::models::{ImportJobRecord, ImportJobStatus, NewImportJob};

#[derive(Clone)]
pub struct ImportQueueDb {
    conn: Arc<Mutex<Connection>>,
}

impl ImportQueueDb {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", true)?;

        let db = Self {
            conn: Arc::new(Mutex::new(connection)),
        };
        db.bootstrap()?;
        Ok(db)
    }

    pub fn bootstrap(&self) -> rusqlite::Result<()> {
        self.with_conn(|conn| {
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS import_batches (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    root_path TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'queued',
                    total_jobs INTEGER NOT NULL DEFAULT 0,
                    queued_jobs INTEGER NOT NULL DEFAULT 0,
                    running_jobs INTEGER NOT NULL DEFAULT 0,
                    completed_jobs INTEGER NOT NULL DEFAULT 0,
                    failed_jobs INTEGER NOT NULL DEFAULT 0,
                    last_error TEXT,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS import_jobs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    batch_id INTEGER NOT NULL REFERENCES import_batches(id) ON DELETE CASCADE,
                    source_path TEXT NOT NULL,
                    source_name TEXT NOT NULL,
                    dest_path TEXT NOT NULL,
                    status TEXT NOT NULL,
                    attempt_count INTEGER NOT NULL DEFAULT 0,
                    max_attempts INTEGER NOT NULL,
                    next_retry_at TEXT,
                    last_error TEXT,
                    files_written_json TEXT NOT NULL DEFAULT '[]',
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE INDEX IF NOT EXISTS idx_import_batches_status ON import_batches(status);
                CREATE INDEX IF NOT EXISTS idx_import_jobs_status ON import_jobs(status);
                CREATE INDEX IF NOT EXISTS idx_import_jobs_batch_id ON import_jobs(batch_id);
                "#,
            )
        })
    }

    pub fn table_names(&self) -> rusqlite::Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect()
        })
    }

    pub fn insert_batch(&self, root_path: &str) -> rusqlite::Result<i64> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO import_batches (root_path) VALUES (?1)",
                params![root_path],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    pub fn insert_job(&self, new_job: NewImportJob) -> rusqlite::Result<i64> {
        self.with_conn(|conn| {
            conn.execute(
                r#"
                INSERT INTO import_jobs (
                    batch_id,
                    source_path,
                    source_name,
                    dest_path,
                    status,
                    attempt_count,
                    max_attempts
                ) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)
                "#,
                params![
                    new_job.batch_id,
                    new_job.source_path,
                    new_job.source_name,
                    new_job.dest_path,
                    status_to_wire(ImportJobStatus::Queued),
                    new_job.max_attempts,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    pub fn mark_job_stage(&self, job_id: i64, status: ImportJobStatus) -> rusqlite::Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE import_jobs SET status = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
                params![job_id, status_to_wire(status)],
            )?;
            Ok(())
        })
    }

    pub fn recover_stale_jobs(&self) -> rusqlite::Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                r#"
                UPDATE import_jobs
                SET status = 'queued',
                    updated_at = CURRENT_TIMESTAMP
                WHERE status IN ('copying', 'preprocessing', 'ingesting')
                "#,
                [],
            )?;
            Ok(())
        })
    }

    pub fn get_job(&self, job_id: i64) -> rusqlite::Result<ImportJobRecord> {
        self.with_conn(|conn| {
            conn.query_row(
                r#"
                SELECT
                    id,
                    batch_id,
                    source_path,
                    source_name,
                    dest_path,
                    status,
                    attempt_count
                FROM import_jobs
                WHERE id = ?1
                "#,
                params![job_id],
                |row| {
                    let status_raw: String = row.get(5)?;
                    let status = ImportJobStatus::from_wire(&status_raw).map_err(|err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            Type::Text,
                            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
                        )
                    })?;

                    Ok(ImportJobRecord {
                        id: row.get(0)?,
                        batch_id: row.get(1)?,
                        source_path: row.get(2)?,
                        source_name: row.get(3)?,
                        dest_path: row.get(4)?,
                        status,
                        attempt_count: row.get(6)?,
                    })
                },
            )
        })
    }

    fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let guard = self
            .conn
            .lock()
            .expect("import queue sqlite mutex poisoned");
        f(&guard)
    }
}

fn status_to_wire(status: ImportJobStatus) -> &'static str {
    match status {
        ImportJobStatus::Queued => "queued",
        ImportJobStatus::Copying => "copying",
        ImportJobStatus::Preprocessing => "preprocessing",
        ImportJobStatus::Ingesting => "ingesting",
        ImportJobStatus::RetryWait => "retry_wait",
        ImportJobStatus::Completed => "completed",
        ImportJobStatus::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("llm-wiki-{name}-{nonce}.sqlite3"))
    }

    #[test]
    fn bootstrap_creates_expected_tables() {
        let db = ImportQueueDb::open(temp_db_path("bootstrap")).unwrap();
        let tables = db.table_names().unwrap();
        assert!(tables.contains(&"import_batches".to_string()));
        assert!(tables.contains(&"import_jobs".to_string()));
    }

    #[test]
    fn reopen_requeues_stale_running_jobs() {
        let db_path = temp_db_path("requeue");

        let job_id = {
            let db = ImportQueueDb::open(&db_path).unwrap();
            let batch_id = db.insert_batch("/tmp/source").unwrap();
            let job_id = db
                .insert_job(NewImportJob {
                    batch_id,
                    source_path: "/tmp/source/a.md".into(),
                    source_name: "a.md".into(),
                    dest_path: "/tmp/project/raw/sources/a.md".into(),
                    max_attempts: 3,
                })
                .unwrap();
            db.mark_job_stage(job_id, ImportJobStatus::Ingesting)
                .unwrap();
            job_id
        };

        let reopened = ImportQueueDb::open(&db_path).unwrap();
        reopened.recover_stale_jobs().unwrap();

        let job = reopened.get_job(job_id).unwrap();
        assert_eq!(job.status, ImportJobStatus::Queued);
    }
}
