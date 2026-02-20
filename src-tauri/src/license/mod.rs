use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use license_core::LicensePayload;
use once_cell::sync::Lazy;
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::AppHandle;
use time::OffsetDateTime;

pub mod commands;
mod device;
mod storage;
mod validator;

use ed25519_dalek::PublicKey;

#[derive(Clone)]
pub struct LicenseState {
    cache: Arc<RwLock<Option<LicenseCache>>>,
}

#[derive(Clone)]
pub struct LicenseCache {
    pub payload: LicensePayload,
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
    let cache = state
        .snapshot()
        .ok_or_else(|| CommandError::new("LicenseRequired", "Instala una licencia válida para continuar."))?;

    let payload = &cache.payload;
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let not_before = payload.not_before as i64;
    let not_after = payload.not_after as i64;
    let skew = payload.max_clock_skew as i64;

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

static LICENSE_PUBLIC_KEY: Lazy<RwLock<PublicKey>> = Lazy::new(|| {
    let bytes = include_bytes!("public_key_dev.der");
    let key = PublicKey::from_bytes(bytes).expect("invalid embedded license public key");
    RwLock::new(key)
});

pub fn public_key() -> PublicKey {
    LICENSE_PUBLIC_KEY
        .read()
        .expect("public key lock poisoned")
        .clone()
}

pub async fn bootstrap(
    app: &AppHandle,
    pool: &SqlitePool,
    state: &LicenseState,
) -> anyhow::Result<()> {
    if let Some(record) = storage::load_blob(pool).await? {
        let device_hash = device::get_or_init_device_hash(app)
            .map_err(|err| anyhow::anyhow!(err.message.clone()))?;
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let key = public_key();
        match validator::evaluate_license(&key, &record.raw_bytes, &device_hash, now) {
            Ok(evaluation) => {
                if evaluation.status != validator::LicenseRuntimeStatus::Active {
                    tracing::info!("License bootstrap classification: {:?}", evaluation.status);
                }
                state.replace(Some(LicenseCache {
                    payload: evaluation.payload,
                    installed_at: record.installed_at,
                    last_verified_at: now,
                }));
                if record.last_verified_at != now {
                    storage::update_last_verified(pool, now).await?;
                }
            }
            Err(err) => {
                tracing::warn!(
                    "Failed to bootstrap license: {} ({})",
                    err.message,
                    err.code
                );
                storage::delete_blob(pool).await?;
                state.replace(None);
            }
        }
    } else {
        state.replace(None);
    }
    Ok(())
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
