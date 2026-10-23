use std::{fs, path::Path};

use crate::config::AppConfig;
use crate::pipeline::types::InboxError;
use crate::util::ensure_dir_exists;

/// Place installed mod folders from a temp staging dir into the game mod folder.
pub fn place_mods(
    staging: &Path,
    mod_names: &[String],
    game_id: &str,
    author_label: &str,
    config: &AppConfig,
    organize_by_author: bool,
) -> Result<Vec<String>, InboxError> {
    let game = config
        .find_game(game_id)
        .ok_or_else(|| InboxError::GameNotFound {
            game_id: game_id.to_string(),
        })?;

    if game.mod_folder.trim().is_empty() {
        return Err(InboxError::ModFolderMissing {
            game: game.name.clone(),
        });
    }

    let by_author = organize_by_author && game.organize_by_author && !author_label.trim().is_empty();

    let mut dest_root = std::path::PathBuf::from(&game.mod_folder);
    if by_author {
        let folder = config.placement_folder_name(author_label);
        dest_root = dest_root.join(folder);
    }

    ensure_dir_exists(&dest_root).map_err(|e| InboxError::Io { message: e })?;

    let mut placed = Vec::new();
    for name in mod_names {
        let src = staging.join(name);
        if !src.exists() {
            continue;
        }
        let dest = unique_dest(&dest_root, name);
        let dest_name = dest
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| name.clone());

        if src.is_dir() {
            copy_tree(&src, &dest)?;
            let _ = fs::remove_dir_all(&src);
        } else {
            fs::copy(&src, &dest).map_err(|e| InboxError::Io {
                message: e.to_string(),
            })?;
            let _ = fs::remove_file(&src);
        }
        placed.push(if by_author {
            format!("{}/{}", config.placement_folder_name(author_label), dest_name)
        } else {
            dest_name
        });
    }

    Ok(placed)
}

fn unique_dest(root: &Path, name: &str) -> std::path::PathBuf {
    let dest = root.join(name);
    if !dest.exists() {
        return dest;
    }
    let mut i = 2;
    loop {
        let cand = root.join(format!("{name}_{i}"));
        if !cand.exists() {
            return cand;
        }
        i += 1;
    }
}

fn copy_tree(src: &Path, dest: &Path) -> Result<(), InboxError> {
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
                ensure_dir_exists(parent).map_err(|e| InboxError::Io {
                    message: e,
                })?;
            }
            fs::copy(entry.path(), &out).map_err(|e| InboxError::Io {
                message: e.to_string(),
            })?;
        }
    }
    Ok(())
}
