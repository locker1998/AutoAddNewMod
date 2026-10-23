use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn ensure_dir_exists(path: &Path) -> Result<(), String> {
    if !path.exists() {
        fs::create_dir_all(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn app_data_dir() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("AutoAddNewMod")
}

pub fn default_incoming_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join("Incoming");
        }
    }
    app_data_dir().join("Incoming")
}
