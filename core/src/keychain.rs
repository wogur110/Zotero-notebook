//! API keys in the OS keychain (Windows Credential Manager, macOS Keychain,
//! Linux Secret Service). Keys never touch the settings file or the DB.

use keyring::Entry;

use crate::error::{Error, Result};
use crate::models::ProviderId;

const SERVICE: &str = "zotero-notebook";

fn entry(provider: ProviderId) -> Result<Entry> {
    Entry::new(SERVICE, &format!("{}-api-key", provider.as_str()))
        .map_err(|e| Error::Keychain(e.to_string()))
}

pub fn save_key(provider: ProviderId, key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() {
        return Err(Error::Other("API key must not be empty".into()));
    }
    entry(provider)?
        .set_password(key)
        .map_err(|e| Error::Keychain(e.to_string()))
}

pub fn get_key(provider: ProviderId) -> Result<Option<String>> {
    match entry(provider)?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(Error::Keychain(e.to_string())),
    }
}

pub fn has_key(provider: ProviderId) -> Result<bool> {
    Ok(get_key(provider)?.is_some())
}

pub fn delete_key(provider: ProviderId) -> Result<()> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(Error::Keychain(e.to_string())),
    }
}
