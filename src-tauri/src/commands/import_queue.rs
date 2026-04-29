use tauri::State;

use crate::import_queue::{
    config::load_import_max_concurrency, db::ImportQueueDb, models::ImportBatchSummary,
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
