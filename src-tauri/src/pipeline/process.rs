use std::{
    collections::HashMap,
    fs,
    path::Path,
};

use log::{info, warn};

use crate::config::AppConfig;
use crate::pipeline::extract::install_folder_as_mods;
use crate::pipeline::logic::{parse, run, LogicHost, ScriptResult};
use crate::pipeline::place::place_mods;
use crate::pipeline::types::{ExtractItemResult, InboxError, ProcessInboxResult};
use crate::util::ensure_dir_exists;

pub fn process_items(
    names: &[String],
    passwords: &HashMap<String, String>,
    incoming_dir: &Path,
    config: &AppConfig,
    get_meta: impl Fn(&str) -> (Option<String>, Option<String>),
    mut on_success: impl FnMut(&str),
) -> Result<ProcessInboxResult, InboxError> {
    let staging = incoming_dir.join(".process_staging");
    ensure_dir_exists(&staging).map_err(|e| InboxError::Io { message: e })?;

    let mut results = Vec::new();
    let mut all_installed = Vec::new();

    for name in names {
        let path = incoming_dir.join(name);
        if !path.exists() {
            results.push(ExtractItemResult {
                name: name.clone(),
                success: false,
                installed_mods: vec![],
                error: Some(InboxError::PathNotFound {
                    path: path.to_string_lossy().to_string(),
                }),
                needs_password: false,
                message: None,
            });
            continue;
        }

        let (game_id, author) = get_meta(name);
        let Some(game_id) = game_id.filter(|s| !s.trim().is_empty()) else {
            results.push(ExtractItemResult {
                name: name.clone(),
                success: false,
                installed_mods: vec![],
                error: Some(InboxError::GameRequired),
                needs_password: false,
                message: None,
            });
            continue;
        };
        let Some(author) = author.filter(|s| !s.trim().is_empty()) else {
            results.push(ExtractItemResult {
                name: name.clone(),
                success: false,
                installed_mods: vec![],
                error: Some(InboxError::AuthorRequired),
                needs_password: false,
                message: None,
            });
            continue;
        };

        if config.find_game_author_by_label(&game_id, &author).is_none() {
            results.push(ExtractItemResult {
                name: name.clone(),
                success: false,
                installed_mods: vec![],
                error: Some(InboxError::AuthorRequired),
                needs_password: false,
                message: None,
            });
            continue;
        }

        let password = passwords
            .get(name)
            .cloned()
            .or_else(|| passwords.get("*").cloned());

        let outcome = process_one(
            &path,
            &staging,
            &game_id,
            &author,
            password.as_deref(),
            config,
        );

        match outcome {
            Ok((installed, message)) => {
                info!("Processed '{}' -> {:?}", name, installed);
                all_installed.extend(installed.clone());
                if path.is_dir() {
                    let _ = fs::remove_dir_all(&path);
                } else {
                    let _ = fs::remove_file(&path);
                }
                on_success(name);
                results.push(ExtractItemResult {
                    name: name.clone(),
                    success: true,
                    installed_mods: installed,
                    error: None,
                    needs_password: false,
                    message,
                });
            }
            Err(InboxError::PasswordRequired) => {
                results.push(ExtractItemResult {
                    name: name.clone(),
                    success: false,
                    installed_mods: vec![],
                    error: Some(InboxError::PasswordRequired),
                    needs_password: true,
                    message: None,
                });
            }
            Err(e) => {
                let message = match &e {
                    InboxError::DecryptFailed { message } => Some(message.clone()),
                    _ => None,
                };
                warn!("Failed to process '{}': {:?}", name, e);
                results.push(ExtractItemResult {
                    name: name.clone(),
                    success: false,
                    installed_mods: vec![],
                    error: Some(e),
                    needs_password: false,
                    message,
                });
            }
        }
    }

    let _ = fs::remove_dir_all(&staging);

    Ok(ProcessInboxResult {
        results,
        installed_mods: all_installed,
    })
}

fn process_one(
    path: &Path,
    staging: &Path,
    game_id: &str,
    author_label: &str,
    password: Option<&str>,
    config: &AppConfig,
) -> Result<(Vec<String>, Option<String>), InboxError> {
    let author = config
        .find_game_author_by_label(game_id, author_label)
        .ok_or(InboxError::AuthorRequired)?;
    let stmts = parse(&author.custom_logic).map_err(|message| InboxError::DecryptFailed {
        message: format!("Logic error: {message}"),
    })?;

    let work = staging.join(format!("work_{}", uuid::Uuid::new_v4()));
    ensure_dir_exists(&work).map_err(|e| InboxError::Io { message: e })?;

    let mut host = LogicHost::new(
        path.to_path_buf(),
        work.clone(),
        &author.field_values,
        password.map(|s| s.to_string()),
    );

    let script: ScriptResult = run(&stmts, &mut host).map_err(|message| InboxError::DecryptFailed { message })?;
    let message = script.message.clone();

    if !script.success {
        let _ = fs::remove_dir_all(&work);
        return Err(InboxError::DecryptFailed {
            message: message.unwrap_or_else(|| "Logic returned false".into()),
        });
    }

    let organize_by_author = config
        .find_game(game_id)
        .map(|g| g.organize_by_author)
        .unwrap_or(true);

    let installed = if script.did_extract {
        script.extracted
    } else if path.is_dir() {
        install_folder_as_mods(path, &work)?
    } else {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "mod".into());
        let dest = work.join(&name);
        fs::copy(path, &dest).map_err(|e| InboxError::Io {
            message: e.to_string(),
        })?;
        vec![name]
    };

    let placed = place_mods(
        &work,
        &installed,
        game_id,
        author_label,
        config,
        organize_by_author,
    )?;
    let _ = fs::remove_dir_all(&work);
    Ok((placed, message))
}
