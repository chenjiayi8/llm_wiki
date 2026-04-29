use chrono::{Duration, Utc};
use tauri::State;

use crate::import_queue::{
    config::load_import_max_concurrency,
    db::ImportQueueDb,
    models::{ImportBatchSummary, ImportJobRecord, ImportJobStatus},
};

#[tauri::command]
pub fn enqueue_import_batch(
    db: State<ImportQueueDb>,
    root_path: String,
    source_paths: Vec<String>,
    project_path: String,
) -> Result<i64, String> {
    db.enqueue_import_batch_atomic(&root_path, source_paths, &project_path, 3)
}

#[tauri::command]
pub fn get_import_queue_summary(db: State<ImportQueueDb>) -> Result<ImportBatchSummary, String> {
    db.global_summary(load_import_max_concurrency())
}

#[tauri::command]
pub fn claim_import_jobs(
    db: State<ImportQueueDb>,
    limit: usize,
) -> Result<Vec<ImportJobRecord>, String> {
    db.claim_next_jobs(limit)
}

#[tauri::command]
pub fn update_import_job_stage(
    db: State<ImportQueueDb>,
    job_id: i64,
    status: String,
) -> Result<(), String> {
    let parsed = ImportJobStatus::from_wire(&status)?;
    db.mark_job_stage(job_id, parsed)
}

#[tauri::command]
pub fn complete_import_job(
    db: State<ImportQueueDb>,
    job_id: i64,
    files_written_json: String,
) -> Result<(), String> {
    db.mark_job_completed(job_id, &files_written_json)
}

#[tauri::command]
pub fn fail_import_job(
    db: State<ImportQueueDb>,
    job_id: i64,
    attempt_count: i64,
    last_error: String,
) -> Result<(), String> {
    if attempt_count < 3 {
        let next_retry_at = compute_next_retry_timestamp(attempt_count);
        return db.mark_job_retry(job_id, attempt_count, &next_retry_at, &last_error);
    }

    db.mark_job_failed(job_id, &last_error)
}

fn compute_next_retry_timestamp(attempt_count: i64) -> String {
    let bounded_attempt = attempt_count.max(1).min(3);
    let delay_seconds = 15 * (1_i64 << (bounded_attempt - 1));
    (Utc::now() + Duration::seconds(delay_seconds))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}
