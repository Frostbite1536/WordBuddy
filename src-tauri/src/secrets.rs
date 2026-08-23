//! OS-vault secret storage (audit M13 / M-tier hardening).
//!
//! API keys and the extension relay token live in the operating
//! system's credential vault instead of a plaintext JSON file:
//!
//! - Windows: Credential Manager (DPAPI-backed)
//! - macOS:   Keychain (Security framework)
//! - Linux:   Secret Service (GNOME Keyring / KWallet over DBus)
//!
//! One code path serves all three, so the linux/macos compatibility
//! work needs no storage rework. When no OS vault is reachable
//! (headless Linux without a Secret Service daemon) we fall back to a
//! 0600 file next to config.json and log loudly — degraded, not silent.

use std::collections::HashMap;
use std::path::PathBuf;

const SERVICE: &str = "wordbuddy";

/// Canonical secret names used across the app.
pub const RELAY_TOKEN_KEY: &str = "relay-token";
pub fn api_key_name(service: &str) -> String {
    format!("api-key:{service}")
}

fn fallback_path() -> Result<PathBuf, String> {
    let base = dirs_next::config_dir().ok_or_else(|| "no config dir".to_string())?;
    let dir = base.join("wordbuddy");
    std::fs::create_dir_all(&dir).map_err(|e| format!("config dir: {e}"))?;
    Ok(dir.join("secrets.json"))
}

fn fallback_read_all() -> HashMap<String, String> {
    let Ok(path) = fallback_path() else { return HashMap::new() };
    let Ok(data) = std::fs::read_to_string(&path) else { return HashMap::new() };
    serde_json::from_str(&data).unwrap_or_default()
}

fn fallback_write_all(map: &HashMap<String, String>) {
    let Some(path) = fallback_path().ok() else { return };
    if let Ok(data) = serde_json::to_string_pretty(map) {
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .and_then(|mut f| f.write_all(data.as_bytes()));
        }
        #[cfg(not(unix))]
        {
            let _ = std::fs::write(&path, &data);
        }
    }
}

pub fn fallback_get(key: &str) -> Option<String> {
    fallback_read_all().get(key).cloned()
}

pub fn fallback_set(key: &str, value: &str) -> Result<(), String> {
    let mut map = fallback_read_all();
    map.insert(key.to_string(), value.to_string());
    fallback_write_all(&map);
    Ok(())
}

pub fn fallback_delete(key: &str) {
    let mut map = fallback_read_all();
    if map.remove(key).is_some() {
        fallback_write_all(&map);
    }
}

/// Read a secret from the OS vault; falls back to the file store when
/// the entry isn't in the vault (e.g. written by an older build, or by
/// the fallback path itself). `None` = genuinely not configured.
pub fn get_secret(key: &str) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(SERVICE, key).map_err(|e| format!("vault entry: {e}"))?;
    match entry.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(fallback_get(key)),
        Err(e) => Err(format!("vault read {key}: {e}")),
    }
}

/// Store a secret in the OS vault. If the platform vault is unavailable,
/// degrades to the 0600 fallback file with a loud log line.
pub fn set_secret(key: &str, value: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, key).map_err(|e| format!("vault entry: {e}"))?;
    match entry.set_password(value) {
        Ok(()) => {
            // Vault write succeeded — clear any stale fallback copy so
            // the plaintext store never lags the vault.
            fallback_delete(key);
            Ok(())
        }
        Err(e) => {
            eprintln!(
                "[secrets] OS vault unavailable for '{key}' ({e}); \
                 falling back to file storage — consider configuring a keyring service"
            );
            fallback_set(key, value)
        }
    }
}

/// Remove a secret from both stores (vault failure is non-fatal here —
/// best-effort cleanup of whichever copy exists).
pub fn delete_secret(key: &str) {
    if let Ok(entry) = keyring::Entry::new(SERVICE, key) {
        if let Err(e) = entry.delete_credential() {
            if !matches!(e, keyring::Error::NoEntry) {
                eprintln!("[secrets] vault delete {key}: {e}");
            }
        }
    }
    fallback_delete(key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_store_roundtrips_and_deletes() {
        let key = "test-fallback-roundtrip";
        assert_eq!(fallback_get(key), None);
        fallback_set(key, "s3cret").unwrap();
        assert_eq!(fallback_get(key).as_deref(), Some("s3cret"));
        fallback_delete(key);
        assert_eq!(fallback_get(key), None);
    }

    #[test]
    fn os_vault_set_get_delete() {
        // Exercises the real OS vault (Credential Manager / Keychain /
        // Secret Service). Uses a dedicated key and removes it after.
        let key = "test-vault-roundtrip";
        set_secret(key, "vault-value").expect("set_secret");
        let read = get_secret(key).expect("get_secret");
        assert_eq!(read.as_deref(), Some("vault-value"));
        delete_secret(key);
        assert_eq!(get_secret(key).unwrap(), None);
    }

    #[test]
    fn api_key_name_format_is_stable() {
        // Wire format other code paths rely on; changing it orphans
        // stored credentials.
        assert_eq!(api_key_name("anthropic"), "api-key:anthropic");
    }
}
