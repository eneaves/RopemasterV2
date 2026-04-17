//! SQLite persistence for licenses plus helper utilities to work with
//! filesystem snapshots/history.

use std::path::PathBuf;

use sqlx::{Row, SqlitePool};
use tauri::{AppHandle, Manager};

use crate::license::{write_atomic, CmdResult, CommandError, NormalizedLicense};

#[derive(Debug, Clone)]
pub struct StoredLicenseBlob {
    pub raw_bytes: Vec<u8>,
    pub installed_at: i64,
    pub last_verified_at: i64,
}

pub fn license_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|err| CommandError::io(err.to_string()))?
        .join("licenses");
    std::fs::create_dir_all(&dir).map_err(|err| CommandError::io(err.to_string()))?;
    Ok(dir)
}

pub fn requests_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    let path = license_dir(app)?.join("requests");
    std::fs::create_dir_all(&path).map_err(|err| CommandError::io(err.to_string()))?;
    Ok(path)
}

pub fn installed_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    let path = license_dir(app)?.join("installed");
    std::fs::create_dir_all(&path).map_err(|err| CommandError::io(err.to_string()))?;
    Ok(path)
}

pub fn current_license_path(app: &AppHandle) -> CmdResult<PathBuf> {
    Ok(installed_dir(app)?.join("current.lic"))
}

#[allow(dead_code)]
pub fn snapshot_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    let path = license_dir(app)?.join("snapshots");
    std::fs::create_dir_all(&path).map_err(|err| CommandError::io(err.to_string()))?;
    Ok(path)
}

pub fn persist_license_files(
    app: &AppHandle,
    bytes: &[u8],
    payload: &NormalizedLicense,
) -> CmdResult<()> {
    let current_path = current_license_path(app)?;
    write_atomic(&current_path, bytes).map_err(|err| CommandError::io(err.to_string()))?;

    let history_dir = installed_dir(app)?.join("history");
    std::fs::create_dir_all(&history_dir).map_err(|err| CommandError::io(err.to_string()))?;
    let history_name = format!(
        "{}-{}.lic",
        payload.issued_at,
        sanitize_filename(&payload.license_id)
    );
    let history_path = history_dir.join(history_name);
    write_atomic(&history_path, bytes).map_err(|err| CommandError::io(err.to_string()))
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
