//! Zotero Notebook core.
//!
//! Everything in this crate is headless: no Tauri, no GTK, no window system.
//! The Tauri shell (`app/src-tauri`) is a thin wrapper that forwards commands
//! here. All HTTP endpoints (Zotero local API, the companion plugin, LLM
//! providers) take a configurable base URL so the whole crate can be tested
//! against mock servers (see `core/tests`).

pub mod error;
pub mod models;

pub mod zotero {
    //! Clients for the two HTTP surfaces of a locally running Zotero:
    //! - `plugin_api`: the Zotero Notebook companion plugin (read + write),
    //!   see `docs/PLUGIN_API.md`.
    //! - `local_api`: Zotero's built-in read-only local API (7+) under
    //!   `/api/users/0`, used as a degraded fallback when the plugin is not
    //!   installed.
    pub mod local_api;
    pub mod plugin_api;
}

pub mod llm {
    //! LLM providers behind a common interface (`AnyProvider`).
    pub mod anthropic;
    pub mod gemini;
    pub mod provider;
    pub use provider::{AnyProvider, ClassifyRequest, ClassifyResponse, SummarizeRequest};
}

pub mod abstract_lookup;
pub mod classify;
pub mod db;
pub mod keychain;
pub mod settings;

pub use error::{Error, Result};
