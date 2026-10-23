use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use log::warn;
use zip::ZipArchive;

use crate::util::ensure_dir_exists;

use crate::pipeline::types::InboxError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    Rar,
    SevenZ,
    Modpkg,
    Encrypt,
    Unknown,
}

pub fn guess_kind(path: &Path) -> ArchiveKind {
    let name = path
        .file_name()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name.ends_with(".encrypt") {
        return ArchiveKind::Encrypt;
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "zip" => ArchiveKind::Zip,
        "rar" => ArchiveKind::Rar,
        "7z" => ArchiveKind::SevenZ,
        "modpkg" => ArchiveKind::Modpkg,
        _ => {
            if let Ok(mut f) = File::open(path) {
                let mut magic = [0u8; 4];
                if f.read(&mut magic).ok() == Some(4) && magic.starts_with(b"PK") {
                    return ArchiveKind::Zip;
                }
            }
            ArchiveKind::Unknown
        }
    }
}

fn find_all_mod_roots(dir: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for entry in walkdir::WalkDir::new(dir) {
        let Ok(entry) = entry else { continue };
        if entry
            .path()
            .extension()
            .map(|e| e == "modfile")
            .unwrap_or(false)
        {
            if let Some(parent) = entry.path().parent() {
                roots.push(parent.to_path_buf());
            }
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

fn unique_staging_name(staging: &Path, preferred: &str) -> String {
    let mut name = preferred.to_string();
    let mut i = 2;
    while staging.join(&name).exists() {
        name = format!("{preferred}_{i}");
        i += 1;
    }
    name
}

fn copy_tree(src: &Path, dest: &Path) -> Result<(), InboxError> {
    ensure_dir_exists(dest).map_err(|e| InboxError::Io {
        message: e.to_string(),
    })?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.map_err(|e| InboxError::Io {
            message: e.to_string(),
        })?;
        let relative = entry.path().strip_prefix(src).map_err(|e| InboxError::Io {
            message: e.to_string(),
        })?;
        let out = dest.join(relative);
        if entry.file_type().is_dir() {
            ensure_dir_exists(&out).map_err(|e| InboxError::Io {
                message: e.to_string(),
            })?;
        } else {
            if let Some(parent) = out.parent() {
                ensure_dir_exists(parent).map_err(|e| InboxError::Io {
                    message: e.to_string(),
                })?;
            }
            fs::copy(entry.path(), &out).map_err(|e| InboxError::Io {
                message: e.to_string(),
            })?;
        }
    }
    Ok(())
}

fn promote_extracted_to_staging(temp: &Path, staging: &Path, fallback_name: &str) -> Result<Vec<String>, InboxError> {
    let roots = find_all_mod_roots(temp);
    let mut installed = Vec::new();

    if roots.is_empty() {
        // No .modfile ??install whole extract as one mod (or each top-level folder)
        let top_dirs: Vec<_> = fs::read_dir(temp)
            .map_err(|e| InboxError::Io {
                message: e.to_string(),
            })?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();

        let top_files: Vec<_> = fs::read_dir(temp)
            .map_err(|e| InboxError::Io {
                message: e.to_string(),
            })?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .collect();

        if !top_dirs.is_empty() && top_files.is_empty() {
            for dir in top_dirs {
                let name = unique_staging_name(
                    staging,
                    &dir.file_name().to_string_lossy(),
                );
                let dest = staging.join(&name);
                copy_tree(&dir.path(), &dest)?;
                installed.push(name);
            }
        } else {
            let name = unique_staging_name(staging, fallback_name);
            let dest = staging.join(&name);
            copy_tree(temp, &dest)?;
            installed.push(name);
        }
    } else {
        for root in roots {
            // If the .modfile sits at the extract root, root == temp (.inbox_tmp_*),
            // so use the archive stem instead of the temp folder name.
            let preferred = if root == temp {
                fallback_name.to_string()
            } else {
                root.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| fallback_name.to_string())
            };
            let name = unique_staging_name(staging, &preferred);
            let dest = staging.join(&name);
            copy_tree(&root, &dest)?;
            installed.push(name);
        }
    }

    Ok(installed)
}

pub fn install_folder_as_mods(path: &Path, staging: &Path) -> Result<Vec<String>, InboxError> {
    let preferred = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "mod".into());

    // If the folder itself is a mod (has .modfile at any depth), promote roots
    let roots = find_all_mod_roots(path);
    if !roots.is_empty() {
        let mut installed = Vec::new();
        for root in roots {
            let name_pref = root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| preferred.clone());
            let name = unique_staging_name(staging, &name_pref);
            let dest = staging.join(&name);
            copy_tree(&root, &dest)?;
            installed.push(name);
        }
        return Ok(installed);
    }

    let name = unique_staging_name(staging, &preferred);
    let dest = staging.join(&name);
    copy_tree(path, &dest)?;
    Ok(vec![name])
}

