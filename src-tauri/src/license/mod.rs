use std::{
    fs,
    fs::OpenOptions,
    io,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::AppHandle;
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

pub mod commands;
mod model;
pub(crate) mod modern;
#[cfg(test)]
mod phase2_adversarial_tests;
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
    pub raw_bytes: Vec<u8>,
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

#[cfg(test)]
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
    let verification_environment =
        runtime::service::verification_environment_for_keyring_env(runtime::keyring::KEYRING_ENV)
            .map_err(|err| anyhow::anyhow!(err.message.clone()))?;
    let runtime = runtime::LicenseRuntime::new(
        binding,
        runtime::default_keyring(),
        state.clone(),
        storage::license_dir(app).map_err(|err| anyhow::anyhow!(err.message.clone()))?,
        verification_environment,
    );
    storage::validate_local_license_files(app)
        .map_err(|err| anyhow::anyhow!(err.message.clone()))?;
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
    {
        let mut file = create_generic_tmp_file(&tmp_path)?;
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp_path, path)?;
    Ok(())
}

pub(crate) fn write_atomic_secure(path: &Path, bytes: &[u8]) -> io::Result<()> {
    ensure_secure_parent_dir(path)?;
    validate_sensitive_target(path)?;
    let tmp_path = tmp_path(path);
    {
        let mut file = create_sensitive_tmp_file(&tmp_path)?;
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    #[cfg(unix)]
    fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600))?;
    fs::rename(&tmp_path, path)?;
    set_sensitive_file_permissions(path)?;
    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let mut file_name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "license".into());
    file_name.push(format!(".tmp-{}-{}", std::process::id(), Uuid::new_v4()));
    tmp.set_file_name(file_name);
    tmp
}

pub(crate) fn ensure_sensitive_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} debe ser un directorio regular", path.display()),
        ));
    }
    #[cfg(unix)]
    if meta.mode() & 0o777 != 0o700 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn ensure_secure_parent_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_sensitive_dir(parent)?;
    }
    Ok(())
}

pub(crate) fn validate_sensitive_file(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} debe ser un archivo regular", path.display()),
        ));
    }
    set_sensitive_file_permissions(path)
}

fn validate_sensitive_target(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => validate_sensitive_file(path),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn create_sensitive_tmp_file(path: &Path) -> io::Result<fs::File> {
    #[cfg(unix)]
    {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new().write(true).create_new(true).open(path)
    }
}

fn create_generic_tmp_file(path: &Path) -> io::Result<fs::File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

fn set_sensitive_file_permissions(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        let mode = fs::metadata(path)?.mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "{} mantiene permisos inseguros {:o}; se esperaba 600",
                    path.display(),
                    mode
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod permission_tests {
    use super::{ensure_sensitive_dir, write_atomic_secure};
    use std::fs;
    use std::path::PathBuf;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("license-perms-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_secure_sets_restrictive_permissions() {
        let dir = temp_dir();
        let path = dir.join("installed").join("current.lic");
        write_atomic_secure(&path, b"license-bytes").unwrap();

        let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let dir_mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_sensitive_dir_repairs_insecure_permissions() {
        let dir = temp_dir();
        let sensitive = dir.join("licenses");
        fs::create_dir_all(&sensitive).unwrap();
        fs::set_permissions(&sensitive, fs::Permissions::from_mode(0o755)).unwrap();

        ensure_sensitive_dir(&sensitive).unwrap();

        let dir_mode = fs::metadata(&sensitive).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
    }
}
