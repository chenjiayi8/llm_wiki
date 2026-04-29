use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, types::Type, Connection};

use crate::import_queue::models::{
    build_headline, ImportBatchSummary, ImportJobRecord, ImportJobStatus, NewImportJob,
};

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

    pub fn insert_batch(&self, root_path: &str) -> Result<i64, String> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO import_batches (root_path) VALUES (?1)",
                params![root_path],
            )?;
            Ok(conn.last_insert_rowid())
        })
        .map_err(|err| err.to_string())
    }

    pub fn insert_job(&self, new_job: NewImportJob) -> Result<i64, String> {
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
        .map_err(|err| err.to_string())
    }

    pub fn recompute_batch(&self, batch_id: i64) -> Result<(), String> {
        self.with_conn(|conn| recompute_batch_inner(conn, batch_id))
            .map_err(|err| err.to_string())
    }

    pub fn claim_next_jobs(&self, limit: usize) -> Result<Vec<ImportJobRecord>, String> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT id
                FROM import_jobs
                WHERE status = 'queued'
                ORDER BY id ASC
                LIMIT ?1
                "#,
            )?;
            let ids: Vec<i64> = stmt
                .query_map(params![limit as i64], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            if ids.is_empty() {
                return Ok(Vec::new());
            }

            let mut records = Vec::with_capacity(ids.len());
            let mut touched_batches = Vec::new();

            for id in ids {
                let updated = conn.execute(
                    r#"
                    UPDATE import_jobs
                    SET status = ?2,
                        updated_at = CURRENT_TIMESTAMP
                    WHERE id = ?1 AND status = 'queued'
                    "#,
                    params![id, status_to_wire(ImportJobStatus::Copying)],
                )?;
                if updated == 0 {
                    continue;
                }

                let record = query_job_record(conn, id)?;
                touched_batches.push(record.batch_id);
                records.push(record);
            }

            touched_batches.sort_unstable();
            touched_batches.dedup();
            for batch_id in touched_batches {
                recompute_batch_inner(conn, batch_id)?;
            }

            Ok(records)
        })
        .map_err(|err| err.to_string())
    }

    pub fn mark_job_stage(&self, id: i64, status: ImportJobStatus) -> Result<(), String> {
        self.with_conn(|conn| {
            let updated = conn.execute(
                "UPDATE import_jobs SET status = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
                params![id, status_to_wire(status)],
            )?;
            if updated == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            let batch_id = job_batch_id(conn, id)?;
            recompute_batch_inner(conn, batch_id)
        })
        .map_err(|err| err.to_string())
    }

    pub fn mark_job_completed(&self, id: i64, files_written_json: &str) -> Result<(), String> {
        self.with_conn(|conn| {
            let updated = conn.execute(
                r#"
                UPDATE import_jobs
                SET status = 'completed',
                    files_written_json = ?2,
                    next_retry_at = NULL,
                    last_error = NULL,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1
                "#,
                params![id, files_written_json],
            )?;
            if updated == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            let batch_id = job_batch_id(conn, id)?;
            recompute_batch_inner(conn, batch_id)
        })
        .map_err(|err| err.to_string())
    }

    pub fn mark_job_retry(
        &self,
        id: i64,
        attempt_count: i64,
        next_retry_at: &str,
        last_error: &str,
    ) -> Result<(), String> {
        self.with_conn(|conn| {
            let updated = conn.execute(
                r#"
                UPDATE import_jobs
                SET status = 'retry_wait',
                    attempt_count = ?2,
                    next_retry_at = ?3,
                    last_error = ?4,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1
                "#,
                params![id, attempt_count, next_retry_at, last_error],
            )?;
            if updated == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            let batch_id = job_batch_id(conn, id)?;
            recompute_batch_inner(conn, batch_id)
        })
        .map_err(|err| err.to_string())
    }

    pub fn mark_job_failed(&self, id: i64, last_error: &str) -> Result<(), String> {
        self.with_conn(|conn| {
            let updated = conn.execute(
                r#"
                UPDATE import_jobs
                SET status = 'failed',
                    next_retry_at = NULL,
                    last_error = ?2,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1
                "#,
                params![id, last_error],
            )?;
            if updated == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            let batch_id = job_batch_id(conn, id)?;
            recompute_batch_inner(conn, batch_id)
        })
        .map_err(|err| err.to_string())
    }

    pub fn recover_stale_jobs(&self) -> Result<(), String> {
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

            let mut stmt = conn.prepare("SELECT id FROM import_batches ORDER BY id ASC")?;
            let batch_ids: Vec<i64> = stmt
                .query_map([], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for batch_id in batch_ids {
                recompute_batch_inner(conn, batch_id)?;
            }

            Ok(())
        })
        .map_err(|err| err.to_string())
    }

    pub fn global_summary(&self, max_concurrency: usize) -> Result<ImportBatchSummary, String> {
        self.with_conn(|conn| {
            let active_batches = conn.query_row(
                "SELECT COUNT(*) FROM import_batches WHERE status IN ('queued', 'running')",
                [],
                |row| row.get::<_, i64>(0),
            )?;

            let (total_jobs, queued_jobs, running_jobs, retrying_jobs, completed_jobs, failed_jobs): (
                i64,
                i64,
                i64,
                i64,
                i64,
                i64,
            ) = conn.query_row(
                r#"
                SELECT
                    COUNT(*) AS total_jobs,
                    COALESCE(SUM(CASE WHEN status = 'queued' THEN 1 ELSE 0 END), 0) AS queued_jobs,
                    COALESCE(SUM(CASE WHEN status IN ('copying', 'preprocessing', 'ingesting') THEN 1 ELSE 0 END), 0) AS running_jobs,
                    COALESCE(SUM(CASE WHEN status = 'retry_wait' THEN 1 ELSE 0 END), 0) AS retrying_jobs,
                    COALESCE(SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END), 0) AS completed_jobs,
                    COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed_jobs
                FROM import_jobs
                "#,
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )?;

            let mut summary = ImportBatchSummary {
                active_batches,
                total_jobs,
                queued_jobs,
                running_jobs,
                retrying_jobs,
                completed_jobs,
                failed_jobs,
                is_idle: queued_jobs + running_jobs + retrying_jobs == 0,
                headline: String::new(),
                max_concurrency,
            };
            summary.headline = build_headline(&summary);
            Ok(summary)
        })
        .map_err(|err| err.to_string())
    }

    pub fn get_job(&self, job_id: i64) -> Result<ImportJobRecord, String> {
        self.with_conn(|conn| query_job_record(conn, job_id))
            .map_err(|err| err.to_string())
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

fn query_job_record(conn: &Connection, job_id: i64) -> rusqlite::Result<ImportJobRecord> {
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
}

fn job_batch_id(conn: &Connection, job_id: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT batch_id FROM import_jobs WHERE id = ?1",
        params![job_id],
        |row| row.get::<_, i64>(0),
    )
}

