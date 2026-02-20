//! SQLite persistence for licenses. Only the raw_bytes blob and timestamps are stored.
//! Metadata (plan, expiration, etc.) is always reconstructed by re-verifying the blob.

use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone)]
pub struct StoredLicenseBlob {
    pub raw_bytes: Vec<u8>,
    pub installed_at: i64,
    pub last_verified_at: i64,
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
