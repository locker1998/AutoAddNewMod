mod commands;
mod config;
mod inbox;
mod pipeline;
mod state;
mod util;

use std::sync::Mutex as StdMutex;

use log::info;
use tauri::{Emitter, Manager};

use commands::{collect_drop_paths, import_paths_internal, restart_watcher};
use state::AppState;
use util::ensure_dir_exists;

static PENDING_IMPORTS: StdMutex<Vec<String>> = StdMutex::new(Vec::new());

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let paths = collect_drop_paths(&args);
            if !paths.is_empty() {
                if let Some(state) = app.try_state::<AppState>() {
                    match import_paths_internal(state.inner(), paths) {
                        Ok(_) => {
                            let _ = app.emit("inbox-changed", ());
                        }
                        Err(e) => log::warn!("Drop-on-exe import failed: {e}"),
                    }
                } else if let Ok(mut pending) = PENDING_IMPORTS.lock() {
                    pending.extend(paths);
                }
            }
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_focus();
                let _ = win.unminimize();
            }
        }))
        .manage(AppState::new())
        .setup(|app| {
            let state = app.state::<AppState>();
            {
                let cfg = state.config.lock();
                let _ = ensure_dir_exists(&crate::util::app_data_dir());
                let _ = ensure_dir_exists(&cfg.incoming_dir());
            }
            restart_watcher(state.inner(), app.handle())?;

            let args: Vec<String> = std::env::args().collect();
            let mut paths = collect_drop_paths(&args);
            if let Ok(mut pending) = PENDING_IMPORTS.lock() {
                paths.append(&mut *pending);
            }
            if !paths.is_empty() {
                info!("Importing {} path(s) from argv/drop", paths.len());
                let _ = import_paths_internal(state.inner(), paths);
                let _ = app.emit("inbox-changed", ());
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::mod_logic_template,
            commands::validate_mod_logic,
            commands::parse_author_logic,
            commands::preset_authors_for_game,
            commands::list_inbox,
            commands::import_to_inbox,
            commands::set_inbox_meta,
            commands::delete_inbox,
            commands::process_inbox,
            commands::open_incoming_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
