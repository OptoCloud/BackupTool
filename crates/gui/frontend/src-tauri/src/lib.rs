mod commands;

use tauri_plugin_dialog::DialogExt;

#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder.map(|f| f.to_string()));
    });
    rx.recv().ok().flatten()
}

#[tauri::command]
async fn pick_archive_file(app: tauri::AppHandle, save: bool) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    if save {
        app.dialog()
            .file()
            .add_filter("BPFS archive", &["bpfs"])
            .save_file(move |file| {
                let _ = tx.send(file.map(|f| f.to_string()));
            });
    } else {
        app.dialog()
            .file()
            .add_filter("BPFS archive", &["bpfs"])
            .pick_file(move |file| {
                let _ = tx.send(file.map(|f| f.to_string()));
            });
    }
    rx.recv().ok().flatten()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(commands::TaskState::default())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            pick_folder,
            pick_archive_file,
            commands::pack_archive,
            commands::list_archive,
            commands::extract_archive,
            commands::verify_archive,
            commands::cancel_task,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
