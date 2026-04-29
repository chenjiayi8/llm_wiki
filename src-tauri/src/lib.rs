mod clip_server;
mod commands;
mod import_queue;
mod types;

use std::fs;
use std::path::PathBuf;

fn import_queue_db_path() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".llm-wiki")
            .join("import-queue.sqlite3");
    }

    std::env::temp_dir().join("llm-wiki-import-queue.sqlite3")
}

fn init_import_queue_db() -> Result<import_queue::db::ImportQueueDb, String> {
    let db_path = import_queue_db_path();
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let db = import_queue::db::ImportQueueDb::open(&db_path).map_err(|err| err.to_string())?;
    db.recover_stale_jobs()?;
    Ok(db)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    clip_server::start_clip_server();
    let import_queue_db =
        init_import_queue_db().expect("failed to initialize persistent import queue database");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(import_queue_db)
        .invoke_handler(tauri::generate_handler![
            commands::fs::read_file,
            commands::fs::write_file,
            commands::fs::list_directory,
            commands::fs::copy_file,
            commands::fs::preprocess_file,
            commands::fs::delete_file,
            commands::fs::find_related_wiki_pages,
            commands::fs::create_directory,
            commands::project::create_project,
            commands::project::open_project,
            commands::import_queue::enqueue_import_batch,
            commands::import_queue::get_import_queue_summary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
