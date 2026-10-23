use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InboxItemKind {
    Archive,
    Encrypted,
    Folder,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub name: String,
    pub path: String,
    pub kind: InboxItemKind,
    pub size: u64,
    pub game_id: Option<String>,
    pub author: Option<String>,
    pub requires_decryption: bool,
    pub has_decrypt_key: bool,
    pub has_decryptor: bool,
    pub ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum InboxError {
    PathNotFound { path: String },
    InvalidName,
    AlreadyExists { name: String },
    UnsupportedFormat,
    InvalidArchive,
    PasswordRequired,
    ModAlreadyExists { name: String },
    AuthorRequired,
    GameRequired,
    GameNotFound { game_id: String },
    ModFolderMissing { game: String },
    KeyRequired { author: String },
    DecryptFailed { message: String },
    DecryptNotConfigured { game: String, author: String },
    Io { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractItemResult {
    pub name: String,
    pub success: bool,
    pub installed_mods: Vec<String>,
    pub error: Option<InboxError>,
    pub needs_password: bool,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInboxResult {
    pub results: Vec<ExtractItemResult>,
    pub installed_mods: Vec<String>,
}
