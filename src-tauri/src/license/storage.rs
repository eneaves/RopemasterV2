//! SQLite persistence for licenses plus helper utilities to work with
//! filesystem snapshots/history.

use std::path::PathBuf;
use std::{fs, path::Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use tauri::{AppHandle, Manager};

use crate::license::{
    ensure_sensitive_dir, write_atomic_secure, CmdResult, CommandError, NormalizedLicense,
};

#[derive(Debug, Clone)]
pub struct StoredLicenseBlob {
    pub raw_bytes: Vec<u8>,
    pub installed_at: i64,
    pub last_verified_at: i64,
}

pub(crate) const LOCAL_LICENSE_INTEGRITY_PURPOSE: &[u8] = b"local-license-integrity";
const CURRENT_LICENSE_INTEGRITY_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CurrentLicenseIntegrityRecord {
    version: u32,
    sha256: String,
    size_bytes: u64,
    tag_sha256: String,
}

pub(crate) fn installed_dir_from_root(root: &Path) -> PathBuf {
    root.join("installed")
}

pub(crate) fn current_license_path_from_root(root: &Path) -> PathBuf {
    installed_dir_from_root(root).join("current.lic")
}

pub(crate) fn current_license_integrity_path_from_root(root: &Path) -> PathBuf {
    installed_dir_from_root(root).join("current.lic.integrity.json")
}

fn history_dir_from_root(root: &Path) -> PathBuf {
    installed_dir_from_root(root).join("history")
}

pub fn license_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|err| CommandError::io(err.to_string()))?
        .join("licenses");
    ensure_sensitive_dir(&dir).map_err(|err| CommandError::io(err.to_string()))?;
    Ok(dir)
}

pub fn requests_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    let path = license_dir(app)?.join("requests");
    ensure_sensitive_dir(&path).map_err(|err| CommandError::io(err.to_string()))?;
    Ok(path)
}

pub fn installed_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    let path = installed_dir_from_root(&license_dir(app)?);
    ensure_sensitive_dir(&path).map_err(|err| CommandError::io(err.to_string()))?;
    Ok(path)
}

pub fn current_license_path(app: &AppHandle) -> CmdResult<PathBuf> {
    Ok(current_license_path_from_root(&license_dir(app)?))
}

pub fn current_license_integrity_path(app: &AppHandle) -> CmdResult<PathBuf> {
    Ok(current_license_integrity_path_from_root(&license_dir(app)?))
}

fn validate_optional_sensitive_file(path: &Path) -> CmdResult<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => super::validate_sensitive_file(path).map_err(|err| CommandError::io(err.to_string())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(CommandError::io(err.to_string())),
    }
}

fn validate_history_dir(path: &Path) -> CmdResult<()> {
    if !path.exists() {
        return Ok(());
    }
    ensure_sensitive_dir(path).map_err(|err| CommandError::io(err.to_string()))?;
    for entry in fs::read_dir(path).map_err(|err| CommandError::io(err.to_string()))? {
        let entry = entry.map_err(|err| CommandError::io(err.to_string()))?;
        super::validate_sensitive_file(&entry.path())
            .map_err(|err| CommandError::io(err.to_string()))?;
    }
    Ok(())
}

pub fn validate_local_license_files(app: &AppHandle) -> CmdResult<()> {
    let installed = installed_dir(app)?;
    validate_optional_sensitive_file(&installed.join("current.lic"))?;
    validate_optional_sensitive_file(&installed.join("current.lic.integrity.json"))?;
    validate_history_dir(&installed.join("history"))?;
    Ok(())
}

#[cfg(test)]
fn validate_current_license_file(path: &Path) -> CmdResult<()> {
    validate_optional_sensitive_file(path)
}

#[cfg(test)]
fn read_current_license_file(path: &Path) -> CmdResult<Vec<u8>> {
    validate_current_license_file(path)?;
    fs::read(path).map_err(|err| CommandError::io(err.to_string()))
}

#[allow(dead_code)]
pub fn snapshot_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    let path = license_dir(app)?.join("snapshots");
    ensure_sensitive_dir(&path).map_err(|err| CommandError::io(err.to_string()))?;
    Ok(path)
}

