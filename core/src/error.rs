use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    /// Nothing is listening on the Zotero port (Zotero is not running).
    #[error("Zotero is not running (no server on {0})")]
    ZoteroOffline(String),

    /// Zotero is running but the companion plugin did not answer.
    #[error("Zotero Notebook plugin is not installed or not responding")]
    PluginMissing,

    /// Zotero rejected the request (e.g. "Allow other applications" is off).
    #[error("Zotero rejected the request (HTTP {status}): {message}")]
    ZoteroRejected { status: u16, message: String },

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("LLM provider error ({provider}): {message}")]
    Llm { provider: String, message: String },

    #[error("API key for {0} is not configured")]
    MissingApiKey(String),

    #[error("keychain error: {0}")]
    Keychain(String),

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn llm(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Error::Llm {
            provider: provider.into(),
            message: message.into(),
        }
    }
}

impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
