mod clip_server;
mod commands;
mod import_queue;
mod types;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    clip_server::start_clip_server();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