fn recompute_batch_inner(conn: &Connection, batch_id: i64) -> rusqlite::Result<()> {
    let (total_jobs, queued_jobs, running_jobs, retrying_jobs, completed_jobs, failed_jobs): (
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
    ) = conn.query_row(
        r#"
        SELECT
            COUNT(*) AS total_jobs,
            COALESCE(SUM(CASE WHEN status = 'queued' THEN 1 ELSE 0 END), 0) AS queued_jobs,
            COALESCE(SUM(CASE WHEN status IN ('copying', 'preprocessing', 'ingesting') THEN 1 ELSE 0 END), 0) AS running_jobs,
            COALESCE(SUM(CASE WHEN status = 'retry_wait' THEN 1 ELSE 0 END), 0) AS retrying_jobs,
            COALESCE(SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END), 0) AS completed_jobs,
            COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed_jobs
        FROM import_jobs
        WHERE batch_id = ?1
        "#,
        params![batch_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        },
    )?;

    let status = if queued_jobs > 0 || running_jobs > 0 || retrying_jobs > 0 {
        "running"
    } else if failed_jobs > 0 {
        "completed_with_errors"
    } else {
        "completed"
    };

    conn.execute(
        r#"
        UPDATE import_batches
        SET status = ?2,
            total_jobs = ?3,
            queued_jobs = ?4,
            running_jobs = ?5,
            completed_jobs = ?6,
            failed_jobs = ?7,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
        "#,
        params![
            batch_id,
            status,
            total_jobs,
            queued_jobs,
            running_jobs,
            completed_jobs,
            failed_jobs
        ],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("llm-wiki-{name}-{nonce}.sqlite3"))
    }

    fn batch_status(db: &ImportQueueDb, batch_id: i64) -> String {
        db.with_conn(|conn| {
            conn.query_row(
                "SELECT status FROM import_batches WHERE id = ?1",
                params![batch_id],
                |row| row.get::<_, String>(0),
            )
        })
        .unwrap()
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

    #[test]
    fn create_batch_populates_summary_counts() {
        let db = ImportQueueDb::open(temp_db_path("summary")).unwrap();
        let batch_id = db.insert_batch("/tmp/folder").unwrap();
        db.insert_job(NewImportJob {
            batch_id,
            source_path: "/tmp/folder/a.md".into(),
            source_name: "a.md".into(),
            dest_path: "/tmp/project/raw/sources/a.md".into(),
            max_attempts: 3,
        })
        .unwrap();

        db.recompute_batch(batch_id).unwrap();
        assert_eq!(batch_status(&db, batch_id), "running");
        let summary = db.global_summary(5).unwrap();
        assert_eq!(summary.active_batches, 1);
        assert_eq!(summary.total_jobs, 1);
        assert_eq!(summary.queued_jobs, 1);
        assert_eq!(summary.max_concurrency, 5);
        assert!(!summary.headline.is_empty());
        assert!(!summary.is_idle);
    }

    #[test]
    fn recompute_batch_sets_completed_with_errors_when_terminal_failures_exist() {
        let db = ImportQueueDb::open(temp_db_path("completed-with-errors")).unwrap();
        let batch_id = db.insert_batch("/tmp/folder").unwrap();
        let job_a = db
            .insert_job(NewImportJob {
                batch_id,
                source_path: "/tmp/folder/a.md".into(),
                source_name: "a.md".into(),
                dest_path: "/tmp/project/raw/sources/a.md".into(),
                max_attempts: 3,
            })
            .unwrap();
        let job_b = db
            .insert_job(NewImportJob {
                batch_id,
                source_path: "/tmp/folder/b.md".into(),
                source_name: "b.md".into(),
                dest_path: "/tmp/project/raw/sources/b.md".into(),
                max_attempts: 3,
            })
            .unwrap();

        db.mark_job_failed(job_a, "copy failed").unwrap();
        db.mark_job_completed(job_b, "[]").unwrap();
        db.recompute_batch(batch_id).unwrap();

        assert_eq!(batch_status(&db, batch_id), "completed_with_errors");
    }

    #[test]
    fn claim_next_jobs_respects_limit() {
        let db = ImportQueueDb::open(temp_db_path("claim")).unwrap();
        let batch_id = db.insert_batch("/tmp/folder").unwrap();
        for name in ["a.md", "b.md", "c.md"] {
            db.insert_job(NewImportJob {
                batch_id,
                source_path: format!("/tmp/folder/{name}"),
                source_name: name.to_string(),
                dest_path: format!("/tmp/project/raw/sources/{name}"),
                max_attempts: 3,
            })
            .unwrap();
        }

        let claimed = db.claim_next_jobs(2).unwrap();
        assert_eq!(claimed.len(), 2);
        assert!(claimed
            .iter()
            .all(|job| job.status == ImportJobStatus::Copying));
    }
}
