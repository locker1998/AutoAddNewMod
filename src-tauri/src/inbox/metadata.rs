use std::{collections::HashMap, fs, path::PathBuf};

use log::{info, warn};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InboxItemMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_id: Option<String>,
}

pub struct InboxMetadataStore {
    path: PathBuf,
    data: HashMap<String, InboxItemMetadata>,
}

impl InboxMetadataStore {
    pub fn new(path: PathBuf) -> Self {
        let mut store = Self {
            path,
            data: HashMap::new(),
        };
        store.load();
        store
    }

    fn load(&mut self) {
        if self.path.exists() {
            match fs::read_to_string(&self.path) {
                Ok(contents) => match serde_json::from_str(&contents) {
                    Ok(data) => {
                        self.data = data;
                        info!("Loaded inbox metadata from {:?}", self.path);
                    }
                    Err(e) => warn!("Failed to parse inbox metadata: {:?}", e),
                },
                Err(e) => warn!("Failed to read inbox metadata: {:?}", e),
            }
        }
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(&self.data) {
            Ok(json) => {
                if let Err(e) = fs::write(&self.path, json) {
                    warn!("Failed to save inbox metadata: {:?}", e);
                }
            }
            Err(e) => warn!("Failed to serialize inbox metadata: {:?}", e),
        }
    }

    pub fn get(&self, name: &str) -> InboxItemMetadata {
        self.data.get(name).cloned().unwrap_or_default()
    }

    pub fn set_meta(
        &mut self,
        names: &[String],
        game_id: Option<String>,
        author: Option<String>,
    ) {
        for name in names {
            let entry = self.data.entry(name.clone()).or_default();
            if game_id.is_some() {
                entry.game_id = game_id.clone();
            }
            if author.is_some() {
                entry.author = author.clone();
            }
        }
        self.save();
    }

    pub fn remove(&mut self, name: &str) {
        if self.data.remove(name).is_some() {
            self.save();
        }
    }
}