fn extract_zip_to(path: &Path, dest: &Path, password: Option<&str>) -> Result<(), InboxError> {
    let file = File::open(path).map_err(|_| InboxError::PathNotFound {
        path: path.to_string_lossy().to_string(),
    })?;

    let mut archive = ZipArchive::new(file).map_err(|_| InboxError::InvalidArchive)?;

    let needs_password = (0..archive.len()).any(|i| {
        archive
            .by_index_raw(i)
            .map(|f| f.encrypted())
            .unwrap_or(false)
    });

    if needs_password && password.is_none() {
        return Err(InboxError::PasswordRequired);
    }

    for i in 0..archive.len() {
        let mut file = if let Some(pw) = password {
            archive.by_index_decrypt(i, pw.as_bytes()).map_err(|e| {
                let msg = e.to_string().to_lowercase();
                if msg.contains("password") || msg.contains("invalid") {
                    InboxError::PasswordRequired
                } else {
                    InboxError::InvalidArchive
                }
            })?
        } else {
            archive.by_index(i).map_err(|_| InboxError::InvalidArchive)?
        };

        let Some(enclosed) = file.enclosed_name() else {
            continue;
        };
        let out = dest.join(enclosed);

        if file.is_dir() {
            ensure_dir_exists(&out).map_err(|e| InboxError::Io {
                message: e.to_string(),
            })?;
        } else {
            if let Some(parent) = out.parent() {
                ensure_dir_exists(parent).map_err(|e| InboxError::Io {
                    message: e.to_string(),
                })?;
            }
            let mut outfile = File::create(&out).map_err(|e| InboxError::Io {
                message: e.to_string(),
            })?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| InboxError::Io {
                message: e.to_string(),
            })?;
        }
    }

    Ok(())
}

fn extract_7z_to(path: &Path, dest: &Path, password: Option<&str>) -> Result<(), InboxError> {
    let result = if let Some(pw) = password {
        sevenz_rust2::decompress_file_with_password(path, dest, pw.into())
    } else {
        sevenz_rust2::decompress_file(path, dest)
    };

    result.map_err(|e| {
        let msg = e.to_string().to_lowercase();
        if msg.contains("password") || msg.contains("encrypted") {
            InboxError::PasswordRequired
        } else {
            warn!("7z extract failed: {:?}", e);
            InboxError::InvalidArchive
        }
    })
}

fn extract_rar_to(path: &Path, dest: &Path, password: Option<&str>) -> Result<(), InboxError> {
    let archive = if let Some(pw) = password {
        unrar::Archive::with_password(path, pw)
    } else {
        unrar::Archive::new(path)
    };

    let mut archive = archive.open_for_processing().map_err(|e| {
        warn!("Failed to open RAR: {:?}", e);
        InboxError::InvalidArchive
    })?;

    loop {
        match archive.read_header() {
            Ok(Some(header)) => {
                archive = header.extract_with_base(dest).map_err(|e| {
                    let msg = format!("{:?}", e).to_lowercase();
                    if msg.contains("password") || msg.contains("bad password") {
                        InboxError::PasswordRequired
                    } else {
                        warn!("RAR extract entry failed: {:?}", e);
                        InboxError::InvalidArchive
                    }
                })?;
            }
            Ok(None) => break,
            Err(e) => {
                let msg = format!("{:?}", e).to_lowercase();
                if msg.contains("password") {
                    return Err(InboxError::PasswordRequired);
                }
                warn!("RAR header error: {:?}", e);
                return Err(InboxError::InvalidArchive);
            }
        }
    }

    Ok(())
}

/// Try passwords in the given order only (no extra empty attempt).
/// Empty list means one attempt with no password.
pub fn extract_mods_password_order(
    path: &Path,
    staging: &Path,
    passwords: &[String],
) -> Result<Vec<String>, InboxError> {
    let kind = guess_kind(path);
    if !matches!(
        kind,
        ArchiveKind::Zip | ArchiveKind::Rar | ArchiveKind::SevenZ
    ) {
        return Err(InboxError::UnsupportedFormat);
    }

    let fallback = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "mod".into());

    let temp = staging.join(format!(".inbox_tmp_{}", fallback));
    let _ = fs::remove_dir_all(&temp);
    ensure_dir_exists(&temp).map_err(|e| InboxError::Io {
        message: e.to_string(),
    })?;

    let attempts: Vec<Option<&str>> = if passwords.is_empty() {
        vec![None]
    } else {
        passwords.iter().map(|p| Some(p.as_str())).collect()
    };

    let mut last_err = InboxError::InvalidArchive;
    let mut ok = false;
    for pw in attempts {
        let _ = fs::remove_dir_all(&temp);
        let _ = ensure_dir_exists(&temp);
        let result = match kind {
            ArchiveKind::Zip => extract_zip_to(path, &temp, pw),
            ArchiveKind::SevenZ => extract_7z_to(path, &temp, pw),
            ArchiveKind::Rar => extract_rar_to(path, &temp, pw),
            _ => Err(InboxError::UnsupportedFormat),
        };
        match result {
            Ok(()) => {
                ok = true;
                break;
            }
            Err(e) => last_err = e,
        }
    }

    if !ok {
        let _ = fs::remove_dir_all(&temp);
        return Err(last_err);
    }

    let installed = promote_extracted_to_staging(&temp, staging, &fallback);
    let _ = fs::remove_dir_all(&temp);
    installed
}
