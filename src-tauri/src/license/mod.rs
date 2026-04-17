use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::AppHandle;
use time::OffsetDateTime;

pub mod commands;
mod model;
pub(crate) mod modern;
#[cfg(test)]
mod phase1_e2e_tests;
#[cfg(test)]
mod phase2_binding_tests;
#[cfg(test)]
mod phase3_policy_tests;
#[cfg(test)]
mod phase5_hybrid_groundwork_tests;
pub mod runtime;
pub(crate) mod storage;
pub(crate) mod validator;

use ed25519_dalek::PublicKey;
pub(crate) use model::{
    BindingMatch, LicenseFormatKind, NormalizedFailureReason, NormalizedLicense,
};

/// Compatibility shim that exposes the cached license status to the rest of
/// the backend while the new runtime evolves. All mutations happen through
/// `LicenseRuntime`, but existing callers can keep using `LicenseState`.
#[derive(Clone)]
pub struct LicenseState {
    cache: Arc<RwLock<Option<LicenseCache>>>,
}

#[derive(Debug, Clone)]
pub struct LicenseCache {
    pub license: NormalizedLicense,
    pub installed_at: i64,
    pub last_verified_at: i64,
}

impl Default for LicenseState {
    fn default() -> Self {
        Self {
            cache: Arc::new(RwLock::new(None)),
        }
    }
}

impl LicenseState {
    pub fn replace(&self, new_value: Option<LicenseCache>) {
        let mut guard = self.cache.write().expect("license cache poisoned");
        *guard = new_value;
    }

    pub fn snapshot(&self) -> Option<LicenseCache> {
        let guard = self.cache.read().expect("license cache poisoned");
        guard.clone()
    }
}

pub fn ensure_active(state: &LicenseState) -> Result<LicenseCache, CommandError> {
    let cache = state.snapshot().ok_or_else(|| {
        CommandError::new(
            "LicenseRequired",
            "Instala una licencia válida para continuar.",
        )
    })?;

    // Binding enforcement: a DeviceMismatch license must never be treated as
    // active regardless of its time window. This prevents any caller that holds
    // a direct LicenseState reference from bypassing the runtime status check.
    if cache.license.binding == BindingMatch::Mismatch {
        return Err(CommandError::new(
            "DeviceMismatch",
            "La licencia pertenece a otro dispositivo.",
        ));
    }

    let payload = &cache.license;
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let not_before = payload.not_before;
    let not_after = payload.not_after;
    let skew = payload.max_clock_skew;

    if now + skew < not_before {
        return Err(CommandError::new(
            "NotYetValid",
            "La licencia aún no es válida en este dispositivo.",
        ));
    }

    if now - skew > not_after {
        return Err(CommandError::new(
            "Expired",
            "La licencia ha expirado. Instala una nueva para continuar.",
        ));
    }

    Ok(cache)
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn io(err: impl Into<String>) -> Self {
        Self::new("Io", err)
    }

    pub fn parse(err: impl Into<String>) -> Self {
        Self::new("Parse", err)
    }
}

pub type CmdResult<T> = Result<T, CommandError>;

#[allow(dead_code)]
pub fn public_key() -> PublicKey {
    runtime::keyring::embedded_public_key()
}

pub async fn bootstrap(
    app: &AppHandle,
    pool: &SqlitePool,
) -> anyhow::Result<runtime::LicenseRuntime> {
    let state = LicenseState::default();
    let binding = runtime::DeviceBindingStore::load_or_init(app)
        .map_err(|err| anyhow::anyhow!(err.message.clone()))?;
    let runtime = runtime::LicenseRuntime::new(binding, runtime::default_keyring(), state.clone());
    runtime.reload_from_storage(pool).await?;
    Ok(runtime)
}

pub(crate) fn ensure_parent_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    ensure_parent_dir(path)?;
    let tmp_path = tmp_path(path);
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let mut file_name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "license".into());
    file_name.push(".tmp");
    tmp.set_file_name(file_name);
    tmp
}