pub fn persist_license_files(
    app: &AppHandle,
    bytes: &[u8],
    payload: &NormalizedLicense,
) -> CmdResult<()> {
    persist_license_files_under_root(&license_dir(app)?, bytes, payload)
}

pub(crate) fn persist_license_files_under_root(
    root: &Path,
    bytes: &[u8],
    payload: &NormalizedLicense,
) -> CmdResult<()> {
    persist_current_license_file_under_root(root, bytes)?;

    let history_dir = history_dir_from_root(root);
    ensure_sensitive_dir(&history_dir).map_err(|err| CommandError::io(err.to_string()))?;
    let history_name = format!(
        "{}-{}.lic",
        payload.issued_at,
        sanitize_filename(&payload.license_id)
    );
    let history_path = history_dir.join(history_name);
    write_atomic_secure(&history_path, bytes).map_err(|err| CommandError::io(err.to_string()))
}

pub(crate) fn persist_current_license_file_under_root(root: &Path, bytes: &[u8]) -> CmdResult<()> {
    let current_path = current_license_path_from_root(root);
    write_atomic_secure(&current_path, bytes).map_err(|err| CommandError::io(err.to_string()))
}

pub(crate) fn persist_current_license_integrity_under_root(
    root: &Path,
    bytes: &[u8],
    integrity_secret: &[u8],
) -> CmdResult<()> {
    let installed = installed_dir_from_root(root);
    ensure_sensitive_dir(&installed).map_err(|err| CommandError::io(err.to_string()))?;
    let path = current_license_integrity_path_from_root(root);
    let record = build_current_license_integrity_record(bytes, integrity_secret);
    let encoded =
        serde_json::to_vec(&record).map_err(|err| CommandError::parse(err.to_string()))?;
    write_atomic_secure(&path, &encoded).map_err(|err| CommandError::io(err.to_string()))?;
    validate_optional_sensitive_file(&path)?;
    Ok(())
}

pub fn persist_current_license_integrity(
    app: &AppHandle,
    bytes: &[u8],
    integrity_secret: &[u8],
) -> CmdResult<()> {
    persist_current_license_integrity_under_root(&license_dir(app)?, bytes, integrity_secret)
}

pub(crate) fn validate_current_license_integrity_under_root(
    root: &Path,
    expected_bytes: &[u8],
    integrity_secret: &[u8],
) -> CmdResult<()> {
    let current_path = current_license_path_from_root(root);
    let integrity_path = current_license_integrity_path_from_root(root);

    validate_required_sensitive_file(
        &current_path,
        "MissingCurrentLicenseFile",
        "Falta current.lic; reinstala la licencia.",
    )?;
    validate_required_sensitive_file(
        &integrity_path,
        "MissingCurrentLicenseIntegrity",
        "Falta la metadata de integridad local de la licencia; reinstala la licencia.",
    )?;

    let current_bytes = fs::read(&current_path).map_err(|err| CommandError::io(err.to_string()))?;
    let encoded =
        fs::read(&integrity_path).map_err(|err| CommandError::io(err.to_string()))?;
    let stored: CurrentLicenseIntegrityRecord = serde_json::from_slice(&encoded).map_err(|err| {
        CommandError::new(
            "LocalLicenseTampered",
            format!(
                "La metadata de integridad de {} es inválida: {err}",
                integrity_path.display()
            ),
        )
    })?;
    let expected_record = build_current_license_integrity_record(&current_bytes, integrity_secret);

    if stored.version != CURRENT_LICENSE_INTEGRITY_VERSION
        || stored.sha256 != expected_record.sha256
        || stored.size_bytes != expected_record.size_bytes
        || stored.tag_sha256 != expected_record.tag_sha256
    {
        return Err(CommandError::new(
            "LocalLicenseTampered",
            "current.lic no coincide con su metadata de integridad local.",
        ));
    }

    if current_bytes != expected_bytes {
        return Err(CommandError::new(
            "LocalLicenseStateMismatch",
            "current.lic no coincide con la licencia almacenada en SQLite.",
        ));
    }

    Ok(())
}

