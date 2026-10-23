use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::pipeline::mod_logic::{
    default_mod_logic_template, parse_mod_logic, strip_legacy_decrypt_commands,
};
use crate::util::{app_data_dir, default_incoming_dir, ensure_dir_exists};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GameAuthorSettings {
    /// Stable id (slug). Older configs used `profileId`.
    #[serde(default, alias = "profileId")]
    pub id: String,
    /// Display / tagging name shown in Inbox.
    #[serde(default)]
    pub name: String,
    /// Legacy; kept for config migration only.
    #[serde(default = "default_auth_mode")]
    pub auth_mode: String,
    /// Legacy single key path.
    #[serde(default)]
    pub key_path: Option<String>,
    /// Legacy password list.
    #[serde(default)]
    pub passwords: Vec<String>,
    /// Values for fields declared in custom_logic (`field_values.key` etc).
    #[serde(default)]
    pub field_values: HashMap<String, String>,
    /// Author processing script.
    #[serde(default)]
    pub custom_logic: String,
}

fn default_auth_mode() -> String {
    "key".to_string()
}

pub fn slugify_author_name(name: &str) -> String {
    let mut s = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    s.trim_matches('-').to_string()
}

impl GameAuthorSettings {
    pub fn field_value(&self, id: &str) -> Option<&str> {
        self.field_values
            .get(id)
            .map(|s| s.as_str())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                if id == "key" || id == "token" {
                    self.key_path.as_deref().filter(|s| !s.trim().is_empty())
                } else {
                    None
                }
            })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GameConfig {
    pub id: String,
    pub name: String,
    pub mod_folder: String,
    #[serde(default = "default_true")]
    pub organize_by_author: bool,
    #[serde(default)]
    pub authors: Vec<GameAuthorSettings>,
}

fn default_true() -> bool {
    true
}

const CONFIG_VERSION: u32 = 9;

