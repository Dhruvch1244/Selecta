//! Selecta's core. Physically separate from the app shell: this crate must
//! build and test with no Tauri, no webview and no UI toolchain installed
//! (CI checks it on its own). Anything the UI shows comes from here as plain
//! serde types, exported to TypeScript by ts-rs, so the two sides never drift.

pub mod analysis;
pub mod decode;
pub mod jobs;
pub mod library;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("decode: {0}")]
    Decode(String),
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;