pub async fn load_blob(pool: &SqlitePool) -> Result<Option<StoredLicenseBlob>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT raw_bytes, installed_at, last_verified_at
        FROM license
        WHERE id = 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| StoredLicenseBlob {
        raw_bytes: row.get::<Vec<u8>, _>("raw_bytes"),
        installed_at: row.get::<i64, _>("installed_at"),
        last_verified_at: row.get::<i64, _>("last_verified_at"),
    }))
}

pub async fn upsert_blob(
    pool: &SqlitePool,
    raw_bytes: &[u8],
    timestamp: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO license (id, raw_bytes, installed_at, last_verified_at)
        VALUES (1, ?1, ?2, ?3)
        ON CONFLICT(id)
        DO UPDATE SET
            raw_bytes = excluded.raw_bytes,
            installed_at = excluded.installed_at,
            last_verified_at = excluded.last_verified_at
        "#,
    )
    .bind(raw_bytes)
    .bind(timestamp)
    .bind(timestamp)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update_last_verified(pool: &SqlitePool, timestamp: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE license
        SET last_verified_at = ?1
        WHERE id = 1
        "#,
    )
    .bind(timestamp)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_blob(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM license WHERE id = 1")
        .execute(pool)
        .await?;
    Ok(())
}

fn sanitize_filename(input: &str) -> String {
    input
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn validate_required_sensitive_file(
    path: &Path,
    missing_code: &str,
    missing_message: &str,
) -> CmdResult<()> {
    super::validate_sensitive_file(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            CommandError::new(missing_code, missing_message)
        } else {
            CommandError::io(err.to_string())
        }
    })
}

fn build_current_license_integrity_record(
    bytes: &[u8],
    integrity_secret: &[u8],
) -> CurrentLicenseIntegrityRecord {
    let sha256 = sha256_hex(bytes);
    let size_bytes = bytes.len() as u64;
    CurrentLicenseIntegrityRecord {
        version: CURRENT_LICENSE_INTEGRITY_VERSION,
        sha256: sha256.clone(),
        size_bytes,
        tag_sha256: integrity_tag_sha256(integrity_secret, &sha256, size_bytes),
    }
}

