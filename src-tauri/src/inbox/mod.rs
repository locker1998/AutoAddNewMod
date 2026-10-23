use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use log::{info, warn};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

pub mod metadata;

pub use metadata::InboxMetadataStore;

use crate::config::AppConfig;
use crate::pipeline::extract::{guess_kind, ArchiveKind};
use crate::pipeline::types::{InboxError, InboxItem, InboxItemKind};
use crate::util::ensure_dir_exists;

pub fn list_inbox_items(
    incoming_dir: &Path,
    metadata: &InboxMetadataStore,
    config: &AppConfig,
) -> Result<Vec<InboxItem>, InboxError> {
    if !incoming_dir.exists() {
        ensure_dir_exists(incoming_dir).map_err(|e| InboxError::Io { message: e })?;
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    let entries = fs::read_dir(incoming_dir).map_err(|e| InboxError::Io {
        message: e.to_string(),
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| InboxError::Io {
            message: e.to_string(),
        })?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();

        if file_name.starts_with('.') {
            continue;
        }

        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = if is_dir {
            0
        } else {
            meta.as_ref().map(|m| m.len()).unwrap_or(0)
        };

        let kind = if is_dir {
            InboxItemKind::Folder
        } else {
            match guess_kind(&path) {
                ArchiveKind::Zip | ArchiveKind::Rar | ArchiveKind::SevenZ => InboxItemKind::Archive,
                ArchiveKind::Modpkg | ArchiveKind::Encrypt => InboxItemKind::Encrypted,
                ArchiveKind::Unknown => InboxItemKind::Unknown,
            }
        };

        let item_meta = metadata.get(&file_name);
        let author = item_meta.author;
        let game_id = item_meta.game_id;

        let assigned = game_id.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
            && author.as_ref().map(|s| !s.is_empty()).unwrap_or(false);
        let setup_ok = match (&game_id, &author) {
            (Some(g), Some(a)) if assigned => config.auth_ready(g, a),
            _ => false,
        };

        items.push(InboxItem {
            name: file_name,
            path: path.to_string_lossy().to_string(),
            kind,
            size,
            game_id,
            author,
            requires_decryption: false,
            has_decrypt_key: setup_ok,
            has_decryptor: false,
            ready: setup_ok,
        });
    }

    items.sort_by_key(|a| a.name.to_lowercase());
    Ok(items)
}

fn unique_inbox_name(incoming: &Path, preferred: &str) -> String {
    let mut name = preferred.to_string();
    let mut i = 2;
    while incoming.join(&name).exists() {
        let p = Path::new(preferred);
        if let (Some(stem), Some(ext)) = (p.file_stem(), p.extension()) {
            name = format!(
                "{}_{}.{}",
                stem.to_string_lossy(),
                i,
                ext.to_string_lossy()
            );
        } else {
            name = format!("{preferred}_{i}");
        }
        i += 1;
    }
    name
}

pub fn import_paths_to_inbox(
    paths: &[String],
    incoming_dir: &Path,
) -> Result<Vec<String>, InboxError> {
    ensure_dir_exists(incoming_dir).map_err(|e| InboxError::Io { message: e })?;

    let mut imported = Vec::new();

    for path_str in paths {
        let src = PathBuf::from(path_str);
        if !src.exists() {
            return Err(InboxError::PathNotFound {
                path: path_str.clone(),
            });
        }

        let preferred = src
            .file_name()
            .ok_or(InboxError::InvalidName)?
            .to_string_lossy()
            .to_string();

        let name = unique_inbox_name(incoming_dir, &preferred);
        let dest = incoming_dir.join(&name);

        if fs::rename(&src, &dest).is_err() {
            if src.is_dir() {
                copy_dir_recursive(&src, &dest)?;
                fs::remove_dir_all(&src).map_err(|e| InboxError::Io {
                    message: e.to_string(),
                })?;
            } else {
                fs::copy(&src, &dest).map_err(|e| InboxError::Io {
                    message: e.to_string(),
                })?;
                fs::remove_file(&src).map_err(|e| InboxError::Io {
                    message: e.to_string(),
                })?;
            }
        }

        imported.push(name);
    }

    Ok(imported)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), InboxError> {
    ensure_dir_exists(dest).map_err(|e| InboxError::Io { message: e })?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.map_err(|e| InboxError::Io {
            message: e.to_string(),
        })?;
        let relative = entry.path().strip_prefix(src).map_err(|e| InboxError::Io {
            message: e.to_string(),
        })?;
        let out = dest.join(relative);
        if entry.file_type().is_dir() {
            ensure_dir_exists(&out).map_err(|e| InboxError::Io { message: e })?;
        } else {
            if let Some(parent) = out.parent() {
                ensure_dir_exists(parent).map_err(|e| InboxError::Io { message: e })?;
            }
            fs::copy(entry.path(), &out).map_err(|e| InboxError::Io {
                message: e.to_string(),
            })?;
        }
    }
    Ok(())
}

pub fn delete_inbox_items(
    names: &[String],
    incoming_dir: &Path,
    metadata: &mut InboxMetadataStore,
) -> Result<(), InboxError> {
    for name in names {
        let path = incoming_dir.join(name);
        if !path.exists() {
            metadata.remove(name);
            continue;
        }
        if path.is_dir() {
            fs::remove_dir_all(&path).map_err(|e| InboxError::Io {
                message: e.to_string(),
            })?;
        } else {
            fs::remove_file(&path).map_err(|e| InboxError::Io {
                message: e.to_string(),
            })?;
        }
        metadata.remove(name);
    }
    Ok(())
}

pub struct InboxWatcher {
    _debouncer: notify_debouncer_mini::Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>,
}

impl InboxWatcher {
    pub fn start(app: AppHandle, incoming: PathBuf) -> Result<Self, String> {
        ensure_dir_exists(&incoming)?;
        let app2 = app.clone();
        let mut debouncer = new_debouncer(Duration::from_millis(400), move |res: DebounceEventResult| {
            match res {
                Ok(_events) => {
                    let _ = app2.emit("inbox-changed", ());
                }
                Err(e) => warn!("Inbox watch error: {:?}", e),
            }
        })
        .map_err(|e| e.to_string())?;

        debouncer
            .watcher()
            .watch(&incoming, RecursiveMode::NonRecursive)
            .map_err(|e| e.to_string())?;

        info!("Watching inbox at {:?}", incoming);
        Ok(Self {
            _debouncer: debouncer,
        })
    }
}

pub type SharedWatcher = Arc<Mutex<Option<InboxWatcher>>>;
