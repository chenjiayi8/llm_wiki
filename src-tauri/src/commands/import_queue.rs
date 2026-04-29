use std::path::Path;

use tauri::State;

use crate::import_queue::{
    config::load_import_max_concurrency,
    db::ImportQueueDb,
    models::{ImportBatchSummary, NewImportJob},
};

#[tauri::command]
pub fn enqueue_import_batch(
    db: State<ImportQueueDb>,
    root_path: String,
    source_paths: Vec<String>,
    project_path: String,
) -> Result<i64, String> {
    let batch_id = db.insert_batch(&root_path)?;

    for source_path in source_paths {
        let source_name = Path::new(&source_path)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("Invalid source path: {source_path}"))?
            .to_string();
        let dest_path = format!("{project_path}/raw/sources/{source_name}");

        db.insert_job(NewImportJob {
            batch_id,
            source_path,
            source_name,
            dest_path,
            max_attempts: 3,
        })?;
    }

    db.recompute_batch(batch_id)?;
    Ok(batch_id)
}

#[tauri::command]
pub fn get_import_queue_summary(db: State<ImportQueueDb>) -> Result<ImportBatchSummary, String> {
    db.global_summary(load_import_max_concurrency())
}
