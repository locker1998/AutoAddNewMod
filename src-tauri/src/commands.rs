use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::config::{ensure_incoming, AppConfig, PartialAppConfig};
use crate::inbox::{
    delete_inbox_items, import_paths_to_inbox, list_inbox_items, InboxWatcher,
};
use crate::pipeline::mod_logic::{default_mod_logic_template, parse_mod_logic};
use crate::pipeline::process::process_items;
use crate::pipeline::types::{InboxItem, ProcessInboxResult};
use crate::state::AppState;

#[tauri::command]
pub fn get_config(state: State<AppState>) -> AppConfig {
    state.config.lock().clone()
}

#[tauri::command]
pub fn save_config(
    state: State<AppState>,
    app: AppHandle,
    partial: PartialAppConfig,
) -> Result<AppConfig, String> {
    let (snapshot, should_restart) = {
        let mut cfg = state.config.lock();
        let old_incoming = cfg.incoming_dir();
        cfg.merge_from(partial);
        cfg.save()?;
        let new_incoming = cfg.incoming_dir();
        ensure_incoming(&cfg)?;
        let should_restart = old_incoming != new_incoming;
        (cfg.clone(), should_restart)
    };

    if should_restart {
        restart_watcher(&state, &app)?;
    }

    Ok(snapshot)
}

#[tauri::command]
pub fn mod_logic_template() -> String {
    default_mod_logic_template()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateModLogicResult {
    pub ok: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub fn validate_mod_logic(source: String) -> ValidateModLogicResult {
    match parse_mod_logic(&source) {
        Ok(_) => ValidateModLogicResult {
            ok: true,
            error: None,
        },
        Err(error) => ValidateModLogicResult {
            ok: false,
            error: Some(error),
        },
    }
}

#[tauri::command]
pub fn parse_author_logic(source: String) -> Result<crate::pipeline::mod_logic::ModLogicView, String> {
    parse_mod_logic(&source).map(|l| l.to_view())
}

#[tauri::command]
pub fn open_incoming_folder(state: State<AppState>) -> Result<String, String> {
    let cfg = state.config.lock();
    let incoming = ensure_incoming(&cfg)?;
    let path = incoming.to_string_lossy().to_string();
    drop(cfg);

    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {e}"))?;
    }
    #[cfg(not(windows))]
    {
        let _ = &path;
        return Err("Open folder is only implemented on Windows in this build".into());
    }

    Ok(path)
}

#[tauri::command]
pub fn list_inbox(state: State<AppState>) -> Result<Vec<InboxItem>, String> {
    let cfg = state.config.lock();
    let incoming = ensure_incoming(&cfg)?;
    let meta = state.inbox_meta.lock();
    list_inbox_items(&incoming, &meta, &cfg).map_err(|e| format!("{e:?}"))
}

pub fn import_paths_internal(state: &AppState, paths: Vec<String>) -> Result<Vec<String>, String> {
    let cfg = state.config.lock();
    let incoming = ensure_incoming(&cfg)?;
    let imported = import_paths_to_inbox(&paths, &incoming).map_err(|e| format!("{e:?}"))?;
    // Do not assign a default game/author — user must set those manually in Inbox.
    Ok(imported)
}

#[tauri::command]
pub fn import_to_inbox(state: State<AppState>, paths: Vec<String>) -> Result<Vec<String>, String> {
    import_paths_internal(state.inner(), paths)
}

#[tauri::command]
pub fn set_inbox_meta(
    state: State<AppState>,
    names: Vec<String>,
    game_id: Option<String>,
    author: Option<String>,
) -> Result<(), String> {
    let mut meta = state.inbox_meta.lock();
    meta.set_meta(&names, game_id, author);
    Ok(())
}

#[tauri::command]
pub fn delete_inbox(state: State<AppState>, names: Vec<String>) -> Result<(), String> {
    let cfg = state.config.lock();
    let incoming = ensure_incoming(&cfg)?;
    drop(cfg);
    let mut meta = state.inbox_meta.lock();
    delete_inbox_items(&names, &incoming, &mut meta).map_err(|e| format!("{e:?}"))
}

#[tauri::command]
pub fn process_inbox(
    state: State<AppState>,
    names: Vec<String>,
    passwords: HashMap<String, String>,
) -> Result<ProcessInboxResult, String> {
    let cfg = state.config.lock().clone();
    let incoming = ensure_incoming(&cfg)?;

    let meta_map: HashMap<String, (Option<String>, Option<String>)> = {
        let meta = state.inbox_meta.lock();
        names
            .iter()
            .map(|n| {
                let m = meta.get(n);
                (n.clone(), (m.game_id, m.author))
            })
            .collect()
    };

    let mut succeeded = Vec::new();
    let result = process_items(
        &names,
        &passwords,
        &incoming,
        &cfg,
        |name| {
            meta_map
                .get(name)
                .cloned()
                .unwrap_or((None, None))
        },
        |name| {
            succeeded.push(name.to_string());
        },
    )
    .map_err(|e| format!("{e:?}"))?;

    {
        let mut meta = state.inbox_meta.lock();
        for name in &succeeded {
            meta.remove(name);
        }
    }

    Ok(result)
}

pub fn restart_watcher(state: &AppState, app: &AppHandle) -> Result<(), String> {
    let incoming = state.incoming_dir();
    let watcher = InboxWatcher::start(app.clone(), incoming)?;
    *state.watcher.lock() = Some(watcher);
    Ok(())
}

pub fn collect_drop_paths(args: &[String]) -> Vec<String> {
    args.iter()
        .skip(1) // skip exe
        .filter(|a| {
            let p = PathBuf::from(a);
            p.exists() && !a.starts_with('-')
        })
        .cloned()
        .collect()
}