fn default_config_version() -> u32 {
    0
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    #[serde(default = "default_config_version")]
    pub config_version: u32,
    pub games: Vec<GameConfig>,
    pub incoming_directory: Option<String>,
    #[serde(default)]
    pub author_user_names: HashMap<String, String>,
    #[serde(default)]
    pub author_keys: HashMap<String, String>,
    #[serde(default)]
    pub author_passwords: HashMap<String, Vec<String>>,
    pub last_game_id: Option<String>,
    pub last_author: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PartialAppConfig {
    pub games: Option<Vec<GameConfig>>,
    pub incoming_directory: Option<String>,
    pub author_user_names: Option<HashMap<String, String>>,
    pub author_keys: Option<HashMap<String, String>>,
    pub author_passwords: Option<HashMap<String, Vec<String>>>,
    pub last_game_id: Option<String>,
    pub last_author: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            games: Vec::new(),
            incoming_directory: None,
            author_user_names: HashMap::new(),
            author_keys: HashMap::new(),
            author_passwords: HashMap::new(),
            last_game_id: None,
            last_author: None,
        }
    }
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        app_data_dir().join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        let mut cfg = if path.exists() {
            match fs::read_to_string(&path) {
                Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
                Err(e) => {
                    warn!("Failed to read config: {e}");
                    Self::default()
                }
            }
        } else {
            Self::default()
        };
        if cfg.config_version < 2 {
            cfg.games.clear();
            cfg.author_keys.clear();
            cfg.author_passwords.clear();
            cfg.last_game_id = None;
            cfg.last_author = None;
            cfg.config_version = 2;
        }
        if cfg.config_version < CONFIG_VERSION {
            if cfg.config_version < 3 {
                cfg.migrate_authors_v3();
            }
            if cfg.config_version < 4 {
                cfg.migrate_authors_v4();
            }
            if cfg.config_version < 5 {
                cfg.migrate_authors_v5();
            }
            if cfg.config_version < 6 {
                cfg.migrate_authors_v6();
            }
            if cfg.config_version < 7 {
                cfg.migrate_authors_v7();
            }
            cfg.config_version = CONFIG_VERSION;
        }
        cfg.sanitize_game_authors();
        cfg.migrate_legacy_author_secrets();
        cfg.normalize_incoming();
        let _ = cfg.save();
        cfg
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            ensure_dir_exists(parent)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())?;
        info!("Saved config to {:?}", path);
        Ok(())
    }

    pub fn merge_from(&mut self, partial: PartialAppConfig) {
        if let Some(games) = partial.games {
            self.games = games;
        }
        if let Some(incoming) = partial.incoming_directory {
            self.incoming_directory = Some(incoming);
        }
        if let Some(author_user_names) = partial.author_user_names {
            for (id, name) in author_user_names {
                let trimmed = name.trim().to_string();
                if !trimmed.is_empty() {
                    self.author_user_names.insert(id, trimmed);
                }
            }
        }
        if let Some(author_keys) = partial.author_keys {
            self.author_keys = author_keys;
        }
        if let Some(author_passwords) = partial.author_passwords {
            self.author_passwords = author_passwords;
        }
        if let Some(last_game_id) = partial.last_game_id {
            self.last_game_id = Some(last_game_id);
        }
        if let Some(last_author) = partial.last_author {
            self.last_author = Some(last_author);
        }
        self.sanitize_game_authors();
        self.normalize_incoming();
    }

    pub fn normalize_incoming(&mut self) {
        let resolved = match self.incoming_directory.as_ref().map(|s| s.trim()) {
            Some(path) if !path.is_empty() => PathBuf::from(path),
            _ => default_incoming_dir(),
        };
        let _ = ensure_dir_exists(&resolved);
        self.incoming_directory = Some(resolved.to_string_lossy().to_string());
    }

    fn migrate_authors_v3(&mut self) {
        for game in &mut self.games {
            for author in &mut game.authors {
                if author.name.trim().is_empty() {
                    author.name = author.id.clone();
                }
                if author.id.trim().is_empty() {
                    author.id = slugify_author_name(&author.name);
                }
                if author.custom_logic.trim().is_empty() {
                    author.custom_logic = default_mod_logic_template();
                }
            }
        }
    }

    fn migrate_authors_v4(&mut self) {
        for game in &mut self.games {
            for author in &mut game.authors {
                if let Some(path) = author.key_path.clone() {
                    if !path.trim().is_empty() {
                        author
                            .field_values
                            .entry("key".into())
                            .or_insert(path.clone());
                        author
                            .field_values
                            .entry("token".into())
                            .or_insert(path);
                    }
                }
                if !author.passwords.is_empty()
                    && !author.field_values.contains_key("passwords")
                {
                    author
                        .field_values
                        .insert("passwords".into(), author.passwords.join("\n"));
                }
            }
        }
    }

    /// Fill empty or unreadable scripts with the generic extract template.
    fn migrate_authors_v5(&mut self) {
        for game in &mut self.games {
            for author in &mut game.authors {
                let trimmed = author.custom_logic.trim();
                if trimmed.is_empty()
                    || parse_mod_logic(&strip_legacy_decrypt_commands(trimmed)).is_err()
                {
                    author.custom_logic = default_mod_logic_template();
                }
            }
        }
    }

    fn migrate_authors_v6(&mut self) {
        for game in &mut self.games {
            for author in &mut game.authors {
                let stripped = strip_legacy_decrypt_commands(&author.custom_logic);
                author.custom_logic = if stripped.trim().is_empty() {
                    default_mod_logic_template()
                } else {
                    stripped
                };
            }
        }
    }

    fn migrate_authors_v7(&mut self) {
        for game in &mut self.games {
            for author in &mut game.authors {
                if parse_mod_logic(&author.custom_logic).is_err() {
                    author.custom_logic = default_mod_logic_template();
                }
            }
        }
    }

    pub fn sanitize_game_authors(&mut self) {
        for game in &mut self.games {
            for author in &mut game.authors {
                author.name = author.name.trim().to_string();
                if author.id.trim().is_empty() {
                    author.id = slugify_author_name(&author.name);
                }
                author.custom_logic = strip_legacy_decrypt_commands(&author.custom_logic);
                if author.custom_logic.trim().is_empty() {
                    author.custom_logic = default_mod_logic_template();
                }
                // Keep legacy mirrors in sync for older tooling.
                if let Some(v) = author.field_values.get("key").cloned() {
                    author.key_path = Some(v);
                } else if let Some(v) = author.field_values.get("token").cloned() {
                    author.key_path = Some(v);
                }
                if let Some(v) = author.field_values.get("passwords") {
                    author.passwords = v
                        .lines()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
            let mut seen = HashSet::new();
            game.authors
                .retain(|a| !a.name.is_empty() && seen.insert(a.id.to_lowercase()));
        }
    }

    pub fn migrate_legacy_author_secrets(&mut self) {
        if self.author_keys.is_empty() && self.author_passwords.is_empty() {
            return;
        }
        for game in &mut self.games {
            for author in &mut game.authors {
                if author.key_path.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
                    if let Some(path) = self.author_keys.get(&author.id) {
                        if !path.is_empty() {
                            author.key_path = Some(path.clone());
                            author.auth_mode = "key".to_string();
                        }
                    }
                }
                if author.passwords.is_empty() {
                    if let Some(pws) = self.author_passwords.get(&author.id) {
                        if !pws.is_empty() {
                            author.passwords = pws.clone();
                            if author.key_path.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
                                author.auth_mode = "password".to_string();
                            }
                        }
                    }
                }
            }
        }
        self.author_keys.clear();
        self.author_passwords.clear();
        let _ = self.save();
    }

    pub fn find_game(&self, game_id: &str) -> Option<&GameConfig> {
        self.games.iter().find(|g| g.id == game_id)
    }

    pub fn find_game_author_by_label(
        &self,
        game_id: &str,
        label: &str,
    ) -> Option<&GameAuthorSettings> {
        let trimmed = label.trim();
        if trimmed.is_empty() {
            return None;
        }
        self.find_game(game_id)?.authors.iter().find(|a| {
            a.name.eq_ignore_ascii_case(trimmed) || a.id.eq_ignore_ascii_case(trimmed)
        })
    }

    pub fn passwords_for_game_author(&self, game_id: &str, author_label: &str) -> Vec<String> {
        let Some(author) = self.find_game_author_by_label(game_id, author_label) else {
            return Vec::new();
        };
        if let Some(raw) = author
            .field_values
            .get("passwords")
            .or_else(|| author.field_values.get("password"))
        {
            return raw
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        author.passwords.clone()
    }

    pub fn auth_ready(&self, game_id: &str, author_label: &str) -> bool {
        let Some(author) = self.find_game_author_by_label(game_id, author_label) else {
            return false;
        };
        let Ok(logic) = parse_mod_logic(&author.custom_logic) else {
            return false;
        };
        for field in &logic.fields {
            if !field.required {
                continue;
            }
            let empty = author
                .field_value(&field.id)
                .map(|s| s.trim().is_empty())
                .unwrap_or(true);
            if empty {
                if (field.id == "passwords" || field.id == "password")
                    && !self
                        .passwords_for_game_author(game_id, author_label)
                        .is_empty()
                {
                    continue;
                }
                return false;
            }
        }
        true
    }

    pub fn incoming_dir(&self) -> PathBuf {
        self.incoming_directory
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(default_incoming_dir)
    }

    pub fn placement_folder_name(&self, author: &str) -> String {
        author.trim().to_string()
    }
}

pub fn ensure_incoming(config: &AppConfig) -> Result<PathBuf, String> {
    let dir = config.incoming_dir();
    ensure_dir_exists(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_game_has_no_builtin_authors() {
        let game = GameConfig {
            id: "any-game".into(),
            name: "Any Game".into(),
            mod_folder: String::new(),
            organize_by_author: true,
            authors: Vec::new(),
        };
        assert!(game.authors.is_empty());
    }
}
