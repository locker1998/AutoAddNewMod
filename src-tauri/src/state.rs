use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::config::AppConfig;
use crate::inbox::InboxMetadataStore;
use crate::inbox::SharedWatcher;
use crate::util::app_data_dir;

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub inbox_meta: Mutex<InboxMetadataStore>,
    pub watcher: SharedWatcher,
}

impl AppState {
    pub fn new() -> Self {
        let config = AppConfig::load();
        let meta_path = app_data_dir().join("inbox_metadata.json");
        Self {
            config: Mutex::new(config),
            inbox_meta: Mutex::new(InboxMetadataStore::new(meta_path)),
            watcher: Arc::new(Mutex::new(None)),
        }
    }

    pub fn incoming_dir(&self) -> PathBuf {
        self.config.lock().incoming_dir()
    }
}
