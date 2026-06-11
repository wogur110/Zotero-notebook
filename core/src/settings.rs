//! JSON settings persistence (app config dir). Loading is infallible:
//! anything unreadable falls back to defaults so the app always starts.

use std::path::Path;

use crate::error::Result;
use crate::models::AppSettings;

pub fn load(path: &Path) -> AppSettings {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("settings file at {path:?} is corrupt ({e}); using defaults");
                AppSettings::default()
            }
        },
        Err(_) => AppSettings::default(),
    }
}

pub fn save(path: &Path, settings: &AppSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(settings)
        .expect("AppSettings always serializes");
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