fn integrity_tag_sha256(integrity_secret: &[u8], sha256: &str, size_bytes: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"roping-manager-local-license-integrity-v1");
    hasher.update(integrity_secret);
    hasher.update(sha256.as_bytes());
    hasher.update(size_bytes.to_le_bytes());
    hex::encode(hasher.finalize())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{
        current_license_integrity_path_from_root, current_license_path_from_root,
        persist_current_license_integrity_under_root, read_current_license_file,
        validate_current_license_file, validate_current_license_integrity_under_root,
    };
    use crate::license::{ensure_sensitive_dir, write_atomic_secure};
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    #[cfg(unix)]
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("current-license-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn integrity_secret() -> Vec<u8> {
        b"local-license-test-secret".to_vec()
    }

    #[cfg(unix)]
    #[test]
    fn current_license_file_is_created_with_restrictive_permissions() {
        let root = temp_dir();
        let installed = root.join("installed");
        ensure_sensitive_dir(&installed).unwrap();
        let current = installed.join("current.lic");

        write_atomic_secure(&current, b"LICGEN-current").unwrap();

        let file_mode = fs::metadata(&current).unwrap().permissions().mode() & 0o777;
        let dir_mode = fs::metadata(&installed).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn current_license_file_repairs_insecure_permissions_on_validation() {
        let root = temp_dir();
        let installed = root.join("installed");
        ensure_sensitive_dir(&installed).unwrap();
        let current = installed.join("current.lic");
        write_atomic_secure(&current, b"LICGEN-current").unwrap();
        fs::set_permissions(&current, fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(&installed, fs::Permissions::from_mode(0o755)).unwrap();

        validate_current_license_file(&current).unwrap();
        ensure_sensitive_dir(&installed).unwrap();

        assert_eq!(fs::metadata(&current).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(&installed).unwrap().permissions().mode() & 0o777, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn current_license_file_symlink_is_rejected() {
        let root = temp_dir();
        let installed = root.join("installed");
        ensure_sensitive_dir(&installed).unwrap();
        let target = root.join("target.lic");
        fs::write(&target, b"payload").unwrap();
        let current = installed.join("current.lic");
        symlink(&target, &current).unwrap();

        let err = validate_current_license_file(&current).unwrap_err();
        assert_eq!(err.code, "Io");
    }

    #[test]
    fn current_license_file_reads_normally_after_validation() {
        let root = temp_dir();
        let installed = root.join("installed");
        ensure_sensitive_dir(&installed).unwrap();
        let current = installed.join("current.lic");
        let bytes = b"LICGEN-current".to_vec();
        write_atomic_secure(&current, &bytes).unwrap();

        let read_back = read_current_license_file(&current).unwrap();
        assert_eq!(read_back, bytes);
    }

    #[cfg(unix)]
    #[test]
    fn current_license_integrity_file_is_created_with_restrictive_permissions() {
        let root = temp_dir();
        let installed = root.join("installed");
        ensure_sensitive_dir(&installed).unwrap();
        let current = current_license_path_from_root(&root);
        let integrity = current_license_integrity_path_from_root(&root);
        let bytes = b"LICGEN-current".to_vec();
        write_atomic_secure(&current, &bytes).unwrap();

        persist_current_license_integrity_under_root(&root, &bytes, &integrity_secret()).unwrap();

        let file_mode = fs::metadata(&integrity).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
    }

    #[test]
    fn current_license_integrity_validation_accepts_matching_state() {
        let root = temp_dir();
        let installed = root.join("installed");
        ensure_sensitive_dir(&installed).unwrap();
        let current = current_license_path_from_root(&root);
        let bytes = b"LICGEN-current".to_vec();
        write_atomic_secure(&current, &bytes).unwrap();
        let secret = integrity_secret();
        persist_current_license_integrity_under_root(&root, &bytes, &secret).unwrap();

        validate_current_license_integrity_under_root(&root, &bytes, &secret).unwrap();
    }

    #[test]
    fn current_license_integrity_detects_modified_current_file() {
        let root = temp_dir();
        let installed = root.join("installed");
        ensure_sensitive_dir(&installed).unwrap();
        let current = current_license_path_from_root(&root);
        let bytes = b"LICGEN-current".to_vec();
        write_atomic_secure(&current, &bytes).unwrap();
        let secret = integrity_secret();
        persist_current_license_integrity_under_root(&root, &bytes, &secret).unwrap();
        write_atomic_secure(&current, b"LICGEN-tampered").unwrap();

        let err = validate_current_license_integrity_under_root(&root, &bytes, &secret)
            .unwrap_err();
        assert_eq!(err.code, "LocalLicenseTampered");
    }

    #[test]
    fn current_license_integrity_detects_sqlite_mismatch() {
        let root = temp_dir();
        let installed = root.join("installed");
        ensure_sensitive_dir(&installed).unwrap();
        let current = current_license_path_from_root(&root);
        let bytes = b"LICGEN-current".to_vec();
        let other = b"LICGEN-other".to_vec();
        write_atomic_secure(&current, &bytes).unwrap();
        let secret = integrity_secret();
        persist_current_license_integrity_under_root(&root, &bytes, &secret).unwrap();

        let err = validate_current_license_integrity_under_root(&root, &other, &secret)
            .unwrap_err();
        assert_eq!(err.code, "LocalLicenseStateMismatch");
    }

    #[test]
    fn current_license_integrity_detects_truncated_current_file() {
        let root = temp_dir();
        let installed = root.join("installed");
        ensure_sensitive_dir(&installed).unwrap();
        let current = current_license_path_from_root(&root);
        let bytes = b"LICGEN-current".to_vec();
        write_atomic_secure(&current, &bytes).unwrap();
        let secret = integrity_secret();
        persist_current_license_integrity_under_root(&root, &bytes, &secret).unwrap();
        write_atomic_secure(&current, b"LIC").unwrap();

        let err = validate_current_license_integrity_under_root(&root, &bytes, &secret)
            .unwrap_err();
        assert_eq!(err.code, "LocalLicenseTampered");
    }
}
