//! Evo secret configuration with redacted errors and scoped log suppression.
use std::{
    fmt, fs,
    io::Read,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use evo_core_entity::UId;
use evo_core_env::UEnv;
use evo_framework::EnumError;
use zeroize::Zeroizing;

use crate::UOpenjevClient;

const TOKEN_NAME: &str = "TYPESAFE_TOKEN";
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Credentials cannot be serialized or read through a public token getter.
pub struct UOpenjevCredentials {
    token: Zeroizing<String>,
}

impl UOpenjevCredentials {
    /// Load the standard Evo `[TYPESAFE_TOKEN]` table from secret_env.toml.
    /// Call at single-threaded startup: Evo's environment map and logger are global.
    pub fn from_path_config(path: &str) -> Result<Self, EnumError> {
        let _quiet = USecretLogGuard::new();
        let id = UId::id_str(TOKEN_NAME);
        UEnv::del_env(&id);
        let result = (|| {
            // Preflight the entire file because upstream from_path_config discards
            // parse errors. Never attach parser errors, which can contain secrets.
            let file = fs::File::open(path).map_err(|_| EnumError::NotValidData)?;
            let mut contents = Zeroizing::new(String::new());
            file.take(MAX_CONFIG_BYTES + 1)
                .read_to_string(&mut contents)
                .map_err(|_| EnumError::NotValidData)?;
            if contents.len() as u64 > MAX_CONFIG_BYTES {
                return Err(EnumError::NotValidData);
            }
            let table: toml::Table =
                toml::from_str(&contents).map_err(|_| EnumError::NotValidData)?;
            for (name, entry) in &table {
                if name == "evo_version" {
                    continue;
                }
                let entry = entry.as_table().ok_or(EnumError::NotValidData)?;
                let enabled = match entry.get("enabled") {
                    None => true,
                    Some(value) => value.as_bool().ok_or(EnumError::NotValidData)?,
                };
                if enabled && entry.get("value").and_then(toml::Value::as_str).is_none() {
                    return Err(EnumError::NotValidData);
                }
            }
            let entry = table
                .get(TOKEN_NAME)
                .and_then(toml::Value::as_table)
                .ok_or(EnumError::NotValidData)?;
            if entry.get("enabled").and_then(toml::Value::as_bool) == Some(false) {
                return Err(EnumError::NotValidData);
            }
            let expected = entry
                .get("value")
                .and_then(toml::Value::as_str)
                .ok_or(EnumError::NotValidData)?;
            Self::validate(expected)?;
            UEnv::from_path_config(path).map_err(|_| EnumError::NotValidData)?;
            let token =
                Zeroizing::new(UEnv::get_env_value(&id).map_err(|_| EnumError::NotValidData)?);
            Self::validate(&token)?;
            if token.as_str() != expected {
                return Err(EnumError::NotValidData);
            }
            Ok(Self { token })
        })();
        // Never let a later failed load reuse this credential from the global map.
        UEnv::del_env(&id);
        result
    }

    fn validate(token: &str) -> Result<(), EnumError> {
        if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(EnumError::NotValidData);
        }
        Ok(())
    }

    /// Create a reusable client without exposing the token to the caller.
    pub fn client(&self, base_url: &str, timeout: Duration) -> anyhow::Result<UOpenjevClient> {
        UOpenjevClient::new(&self.token, base_url, timeout)
    }
}

impl fmt::Debug for UOpenjevCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UOpenjevCredentials { [REDACTED] }")
    }
}

static SECRET_LOG_LOCK: Mutex<()> = Mutex::new(());
struct USecretLogGuard {
    previous: log::LevelFilter,
    _lock: MutexGuard<'static, ()>,
}
impl USecretLogGuard {
    fn new() -> Self {
        let lock = SECRET_LOG_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let previous = log::max_level();
        log::set_max_level(log::LevelFilter::Off);
        Self {
            previous,
            _lock: lock,
        }
    }
}
impl Drop for USecretLogGuard {
    fn drop(&mut self) {
        log::set_max_level(self.previous);
    }
}
