use anyhow::Result;
use calamine::{open_workbook_auto, Data, Reader, Sheets};
use chrono::Utc;
use rand::seq::SliceRandom;
use rand::thread_rng;
use rust_xlsxwriter::*;
use serde_json;
use sqlx::QueryBuilder;
use sqlx::Row;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    FromRow, Sqlite, SqlitePool, Transaction,
};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use tauri::{Emitter, Manager, State};

mod timer_capture;
use once_cell::sync::Lazy;
use std::sync::{Arc, Mutex};
use timer_capture::PolarisTimerCapture;

mod license;

/* ------------------- STATE ------------------- */
#[derive(Clone)]
struct Db(SqlitePool, license::runtime::LicenseRuntime);

impl Db {
    fn require_license(&self) -> Result<(), String> {
        self.1
            .ensure_active()
            .map(|_| ())
            .map_err(|err| err.message)
    }
}

// Global timer capture instance
static TIMER_CAPTURE: Lazy<Arc<Mutex<PolarisTimerCapture>>> =
    Lazy::new(|| Arc::new(Mutex::new(PolarisTimerCapture::new())));

/* ------------------- HELPERS ------------------- */
async fn ensure_event_unlocked(pool: &SqlitePool, event_id: i64) -> Result<(), String> {
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM event WHERE id = ?1 AND is_deleted = 0")
            .bind(event_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;

    match status.as_deref() {
        Some("locked") => Err("El evento está bloqueado; no se permiten cambios.".into()),
        Some(_) => Ok(()),
        None => Err("Evento no encontrado.".into()),
    }
}

async fn log_audit(
    pool: &SqlitePool,
    action: &str,
    entity_type: &str,
    entity_id: Option<i64>,
    metadata: Option<String>,
) -> Result<(), String> {
    // We ignore errors here to not block the main operation, but we log them
    let res = sqlx::query(
        r#"
        INSERT INTO audit_log (action, entity_type, entity_id, metadata, created_at)
        VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
        "#,
    )
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(metadata)
    .execute(pool)
    .await;

    if let Err(e) = res {
        tracing::error!("Failed to write audit log: {}", e);
    }
    Ok(())
}

async fn load_round_order(
    pool: &SqlitePool,
    event_id: i64,
    round: i64,
) -> Result<Vec<i64>, String> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT team_id
        FROM draw
        WHERE event_id = ?1 AND round = ?2
        ORDER BY position ASC
        "#,
    )
    .bind(event_id)
    .bind(round)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

async fn reconcile_future_runs(pool: &SqlitePool, event_id: i64) -> Result<(), String> {
    // Mark future seeded runs as skipped when the same team was already eliminated
    // in a previous completed round via NT or DQ.
    sqlx::query(
        r#"
        UPDATE run
        SET
          status = 'skipped',
          updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
        WHERE event_id = ?1
          AND status != 'completed'
          AND EXISTS (
            SELECT 1
            FROM run prior
            WHERE prior.event_id = run.event_id
              AND prior.team_id = run.team_id
              AND prior.round < run.round
              AND prior.status = 'completed'
              AND (prior.no_time = 1 OR prior.dq = 1)
          )
        "#,
    )
    .bind(event_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    // If a previous NT/DQ is corrected to a valid time, restore future seeded runs
    // so they can be captured again.
    sqlx::query(
        r#"
        UPDATE run
        SET
          status = 'pending',
          updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
        WHERE event_id = ?1
          AND status = 'skipped'
          AND NOT EXISTS (
            SELECT 1
            FROM run prior
            WHERE prior.event_id = run.event_id
              AND prior.team_id = run.team_id
              AND prior.round < run.round
              AND prior.status = 'completed'
              AND (prior.no_time = 1 OR prior.dq = 1)
          )
        "#,
    )
    .bind(event_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

async fn reconcile_all_future_runs(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE run
        SET
          status = 'skipped',
          updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
        WHERE status != 'completed'
          AND EXISTS (
            SELECT 1
            FROM run prior
            WHERE prior.event_id = run.event_id
              AND prior.team_id = run.team_id
              AND prior.round < run.round
              AND prior.status = 'completed'
              AND (prior.no_time = 1 OR prior.dq = 1)
          )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE run
        SET
          status = 'pending',
          updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
        WHERE status = 'skipped'
          AND NOT EXISTS (
            SELECT 1
            FROM run prior
            WHERE prior.event_id = run.event_id
              AND prior.team_id = run.team_id
              AND prior.round < run.round
              AND prior.status = 'completed'
              AND (prior.no_time = 1 OR prior.dq = 1)
          )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

/* ------------------- HEALTH ------------------- */
#[tauri::command]
async fn health_check(db: State<'_, Db>) -> Result<String, String> {
    db.require_license()?;
    sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&db.0)
        .await
        .map(|_| "ok".to_string())
        .map_err(|e| e.to_string())
}

/* ------------------- SERIES ------------------- */
#[derive(serde::Deserialize)]
struct NewSeries {
    name: String,
    season: String,
    status: String, // 'active' | 'upcoming' | 'archived'
    start_date: Option<String>,
    end_date: Option<String>,
}

#[derive(serde::Serialize, FromRow)]
struct SeriesRow {
    id: i64,
    name: String,
    season: String,
    status: String,
    start_date: Option<String>,
    end_date: Option<String>,
    created_at: String,
    updated_at: String,
    events_count: i64,
    progress: f64,
}

#[tauri::command]
async fn list_series(db: State<'_, Db>) -> Result<Vec<SeriesRow>, String> {
    db.require_license()?;
    reconcile_all_future_runs(&db.0).await?;
    sqlx::query_as::<_, SeriesRow>(
        r#"
        SELECT 
            s.id, s.name, s.season, s.status,
            s.start_date, s.end_date, s.created_at, s.updated_at,
            (SELECT COUNT(*) FROM event e WHERE e.series_id = s.id AND e.is_deleted = 0) as events_count,
            COALESCE(
                (
                    SELECT 
                        CASE WHEN COUNT(r.id) = 0 THEN 0.0
                        ELSE CAST(SUM(CASE WHEN r.status IN ('completed', 'skipped') THEN 1 ELSE 0 END) AS REAL) / COUNT(r.id) * 100.0
                        END
                    FROM run r
                    JOIN event e ON r.event_id = e.id
                    WHERE e.series_id = s.id AND e.is_deleted = 0
                ), 
                0.0
            ) as progress
        FROM series s
        WHERE s.is_deleted = 0
        ORDER BY s.created_at DESC
        "#,
    )
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_series(db: State<'_, Db>, payload: NewSeries) -> Result<i64, String> {
    db.require_license()?;
    let res = sqlx::query(
        r#"
        INSERT INTO series (name, season, status, start_date, end_date)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
    )
    .bind(&payload.name)
    .bind(&payload.season)
    .bind(&payload.status)
    .bind(&payload.start_date)
    .bind(&payload.end_date)
    .execute(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    let id = res.last_insert_rowid();
    log_audit(
        &db.0,
        "create_series",
        "series",
        Some(id),
        Some(payload.name),
    )
    .await?;
    Ok(id)
}

#[derive(serde::Deserialize)]
struct UpdateSeries {
    name: Option<String>,
    season: Option<String>,
    status: Option<String>, // 'active' | 'upcoming' | 'archived'
    start_date: Option<Option<String>>,
    end_date: Option<Option<String>>,
}

#[tauri::command]
async fn update_series(db: State<'_, Db>, id: i64, patch: UpdateSeries) -> Result<(), String> {
    db.require_license()?;
    // verify series exists
    let exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM series WHERE id = ?1 AND is_deleted = 0")
            .bind(id)
            .fetch_optional(&db.0)
            .await
            .map_err(|e| e.to_string())?;
    let Some(_exists) = exists else {
        return Err("Serie no encontrada.".into());
    };

    // build update within transaction
    let mut tx: Transaction<'_, Sqlite> = db.0.begin().await.map_err(|e| e.to_string())?;

    if let Some(name) = patch.name {
        sqlx::query("UPDATE series SET name = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2")
            .bind(name)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    if let Some(season) = patch.season {
        sqlx::query("UPDATE series SET season = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2")
            .bind(season)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    if let Some(status) = patch.status {
        if status != "active" && status != "upcoming" && status != "archived" {
            return Err("Status inválido: usa 'active', 'upcoming' o 'archived'.".into());
        }
        sqlx::query("UPDATE series SET status = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2")
            .bind(status)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    // start_date/end_date are Option<Option<String>> to allow explicit null
    if let Some(start_opt) = patch.start_date {
        sqlx::query("UPDATE series SET start_date = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2")
            .bind(start_opt)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    if let Some(end_opt) = patch.end_date {
        sqlx::query("UPDATE series SET end_date = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2")
            .bind(end_opt)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())?;
    log_audit(&db.0, "update_series", "series", Some(id), None).await?;
    Ok(())
}

/* ------------------- EVENTS ------------------- */
#[derive(serde::Deserialize)]
struct NewEvent {
    series_id: i64,
    name: String,
    date: String,
    rounds: i64,
    status: Option<String>,
    location: Option<String>,
    entry_fee: Option<f64>,
    prize_pool: Option<f64>,
    max_team_rating: Option<f64>,
    payoff_allocation: Option<String>,
    admin_pin: Option<String>,
}

#[derive(serde::Serialize, FromRow)]
struct EventRow {
    id: i64,
    series_id: i64,
    name: String,
    date: String,
    status: Option<String>,
    rounds: i64,
    location: Option<String>,
    entry_fee: Option<f64>,
    prize_pool: Option<f64>,
    max_team_rating: Option<f64>,
    created_at: String,
    updated_at: String,
    payoff_allocation: Option<String>,
    admin_pin: Option<String>,
    teams_count: i64,
    pot: f64,
}

#[tauri::command]
async fn list_events(db: State<'_, Db>, series_id: Option<i64>) -> Result<Vec<EventRow>, String> {
    db.require_license()?;
    if let Some(sid) = series_id {
        sqlx::query_as::<_, EventRow>(
            r#"
         SELECT 
             e.id, e.series_id, e.name, e.date, e.status, e.rounds, e.location,
             e.entry_fee, e.prize_pool, e.max_team_rating, e.created_at, e.updated_at,
             e.payoff_allocation, e.admin_pin,
             (SELECT COUNT(*) FROM team t WHERE t.event_id = e.id AND t.status = 'active') as teams_count,
             (
                COALESCE(e.prize_pool, 0.0) + 
                (COALESCE(e.entry_fee, 0.0) * (
                    SELECT COUNT(DISTINCT roper_id) FROM (
                        SELECT header_id AS roper_id FROM team WHERE event_id = e.id AND status = 'active'
                        UNION
                        SELECT heeler_id AS roper_id FROM team WHERE event_id = e.id AND status = 'active'
                    )
                ))
             ) as pot
            FROM event e
            WHERE e.is_deleted = 0 AND e.series_id = ?1
            ORDER BY e.date ASC, e.id ASC
            "#,
        )
        .bind(sid)
        .fetch_all(&db.0)
        .await
        .map_err(|e| e.to_string())
    } else {
        sqlx::query_as::<_, EventRow>(
            r#"
         SELECT 
             e.id, e.series_id, e.name, e.date, e.status, e.rounds, e.location,
             e.entry_fee, e.prize_pool, e.max_team_rating, e.created_at, e.updated_at,
             e.payoff_allocation, e.admin_pin,
             (SELECT COUNT(*) FROM team t WHERE t.event_id = e.id AND t.status = 'active') as teams_count,
             (
                COALESCE(e.prize_pool, 0.0) + 
                (COALESCE(e.entry_fee, 0.0) * (
                    SELECT COUNT(DISTINCT roper_id) FROM (
                        SELECT header_id AS roper_id FROM team WHERE event_id = e.id AND status = 'active'
                        UNION
                        SELECT heeler_id AS roper_id FROM team WHERE event_id = e.id AND status = 'active'
                    )
                ))
             ) as pot
            FROM event e
            WHERE e.is_deleted = 0
            ORDER BY e.date ASC, e.id ASC
            "#,
        )
        .fetch_all(&db.0)
        .await
        .map_err(|e| e.to_string())
    }
}

#[tauri::command]
async fn create_event(db: State<'_, Db>, payload: NewEvent) -> Result<i64, String> {
    db.require_license()?;
    // Normalize status values coming from the frontend. DB CHECK allows
    // only ('active','upcoming','completed','locked'). Map common FE values
    // to the canonical set to avoid constraint errors (e.g. 'draft' -> 'upcoming').
    let raw_status = payload.status.unwrap_or_else(|| "upcoming".to_string());
    let status = match raw_status.as_str() {
        "draft" => "upcoming".to_string(),
        "finalized" => "completed".to_string(),
        "active" | "upcoming" | "completed" | "locked" => raw_status.clone(),
        _ => "upcoming".to_string(),
    };

    let res = sqlx::query(
        r#"
        INSERT INTO event (series_id, name, date, status, rounds, location, entry_fee, prize_pool, max_team_rating, payoff_allocation, admin_pin)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        "#
    )
    .bind(payload.series_id)
    .bind(&payload.name)
    .bind(&payload.date)
    .bind(&status)
    .bind(payload.rounds)
    .bind(&payload.location)
    .bind(payload.entry_fee)
    .bind(payload.prize_pool)
    .bind(payload.max_team_rating)
    .bind(&payload.payoff_allocation)
    .bind(&payload.admin_pin)
    .execute(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    let id = res.last_insert_rowid();
    log_audit(&db.0, "create_event", "event", Some(id), Some(payload.name)).await?;
    Ok(id)
}

#[tauri::command]
async fn update_event_status(db: State<'_, Db>, id: i64, status: String) -> Result<(), String> {
    db.require_license()?;
    let normalized_status = match status.as_str() {
        "draft" => "upcoming".to_string(),
        "finalized" => "completed".to_string(),
        "active" | "upcoming" | "completed" | "locked" => status,
        _ => "upcoming".to_string(),
    };

    sqlx::query("UPDATE event SET status = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2")
        .bind(&normalized_status)
        .bind(id)
        .execute(&db.0)
        .await
        .map_err(|e| e.to_string())?;

    log_audit(
        &db.0,
        "update_event_status",
        "event",
        Some(id),
        Some(normalized_status),
    )
    .await?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct EventPatch {
    name: Option<String>,
    date: Option<String>,
    rounds: Option<i64>,
    status: Option<String>,
    entry_fee: Option<f64>,
    prize_pool: Option<f64>,
    location: Option<String>,
    max_team_rating: Option<f64>,
    payoff_allocation: Option<String>,
    admin_pin: Option<String>,
}

#[tauri::command]
async fn update_event(db: State<'_, Db>, id: i64, patch: EventPatch) -> Result<(), String> {
    db.require_license()?;
    let pool = &db.0;

    // comprobar existencia
    let exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM event WHERE id = ?1 AND is_deleted = 0")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    let Some(_exists) = exists else {
        return Err("Evento no encontrado.".into());
    };

    // impedir cambios si está locked
    // ensure_event_unlocked(pool, id).await?;

    // construir UPDATE dinámico usando QueryBuilder
    let mut builder = QueryBuilder::<Sqlite>::new("UPDATE event SET ");
    let mut has_any = false;

    if let Some(name) = patch.name {
        builder.push("name = ").push_bind(name).push(", ");
        has_any = true;
    }
    if let Some(date) = patch.date {
        builder.push("date = ").push_bind(date).push(", ");
        has_any = true;
    }
    if let Some(rounds) = patch.rounds {
        builder.push("rounds = ").push_bind(rounds).push(", ");
        has_any = true;
    }
    if let Some(raw_status) = patch.status {
        let status = match raw_status.as_str() {
            "draft" => "upcoming".to_string(),
            "finalized" => "completed".to_string(),
            "active" | "upcoming" | "completed" | "locked" => raw_status,
            _ => "upcoming".to_string(),
        };
        builder.push("status = ").push_bind(status).push(", ");
        has_any = true;
    }
    if let Some(entry) = patch.entry_fee {
        builder.push("entry_fee = ").push_bind(entry).push(", ");
        has_any = true;
    }
    if let Some(prize) = patch.prize_pool {
        builder.push("prize_pool = ").push_bind(prize).push(", ");
        has_any = true;
    }
    if let Some(loc) = patch.location {
        builder.push("location = ").push_bind(loc).push(", ");
        has_any = true;
    }
    if let Some(mtr) = patch.max_team_rating {
        builder.push("max_team_rating = ").push_bind(mtr).push(", ");
        has_any = true;
    }
    if let Some(pa) = patch.payoff_allocation {
        builder
            .push("payoff_allocation = ")
            .push_bind(pa)
            .push(", ");
        has_any = true;
    }
    if let Some(pin) = patch.admin_pin {
        builder.push("admin_pin = ").push_bind(pin).push(", ");
        has_any = true;
    }

    if !has_any {
        return Ok(());
    }

    builder
        .push("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ")
        .push_bind(id);

    builder
        .build()
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    log_audit(pool, "update_event", "event", Some(id), None).await?;
    Ok(())
}

#[tauri::command]
async fn delete_event(db: State<'_, Db>, id: i64) -> Result<(), String> {
    db.require_license()?;
    let pool = &db.0;
    // Verificar existencia y estado
    let row_opt = sqlx::query("SELECT status, name FROM event WHERE id = ?1 AND is_deleted = 0")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;

    let Some(row) = row_opt else {
        return Err("Evento no encontrado.".into());
    };
    let _status: String = row.try_get("status").map_err(|e| e.to_string())?;
    let current_name: String = row.try_get("name").map_err(|e| e.to_string())?;
    let deleted_suffix = Utc::now().format("%Y%m%d%H%M%S").to_string();
    let archived_name = format!("{}__deleted__{}", current_name, deleted_suffix);

    // if status == "locked" {
    //     return Err("El evento está bloqueado; no se puede eliminar.".into());
    // }

    // Soft-delete: marcar is_deleted = 1. No cambiamos status a 'archived' porque el CHECK constraint no lo permite.
    let res = sqlx::query("UPDATE event SET is_deleted = 1, name = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2")
        .bind(archived_name)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    if res.rows_affected() == 1 {
        log_audit(pool, "delete_event", "event", Some(id), None).await?;
        Ok(())
    } else {
        Err("Evento no encontrado.".into())
    }
}

#[tauri::command]
async fn duplicate_event(db: State<'_, Db>, id: i64) -> Result<i64, String> {
    db.require_license()?;
    let pool = &db.0;

    let row = sqlx::query(
        r#"SELECT series_id, name, date, status, rounds, entry_fee, prize_pool, location, max_team_rating, payoff_allocation
           FROM event WHERE id = ?1"#,
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    let series_id: i64 = row.try_get("series_id").map_err(|e| e.to_string())?;
    let name_opt: Option<String> = row.try_get("name").ok();
    let date_opt: Option<String> = row.try_get("date").ok();
    let status_opt: Option<String> = row.try_get("status").ok();
    let rounds_opt: Option<i64> = row.try_get("rounds").ok();
    let entry_fee_opt: Option<f64> = row.try_get("entry_fee").ok();
    let prize_pool_opt: Option<f64> = row.try_get("prize_pool").ok();
    let location_opt: Option<String> = row.try_get("location").ok();
    let max_team_rating_opt: Option<f64> = row.try_get("max_team_rating").ok();
    let payoff_allocation_opt: Option<String> = row.try_get("payoff_allocation").ok();

    // bloquear duplicado si está locked
    if let Some(st) = status_opt.as_ref() {
        if st == "locked" {
            return Err("Evento bloqueado; no se puede duplicar.".into());
        }
    }

    let base_name = name_opt.unwrap_or_default();
    let new_name = format!("{} (Copy)", base_name);

    let res = sqlx::query(
        r#"INSERT INTO event (series_id, name, date, status, rounds, entry_fee, prize_pool, location, max_team_rating, payoff_allocation, created_at, updated_at)
           VALUES (?1, ?2, ?3, 'upcoming', ?4, ?5, ?6, ?7, ?8, ?9, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))"#)
        .bind(series_id)
        .bind(new_name)
        .bind(date_opt)
        .bind(rounds_opt)
        .bind(entry_fee_opt)
        .bind(prize_pool_opt)
        .bind(location_opt)
        .bind(max_team_rating_opt)
        .bind(payoff_allocation_opt)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    let new_id = res.last_insert_rowid();
    log_audit(
        pool,
        "duplicate_event",
        "event",
        Some(new_id),
        Some(format!("Copied from {}", id)),
    )
    .await?;
    Ok(new_id)
}

#[tauri::command]
async fn lock_event(db: State<'_, Db>, event_id: i64) -> Result<(), String> {
    db.require_license()?;
    sqlx::query(
        "UPDATE event SET status = 'locked', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?1"
    )
    .bind(event_id)
    .execute(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    log_audit(&db.0, "lock_event", "event", Some(event_id), None).await?;
    Ok(())
}

/* ------------------- PAYOFF RULES ------------------- */

#[derive(serde::Serialize, sqlx::FromRow)]
struct PayoffRuleRow {
    id: i64,
    event_id: i64,
    position: i64,
    percentage: f64,
    is_active: i64,
    created_at: String,
}

#[tauri::command]
async fn list_payoff_rules(
    db: State<'_, Db>,
    event_id: Option<i64>,
) -> Result<Vec<PayoffRuleRow>, String> {
    db.require_license()?;
    if let Some(eid) = event_id {
        sqlx::query_as::<_, PayoffRuleRow>(
            r#"
            SELECT id, event_id, position, percentage, is_active, created_at
            FROM payoff_rule
            WHERE event_id = ?1 AND is_active = 1
            ORDER BY position ASC
            "#,
        )
        .bind(eid)
        .fetch_all(&db.0)
        .await
        .map_err(|e| e.to_string())
    } else {
        sqlx::query_as::<_, PayoffRuleRow>(
            r#"
            SELECT id, event_id, position, percentage, is_active, created_at
            FROM payoff_rule
            WHERE is_active = 1
            ORDER BY event_id ASC, position ASC
            "#,
        )
        .fetch_all(&db.0)
        .await
        .map_err(|e| e.to_string())
    }
}

#[tauri::command]
async fn delete_payoff_rule(db: State<'_, Db>, id: i64) -> Result<(), String> {
    db.require_license()?;
    let res = sqlx::query("UPDATE payoff_rule SET is_active = 0 WHERE id = ?1")
        .bind(id)
        .execute(&db.0)
        .await
        .map_err(|e| e.to_string())?;

    if res.rows_affected() == 0 {
        return Err("Payoff rule no encontrada.".into());
    }
    log_audit(&db.0, "delete_payoff_rule", "payoff_rule", Some(id), None).await?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct NewPayoffRule {
    event_id: i64,
    position: i64,
    percentage: f64,
}

#[tauri::command]
async fn create_payoff_rule(db: State<'_, Db>, rule: NewPayoffRule) -> Result<i64, String> {
    // Validate percentage (0.0 - 1.0)
    if rule.percentage < 0.0 || rule.percentage > 1.0 {
        return Err("Percentage must be between 0.0 and 1.0".into());
    }

    // Check if rule for this position already exists for this event (active or inactive)
    let exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM payoff_rule WHERE event_id = ?1 AND position = ?2")
            .bind(rule.event_id)
            .bind(rule.position)
            .fetch_optional(&db.0)
            .await
            .map_err(|e| e.to_string())?;

    if let Some(id) = exists {
        // Update existing rule (and reactivate it if it was deleted)
        sqlx::query("UPDATE payoff_rule SET percentage = ?1, is_active = 1 WHERE id = ?2")
            .bind(rule.percentage)
            .bind(id)
            .execute(&db.0)
            .await
            .map_err(|e| e.to_string())?;
        log_audit(&db.0, "update_payoff_rule", "payoff_rule", Some(id), None).await?;
        Ok(id)
    } else {
        // Create new rule
        let res = sqlx::query(
            r#"
            INSERT INTO payoff_rule (event_id, position, percentage, is_active)
            VALUES (?1, ?2, ?3, 1)
            "#,
        )
        .bind(rule.event_id)
        .bind(rule.position)
        .bind(rule.percentage)
        .execute(&db.0)
        .await
        .map_err(|e| e.to_string())?;
        let new_id = res.last_insert_rowid();
        log_audit(
            &db.0,
            "create_payoff_rule",
            "payoff_rule",
            Some(new_id),
            None,
        )
        .await?;
        Ok(new_id)
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize, Clone)]
struct PayoffAllocationConfig {
    deduction_pct: Option<f64>,
}

impl PayoffAllocationConfig {
    fn parsed(raw: &Option<String>) -> Self {
        if let Some(json) = raw {
            serde_json::from_str::<PayoffAllocationConfig>(json).unwrap_or_default()
        } else {
            PayoffAllocationConfig::default()
        }
    }
}

#[derive(serde::Serialize)]
struct PayoutBreakdown {
    total_pot: f64,
    deductions: f64,
    net_pot: f64,
    deduction_pct: f64,
    payouts: Vec<PayoutAllocation>,
}

#[derive(serde::Serialize)]
struct PayoutAllocation {
    place: i64,
    percentage: f64,
    amount: f64,
}

async fn get_payout_breakdown_internal(
    pool: &SqlitePool,
    event_id: i64,
) -> Result<PayoutBreakdown, String> {
    // 1. Get Event Details (Entry Fee, Prize Pool)
    // IMPORTANT: We need to satisfy EventRow struct which expects teams_count and pot.
    // We select 0 for them here because we calculate them manually below.
    let event: EventRow = sqlx::query_as(
        r#"
        SELECT 
            id, series_id, name, date, status, rounds, location, 
            entry_fee, prize_pool, max_team_rating, created_at, updated_at,
            payoff_allocation,
            admin_pin,
            0 as teams_count,
            0.0 as pot
        FROM event 
        WHERE id = ?1
        "#,
    )
    .bind(event_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    // 2. Count unique ropers in the event (each roper pays once, regardless of how many teams they're in)
    let unique_ropers: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(DISTINCT roper_id) FROM (
            SELECT header_id AS roper_id FROM team WHERE event_id = ?1 AND status = 'active'
            UNION
            SELECT heeler_id AS roper_id FROM team WHERE event_id = ?1 AND status = 'active'
        )
        "#,
    )
    .bind(event_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    // 3. Calculate Pot (per unique roper)
    let entry_fee = event.entry_fee.unwrap_or(0.0);
    let prize_pool = event.prize_pool.unwrap_or(0.0);
    let total_pot = (unique_ropers as f64 * entry_fee) + prize_pool;

    let config = PayoffAllocationConfig::parsed(&event.payoff_allocation);
    let deduction_pct = config.deduction_pct.unwrap_or(0.0).clamp(0.0, 1.0);
    let deductions = total_pot * deduction_pct;
    let net_pot = total_pot - deductions;

    // 4. Get Payoff Rules
    let rules: Vec<PayoffRuleRow> = sqlx::query_as(
        "SELECT id, event_id, position, percentage, is_active, created_at FROM payoff_rule WHERE event_id = ?1 AND is_active = 1 ORDER BY position ASC"
    )
    .bind(event_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    // 5. Calculate Allocations
    let payouts = rules
        .iter()
        .map(|r| PayoutAllocation {
            place: r.position,
            percentage: r.percentage,
            amount: net_pot * r.percentage,
        })
        .collect();

    Ok(PayoutBreakdown {
        total_pot,
        deductions,
        net_pot,
        deduction_pct,
        payouts,
    })
}

#[tauri::command]
async fn get_payout_breakdown(db: State<'_, Db>, event_id: i64) -> Result<PayoutBreakdown, String> {
    db.require_license()?;
    get_payout_breakdown_internal(&db.0, event_id).await
}

/* ------------------- RUNS (CAPTURE) ------------------- */
#[derive(serde::Deserialize)]
struct SaveRun {
    event_id: i64,
    team_id: i64,
    round: i64,
    position: i64,
    time_sec: Option<f64>, // null si NT/DQ
    penalty: f64,
    no_time: bool,
    dq: bool,
    captured_by: Option<i64>,
}

#[tauri::command]
async fn save_run(db: State<'_, Db>, payload: SaveRun) -> Result<i64, String> {
    db.require_license()?;

    // Validate that the team belongs to the provided event and is active
    let team_event_id: Option<i64> =
        sqlx::query_scalar("SELECT event_id FROM team WHERE id = ?1 AND status = 'active'")
            .bind(payload.team_id)
            .fetch_optional(&db.0)
            .await
            .map_err(|e| e.to_string())?;

    let Some(team_event_id) = team_event_id else {
        return Err("Equipo no encontrado o inactivo.".into());
    };

    if team_event_id != payload.event_id {
        return Err("El equipo no pertenece al evento indicado.".into());
    }

    // Ensure the target event still exists (not deleted)
    let event_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM event WHERE id = ?1 AND is_deleted = 0")
            .bind(payload.event_id)
            .fetch_optional(&db.0)
            .await
            .map_err(|e| e.to_string())?;
    if event_exists.is_none() {
        return Err("Evento no encontrado o inactivo.".into());
    }

    let total = if payload.no_time || payload.dq {
        None
    } else {
        payload.time_sec.map(|t| t + payload.penalty)
    };

    let res = sqlx::query(
        r#"
        INSERT INTO run (event_id, team_id, round, position, time_sec, penalty, total_sec, no_time, dq, status, captured_by)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'completed', ?10)
        ON CONFLICT(event_id, round, team_id) DO UPDATE SET
          position   = excluded.position,
          time_sec   = excluded.time_sec,
          penalty    = excluded.penalty,
          total_sec  = excluded.total_sec,
          no_time    = excluded.no_time,
          dq         = excluded.dq,
          status     = 'completed',
          updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
        "#
    )
    .bind(payload.event_id)
    .bind(payload.team_id)
    .bind(payload.round)
    .bind(payload.position)
    .bind(payload.time_sec)
    .bind(payload.penalty)
    .bind(total)
    .bind(payload.no_time as i32)
    .bind(payload.dq as i32)
    .bind(payload.captured_by)
    .execute(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    reconcile_future_runs(&db.0, payload.event_id).await?;

    let run_id = res.last_insert_rowid();
    log_audit(
        &db.0,
        "save_run",
        "run",
        Some(run_id),
        Some(format!(
            "Event {} Round {}",
            payload.event_id, payload.round
        )),
    )
    .await?;
    Ok(run_id)
}

/* ------------------- TEAMS ------------------- */
#[derive(serde::Serialize, sqlx::FromRow)]
struct RoperRow {
    id: i64,
    first_name: String,
    last_name: String,
    specialty: String,
    rating: i64,
    phone: Option<String>,
    email: Option<String>,
    level: String,
    external_id: Option<String>,
    normalized_phone: Option<String>,
    country_code: Option<String>,
    default_event_level: Option<String>,
    is_active: i64,
    created_at: String,
    updated_at: String,
}

#[derive(serde::Deserialize)]
struct NewRoper {
    first_name: String,
    last_name: String,
    specialty: String,
    rating: i64,
    phone: Option<String>,
    email: Option<String>,
    level: Option<String>,
    external_id: Option<String>,
    normalized_phone: Option<String>,
    country_code: Option<String>,
    default_event_level: Option<String>,
}

#[derive(serde::Deserialize)]
struct UpdateRoper {
    id: i64,
    first_name: Option<String>,
    last_name: Option<String>,
    specialty: Option<String>,
    rating: Option<i64>,
    phone: Option<String>,
    email: Option<String>,
    level: Option<String>,
    external_id: Option<String>,
    normalized_phone: Option<String>,
    country_code: Option<String>,
    default_event_level: Option<String>,
    is_active: Option<bool>,
}

#[derive(serde::Serialize, sqlx::FromRow)]
struct EventRosterRow {
    id: i64,
    event_id: i64,
    roper_id: i64,
    status: String,
    rating_override: Option<f64>,
    source_hash: Option<String>,
    notes: Option<String>,
    created_at: String,
    updated_at: String,
    first_name: String,
    last_name: String,
    specialty: String,
    rating: i64,
    level: String,
    phone: Option<String>,
    email: Option<String>,
}

#[derive(serde::Deserialize)]
struct EventRosterSyncEntry {
    external_id: Option<String>,
    first_name: String,
    last_name: String,
    specialty: Option<String>,
    rating: Option<f64>,
    phone: Option<String>,
    normalized_phone: Option<String>,
    email: Option<String>,
    level: Option<String>,
    status: Option<String>,
    rating_override: Option<f64>,
    notes: Option<String>,
    source_hash: Option<String>,
}

#[derive(serde::Deserialize)]
struct SyncEventRosterPayload {
    event_id: i64,
    entries: Vec<EventRosterSyncEntry>,
    withdraw_absent: Option<bool>,
}

#[derive(serde::Serialize)]
struct SyncEventRosterResult {
    created_ropers: usize,
    updated_ropers: usize,
    reactivated_ropers: usize,
    roster_upserts: usize,
    roster_marked_withdrawn: usize,
}

#[derive(serde::Deserialize)]
struct UpdateEventRosterEntry {
    id: i64,
    status: Option<String>,
    rating_override: Option<f64>,
    notes: Option<String>,
}

fn normalize_level_required(raw: Option<String>) -> Result<String, String> {
    let level = raw.unwrap_or_else(|| "amateur".to_string());
    let normalized = level.trim().to_lowercase();
    if normalized != "pro" && normalized != "amateur" && normalized != "principiante" {
        return Err("Nivel inválido: use 'pro', 'amateur' o 'principiante'.".into());
    }
    Ok(normalized)
}

fn normalize_level_optional(raw: Option<String>) -> Result<Option<String>, String> {
    match raw {
        Some(value) => {
            let normalized = value.trim().to_lowercase();
            if normalized.is_empty() {
                return Ok(None);
            }
            if normalized != "pro" && normalized != "amateur" && normalized != "principiante" {
                return Err("Nivel inválido: use 'pro', 'amateur' o 'principiante'.".into());
            }
            Ok(Some(normalized))
        }
        None => Ok(None),
    }
}

fn normalize_phone_value(phone: &Option<String>) -> Option<String> {
    phone
        .as_ref()
        .map(|p| p.chars().filter(|c| c.is_ascii_digit()).collect::<String>())
        .filter(|p| !p.is_empty())
}

fn clean_string(raw: &Option<String>) -> Option<String> {
    raw.as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn normalize_roster_status(raw: Option<String>) -> Result<String, String> {
    let status = raw
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "registered".into());
    match status.as_str() {
        "registered" | "confirmed" | "withdrawn" => Ok(status),
        _ => {
            Err("Status inválido para roster: usa 'registered', 'confirmed' o 'withdrawn'.".into())
        }
    }
}

async fn upsert_roper_from_entry(
    pool: &SqlitePool,
    entry: &EventRosterSyncEntry,
) -> Result<(i64, bool, bool, bool), String> {
    let first_name = entry.first_name.trim();
    if first_name.is_empty() {
        return Err("Cada registro debe incluir first_name.".into());
    }
    let last_name_raw = entry.last_name.trim();
    let last_name_owned = if last_name_raw.is_empty() {
        "-".to_string()
    } else {
        last_name_raw.to_string()
    };

    let specialty_clean = entry
        .specialty
        .as_ref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| matches!(s.as_str(), "header" | "heeler" | "both"));
    if entry.specialty.is_some() && specialty_clean.is_none() {
        return Err("Specialty inválida en la importación.".into());
    }
    let specialty_value = specialty_clean.clone().unwrap_or_else(|| "both".into());

    let rating_value = entry.rating.unwrap_or(0.0);
    if rating_value.is_sign_negative() {
        return Err("Rating inválido: no puede ser negativo.".into());
    }
    if rating_value.is_nan() {
        return Err("Rating inválido: no puede ser NaN.".into());
    }
    let rating_i64 = rating_value.round() as i64;

    let raw_level = entry.level.clone();
    let normalized_level_for_insert = normalize_level_required(raw_level.clone())?;
    let level_for_update = if raw_level.is_some() {
        Some(normalized_level_for_insert.clone())
    } else {
        None
    };

    let phone_clean = clean_string(&entry.phone);
    let normalized_phone = entry
        .normalized_phone
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| normalize_phone_value(&entry.phone));
    let email_clean = clean_string(&entry.email);
    let external_id_clean = clean_string(&entry.external_id);

    let mut roper_row: Option<(i64, bool)> = None;
    if let Some(ext) = external_id_clean.as_ref() {
        roper_row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT id, is_active FROM roper WHERE external_id = ?1 LIMIT 1",
        )
        .bind(ext)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .map(|(id, is_active)| (id, is_active == 1));
    }
    if roper_row.is_none() {
        if let Some(email) = email_clean.as_ref() {
            roper_row = sqlx::query_as::<_, (i64, i64)>(
                "SELECT id, is_active FROM roper WHERE LOWER(email) = LOWER(?1) LIMIT 1",
            )
            .bind(email)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?
            .map(|(id, is_active)| (id, is_active == 1));
        }
    }
    if roper_row.is_none() {
        if let Some(norm_phone) = normalized_phone.as_ref() {
            roper_row = sqlx::query_as::<_, (i64, i64)>(
                "SELECT id, is_active FROM roper WHERE normalized_phone = ?1 LIMIT 1",
            )
            .bind(norm_phone)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?
            .map(|(id, is_active)| (id, is_active == 1));
        }
    }
    if roper_row.is_none() {
        roper_row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT id, is_active FROM roper WHERE LOWER(first_name) = LOWER(?1) AND LOWER(CASE WHEN TRIM(last_name) = '' THEN '-' ELSE last_name END) = LOWER(?2) LIMIT 1",
        )
        .bind(first_name)
        .bind(&last_name_owned)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .map(|(id, is_active)| (id, is_active == 1));
    }

    if let Some((roper_id, is_active)) = roper_row {
        let mut builder = QueryBuilder::<Sqlite>::new("UPDATE roper SET ");
        builder
            .push("first_name = ")
            .push_bind(first_name.to_string())
            .push(", ");
        builder
            .push("last_name = ")
            .push_bind(last_name_owned.clone())
            .push(", ");

        if let Some(spec) = specialty_clean {
            builder.push("specialty = ").push_bind(spec).push(", ");
        }
        if entry.rating.is_some() {
            builder.push("rating = ").push_bind(rating_i64).push(", ");
        }
        if entry.phone.is_some() {
            if let Some(phone) = phone_clean.as_ref() {
                builder.push("phone = ").push_bind(phone).push(", ");
            } else {
                builder.push("phone = NULL, ");
            }
        }
        if entry.email.is_some() {
            if let Some(email) = email_clean.as_ref() {
                builder.push("email = ").push_bind(email).push(", ");
            } else {
                builder.push("email = NULL, ");
            }
        }
        if let Some(level) = level_for_update {
            builder.push("level = ").push_bind(level).push(", ");
        }
        if entry.external_id.is_some() {
            if let Some(ext) = external_id_clean.as_ref() {
                builder.push("external_id = ").push_bind(ext).push(", ");
            } else {
                builder.push("external_id = NULL, ");
            }
        }
        if entry.normalized_phone.is_some() || entry.phone.is_some() {
            if let Some(norm) = normalized_phone.as_ref() {
                builder
                    .push("normalized_phone = ")
                    .push_bind(norm)
                    .push(", ");
            } else {
                builder.push("normalized_phone = NULL, ");
            }
        }
        let mut reactivated = false;
        if !is_active {
            builder.push("is_active = 1, ");
            reactivated = true;
        }

        builder
            .push("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ")
            .push_bind(roper_id);
        builder
            .build()
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;

        Ok((roper_id, false, true, reactivated))
    } else {
        let res = sqlx::query(
            r#"
            INSERT INTO roper (
                first_name,
                last_name,
                specialty,
                rating,
                phone,
                email,
                level,
                external_id,
                normalized_phone,
                country_code,
                default_event_level
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL)
            "#,
        )
        .bind(first_name)
        .bind(&last_name_owned)
        .bind(&specialty_value)
        .bind(rating_i64)
        .bind(phone_clean.clone())
        .bind(email_clean.clone())
        .bind(&normalized_level_for_insert)
        .bind(external_id_clean.clone())
        .bind(normalized_phone.clone())
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok((res.last_insert_rowid(), true, true, false))
    }
}

#[derive(serde::Serialize, sqlx::FromRow)]
struct TeamRow {
    id: i64,
    event_id: i64,
    header_id: i64,
    heeler_id: i64,
    rating: f64,
    status: String,
    created_at: String,
    updated_at: String,
}

#[tauri::command]
async fn list_teams(db: State<'_, Db>, event_id: i64) -> Result<Vec<TeamRow>, String> {
    db.require_license()?;
    tracing::info!(event_id, "list_teams: called");

    let rows = sqlx::query_as::<_, TeamRow>(
        r#"
        SELECT id, event_id, header_id, heeler_id, rating, status, created_at, updated_at
        FROM team
        WHERE event_id = ?1 AND status = 'active'
        ORDER BY id ASC
        "#,
    )
    .bind(event_id)
    .fetch_all(&db.0)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, event_id, "list_teams failed");
        e.to_string()
    })?;

    tracing::info!(event_id, count = rows.len(), "list_teams: returning rows");
    Ok(rows)
}

#[derive(serde::Deserialize)]
struct NewTeam {
    event_id: i64,
    header_id: i64,
    heeler_id: i64,
    rating: f64,
}

async fn create_team_internal(db: &Db, t: NewTeam) -> Result<i64, String> {
    tracing::info!(
        event_id = t.event_id,
        header_id = t.header_id,
        heeler_id = t.heeler_id,
        rating = t.rating,
        "create_team: attempt"
    );

    ensure_event_unlocked(&db.0, t.event_id).await?;

    // Validación básica: header != heeler
    if t.header_id == t.heeler_id {
        tracing::error!(
            header_id = t.header_id,
            heeler_id = t.heeler_id,
            "create_team failed: same header and heeler"
        );
        return Err(
            "Header y Heeler no pueden ser la misma persona. Aún no clonamos vaqueros.".into(),
        );
    }

    let rows =
        sqlx::query_as::<_, (i64, i64)>("SELECT id, is_active FROM roper WHERE id = ?1 OR id = ?2")
            .bind(t.header_id)
            .bind(t.heeler_id)
            .fetch_all(&db.0)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "create_team: error checking ropers");
                e.to_string()
            })?;

    let mut header_active: Option<bool> = None;
    let mut heeler_active: Option<bool> = None;
    for (id, is_active) in rows {
        if id == t.header_id {
            header_active = Some(is_active == 1);
        } else if id == t.heeler_id {
            heeler_active = Some(is_active == 1);
        }
    }
    if header_active.is_none() || heeler_active.is_none() {
        tracing::error!("create_team failed: missing roper record");
        return Err("Header o Heeler no existen en la tabla roper.".into());
    }
    if !header_active.unwrap() || !heeler_active.unwrap() {
        return Err(
            "Al menos uno de los ropers está inactivo. Rehabilítalo desde el directorio.".into(),
        );
    }

    // Verifica que ambos estén inscritos en el roster del evento
    let roster_counts: (i64, i64) = sqlx::query_as(
        r#"
        SELECT 
          (SELECT COUNT(1) FROM event_roster WHERE event_id = ?1 AND roper_id = ?2 AND status != 'withdrawn') AS header_count,
          (SELECT COUNT(1) FROM event_roster WHERE event_id = ?1 AND roper_id = ?3 AND status != 'withdrawn') AS heeler_count
        "#,
    )
    .bind(t.event_id)
    .bind(t.header_id)
    .bind(t.heeler_id)
    .fetch_one(&db.0)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, event_id = t.event_id, "create_team: roster validation failed");
        e.to_string()
    })?;

    if roster_counts.0 == 0 || roster_counts.1 == 0 {
        return Err(
            "Ambos ropers deben estar en el roster del evento (status distinto a 'withdrawn')."
                .into(),
        );
    }

    let res = sqlx::query(
        r#"
        INSERT INTO team (event_id, header_id, heeler_id, rating, status)
        VALUES (?1, ?2, ?3, ?4, 'active')
        "#,
    )
    .bind(t.event_id)
    .bind(t.header_id)
    .bind(t.heeler_id)
    .bind(t.rating)
    .execute(&db.0)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "create_team failed: insert error");
        e.to_string()
    })?;

    let last_id = res.last_insert_rowid();
    log_audit(
        &db.0,
        "create_team",
        "team",
        Some(last_id),
        Some(format!("Event {}", t.event_id)),
    )
    .await?;
    Ok(last_id)
}

#[tauri::command]
async fn create_team(db: State<'_, Db>, t: NewTeam) -> Result<i64, String> {
    db.require_license()?;
    match create_team_internal(&db, t).await {
        Ok(id) => Ok(id),
        Err(err) if err.contains("UNIQUE") => {
            Err("Ya existe un equipo con ese header/heeler en este evento.".into())
        }
        other => other,
    }
}

#[tauri::command]
async fn hard_delete_teams_for_event(db: State<'_, Db>, event_id: i64) -> Result<(), String> {
    db.require_license()?;
    tracing::info!(event_id, "hard_delete_teams_for_event: starting");
    // verificar que el evento exista y no esté locked
    ensure_event_unlocked(&db.0, event_id).await?;

    let res = sqlx::query("DELETE FROM team WHERE event_id = ?1")
        .bind(event_id)
        .execute(&db.0)
        .await;

    match res {
        Ok(r) => {
            tracing::info!(
                deleted = r.rows_affected(),
                event_id,
                "hard_delete_teams_for_event: completed"
            );
            log_audit(
                &db.0,
                "hard_delete_teams",
                "team",
                None,
                Some(format!("Event {}", event_id)),
            )
            .await?;
            Ok(())
        }
        Err(e) => {
            tracing::error!(error = %e, event_id, "hard_delete_teams_for_event failed");
            Err(e.to_string())
        }
    }
}

#[tauri::command]
async fn list_all_events_raw(db: State<'_, Db>) -> Result<Vec<EventRow>, String> {
    db.require_license()?;
    tracing::info!("list_all_events_raw: returning all events without is_deleted filter");
    sqlx::query_as::<_, EventRow>(
        r#"
        SELECT 
             e.id, e.series_id, e.name, e.date, e.status, e.rounds, e.location,
             e.entry_fee, e.prize_pool, e.max_team_rating, e.created_at, e.updated_at,
             e.payoff_allocation, e.admin_pin,
             (SELECT COUNT(*) FROM team t WHERE t.event_id = e.id AND t.status = 'active') as teams_count,
             (
                COALESCE(e.prize_pool, 0.0) + 
                (COALESCE(e.entry_fee, 0.0) * (SELECT COUNT(*) FROM team t WHERE t.event_id = e.id AND t.status = 'active'))
             ) as pot
        FROM event e
        ORDER BY e.date ASC, e.id ASC
        "#,
    )
    .fetch_all(&db.0)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "list_all_events_raw failed");
        e.to_string()
    })
}

#[tauri::command]
async fn get_series_logs(
    db: State<'_, Db>,
    series_id: i64,
    limit: i64,
) -> Result<Vec<AuditLogItem>, String> {
    db.require_license()?;
    sqlx::query_as::<_, AuditLogItem>(
        r#"
        SELECT id, action, entity_type, entity_id, user_id, metadata, created_at
        FROM audit_log
        WHERE (entity_type = 'series' AND entity_id = ?1)
           OR (entity_type = 'event' AND entity_id IN (SELECT id FROM event WHERE series_id = ?1))
        ORDER BY created_at DESC
        LIMIT ?2
        "#,
    )
    .bind(series_id)
    .bind(limit)
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())
}

/* ------------------- ROPERS ------------------- */

#[tauri::command]
async fn list_ropers(
    db: State<'_, Db>,
    include_inactive: Option<bool>,
) -> Result<Vec<RoperRow>, String> {
    db.require_license()?;
    let include_inactive = include_inactive.unwrap_or(false);
    let mut query = String::from(
        r#"
        SELECT id,
               first_name,
               last_name,
               specialty,
               CAST(rating AS INTEGER) AS rating,
               phone,
               email,
               level,
               external_id,
               normalized_phone,
               country_code,
               default_event_level,
               is_active,
               created_at,
               updated_at
        FROM roper
        "#,
    );
    if !include_inactive {
        query.push_str("WHERE is_active = 1 ");
    }
    query.push_str("ORDER BY last_name, first_name");

    sqlx::query_as::<_, RoperRow>(&query)
        .fetch_all(&db.0)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_roper(db: State<'_, Db>, r: NewRoper) -> Result<i64, String> {
    db.require_license()?;
    // Validar specialty
    if r.specialty != "header" && r.specialty != "heeler" && r.specialty != "both" {
        return Err("Specialty inválida: usa 'header', 'heeler' o 'both'.".into());
    }
    if r.rating < 0 {
        return Err("Rating inválido: debe ser >= 0.".into());
    }

    // validar nivel
    let level_l = normalize_level_required(r.level)?;
    let default_level = normalize_level_optional(r.default_event_level)?;

    let external_id = r
        .external_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let normalized_phone = r
        .normalized_phone
        .clone()
        .filter(|p| !p.trim().is_empty())
        .or_else(|| normalize_phone_value(&r.phone));
    let country_code = r
        .country_code
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let res = sqlx::query(
        r#"
        INSERT INTO roper (
            first_name,
            last_name,
            specialty,
            rating,
            phone,
            email,
            level,
            external_id,
            normalized_phone,
            country_code,
            default_event_level
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        "#,
    )
    .bind(&r.first_name)
    .bind(&r.last_name)
    .bind(&r.specialty)
    .bind(r.rating)
    .bind(&r.phone)
    .bind(&r.email)
    .bind(level_l)
    .bind(external_id)
    .bind(normalized_phone)
    .bind(country_code)
    .bind(default_level)
    .execute(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    let id = res.last_insert_rowid();
    log_audit(
        &db.0,
        "create_roper",
        "roper",
        Some(id),
        Some(format!("{} {}", r.first_name, r.last_name)),
    )
    .await?;
    Ok(id)
}

#[tauri::command]
async fn update_roper(db: State<'_, Db>, r: UpdateRoper) -> Result<(), String> {
    db.require_license()?;
    // verificar existencia
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM roper WHERE id = ?1")
        .bind(r.id)
        .fetch_optional(&db.0)
        .await
        .map_err(|e| e.to_string())?;
    let Some(_exists) = exists else {
        return Err("Roper no encontrado.".into());
    };

    let mut builder = QueryBuilder::<Sqlite>::new("UPDATE roper SET ");
    let mut has_any = false;

    if let Some(first) = r.first_name {
        builder.push("first_name = ").push_bind(first).push(", ");
        has_any = true;
    }
    if let Some(last) = r.last_name {
        builder.push("last_name = ").push_bind(last).push(", ");
        has_any = true;
    }
    if let Some(spec) = r.specialty {
        if spec != "header" && spec != "heeler" && spec != "both" {
            return Err("Specialty inválida: usa 'header', 'heeler' o 'both'.".into());
        }
        builder.push("specialty = ").push_bind(spec).push(", ");
        has_any = true;
    }
    if let Some(rating) = r.rating {
        if rating < 0 {
            return Err("Rating inválido: debe ser >= 0.".into());
        }
        builder.push("rating = ").push_bind(rating).push(", ");
        has_any = true;
    }
    if let Some(phone) = &r.phone {
        builder.push("phone = ").push_bind(phone).push(", ");
        has_any = true;
    }
    if let Some(email) = &r.email {
        builder.push("email = ").push_bind(email).push(", ");
        has_any = true;
    }
    if let Some(level) = r.level {
        let lvl = normalize_level_required(Some(level))?;
        builder.push("level = ").push_bind(lvl).push(", ");
        has_any = true;
    }
    if let Some(raw_external) = r.external_id.as_ref() {
        let trimmed = raw_external.trim();
        if trimmed.is_empty() {
            builder.push("external_id = NULL, ");
        } else {
            builder
                .push("external_id = ")
                .push_bind(trimmed.to_string())
                .push(", ");
        }
        has_any = true;
    }

    let mut normalized_override: Option<String> = None;
    if let Some(raw_normalized) = r
        .normalized_phone
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        normalized_override = Some(raw_normalized);
    } else if r.phone.is_some() {
        normalized_override = normalize_phone_value(&r.phone);
    }
    if let Some(norm_phone) = normalized_override {
        builder
            .push("normalized_phone = ")
            .push_bind(norm_phone)
            .push(", ");
        has_any = true;
    }
    if let Some(raw_cc) = r.country_code.as_ref() {
        let trimmed = raw_cc.trim();
        if trimmed.is_empty() {
            builder.push("country_code = NULL, ");
        } else {
            builder
                .push("country_code = ")
                .push_bind(trimmed.to_string())
                .push(", ");
        }
        has_any = true;
    }
    if let Some(default_level) = normalize_level_optional(r.default_event_level)? {
        builder
            .push("default_event_level = ")
            .push_bind(default_level)
            .push(", ");
        has_any = true;
    }
    if let Some(active) = r.is_active {
        builder
            .push("is_active = ")
            .push_bind(if active { 1 } else { 0 })
            .push(", ");
        has_any = true;
    }

    if !has_any {
        return Ok(());
    }

    builder
        .push("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ")
        .push_bind(r.id);
    builder
        .build()
        .execute(&db.0)
        .await
        .map_err(|e| e.to_string())?;

    log_audit(&db.0, "update_roper", "roper", Some(r.id), None).await?;
    Ok(())
}

#[tauri::command]
async fn delete_roper(db: State<'_, Db>, id: i64) -> Result<(), String> {
    db.require_license()?;
    // Política: soft-delete para ropers. Marcamos `is_active = 0`.
    let res = sqlx::query("UPDATE roper SET is_active = 0, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?1")
        .bind(id)
        .execute(&db.0)
        .await
        .map_err(|e| e.to_string())?;

    if res.rows_affected() == 0 {
        return Err("Roper no encontrado.".into());
    }

    sqlx::query(
        "UPDATE event_roster SET status = 'withdrawn', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE roper_id = ?1 AND status != 'withdrawn'",
    )
    .bind(id)
    .execute(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    log_audit(&db.0, "delete_roper", "roper", Some(id), None).await?;
    Ok(())
}

#[tauri::command]
async fn delete_all_ropers(db: State<'_, Db>) -> Result<i64, String> {
    db.require_license()?;
    // Hard-delete: primero elimina todos los equipos, luego elimina todos los ropers

    // Paso 1: Eliminar todos los equipos
    sqlx::query("DELETE FROM team")
        .execute(&db.0)
        .await
        .map_err(|e| format!("Error eliminando equipos: {e}"))?;

    // Paso 2: Eliminar todos los registros de roster
    sqlx::query("DELETE FROM event_roster")
        .execute(&db.0)
        .await
        .map_err(|e| format!("Error eliminando roster: {e}"))?;

    // Paso 3: Eliminar todos los ropers
    let res = sqlx::query("DELETE FROM roper")
        .execute(&db.0)
        .await
        .map_err(|e| format!("Error eliminando ropers: {e}"))?;

    let count = res.rows_affected() as i64;

    log_audit(
        &db.0,
        "delete_all_ropers",
        "roper",
        None,
        Some(format!(
            "Deleted {} ropers, all teams, and roster entries",
            count
        )),
    )
    .await?;
    Ok(count)
}

#[tauri::command]
async fn list_event_roster(
    db: State<'_, Db>,
    event_id: i64,
    include_withdrawn: Option<bool>,
) -> Result<Vec<EventRosterRow>, String> {
    db.require_license()?;
    let mut query = String::from(
        r#"
        SELECT
            er.id,
            er.event_id,
            er.roper_id,
            er.status,
            er.rating_override,
            er.source_hash,
            er.notes,
            er.created_at,
            er.updated_at,
            r.first_name,
            r.last_name,
            r.specialty,
            CAST(r.rating AS INTEGER) AS rating,
            r.level,
            r.phone,
            r.email
        FROM event_roster er
        JOIN roper r ON r.id = er.roper_id
        WHERE er.event_id = ?1
        "#,
    );
    if !include_withdrawn.unwrap_or(false) {
        query.push_str("AND er.status != 'withdrawn' ");
    }
    query.push_str("ORDER BY r.last_name, r.first_name");

    sqlx::query_as::<_, EventRosterRow>(&query)
        .bind(event_id)
        .fetch_all(&db.0)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_event_roster_entry(
    db: State<'_, Db>,
    payload: UpdateEventRosterEntry,
) -> Result<(), String> {
    db.require_license()?;
    let mut builder = QueryBuilder::<Sqlite>::new("UPDATE event_roster SET ");
    let mut changed = false;

    if let Some(status) = payload.status {
        let normalized = normalize_roster_status(Some(status))?;
        builder.push("status = ").push_bind(normalized).push(", ");
        changed = true;
    }
    if let Some(rating) = payload.rating_override {
        if !rating.is_finite() {
            return Err("rating_override inválido.".into());
        }
        builder
            .push("rating_override = ")
            .push_bind(rating)
            .push(", ");
        changed = true;
    }
    if let Some(notes_raw) = payload.notes {
        if notes_raw.trim().is_empty() {
            builder.push("notes = NULL, ");
        } else {
            builder
                .push("notes = ")
                .push_bind(notes_raw.trim().to_string())
                .push(", ");
        }
        changed = true;
    }

    if !changed {
        return Ok(());
    }

    builder
        .push("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ")
        .push_bind(payload.id);
    builder
        .build()
        .execute(&db.0)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn sync_event_roster_internal(
    db: &Db,
    payload: SyncEventRosterPayload,
) -> Result<SyncEventRosterResult, String> {
    ensure_event_unlocked(&db.0, payload.event_id).await?;

    let withdraw_absent = payload.withdraw_absent.unwrap_or(true);
    let mut created_ropers = 0usize;
    let mut updated_ropers = 0usize;
    let mut reactivated_ropers = 0usize;
    let mut roster_upserts = 0usize;
    let mut processed: HashSet<i64> = HashSet::new();

    for entry in &payload.entries {
        let (roper_id, created, updated, reactivated) =
            upsert_roper_from_entry(&db.0, entry).await?;
        if created {
            created_ropers += 1;
        } else if updated {
            updated_ropers += 1;
        }
        if reactivated {
            reactivated_ropers += 1;
        }
        processed.insert(roper_id);

        let status = normalize_roster_status(entry.status.clone())?;
        let rating_override = match entry.rating_override {
            Some(value) => {
                if !value.is_finite() {
                    return Err("rating_override inválido.".into());
                }
                Some(value)
            }
            None => None,
        };
        let notes = clean_string(&entry.notes);
        let source_hash = clean_string(&entry.source_hash);

        sqlx::query(
            r#"
            INSERT INTO event_roster (event_id, roper_id, status, rating_override, source_hash, notes, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            ON CONFLICT(event_id, roper_id)
            DO UPDATE SET
                status = excluded.status,
                rating_override = excluded.rating_override,
                source_hash = excluded.source_hash,
                notes = excluded.notes,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(payload.event_id)
        .bind(roper_id)
        .bind(status)
        .bind(rating_override)
        .bind(source_hash)
        .bind(notes)
        .execute(&db.0)
        .await
        .map_err(|e| e.to_string())?;

        roster_upserts += 1;
    }

    let mut roster_marked_withdrawn = 0usize;
    if withdraw_absent {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "UPDATE event_roster SET status = 'withdrawn', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE event_id = ",
        );
        builder.push_bind(payload.event_id);
        if !processed.is_empty() {
            builder.push(" AND roper_id NOT IN (");
            let mut separated = builder.separated(", ");
            for roper_id in processed.iter() {
                separated.push_bind(roper_id);
            }
            builder.push(")");
        }
        builder.push(" AND status != 'withdrawn'");

        let res = builder
            .build()
            .execute(&db.0)
            .await
            .map_err(|e| e.to_string())?;
        roster_marked_withdrawn = res.rows_affected() as usize;
    }

    log_audit(
        &db.0,
        "sync_event_roster",
        "event_roster",
        Some(payload.event_id),
        Some(format!(
            "entries={} created={} updated={} reactivated={} withdrawn={}",
            payload.entries.len(),
            created_ropers,
            updated_ropers,
            reactivated_ropers,
            roster_marked_withdrawn
        )),
    )
    .await?;

    Ok(SyncEventRosterResult {
        created_ropers,
        updated_ropers,
        reactivated_ropers,
        roster_upserts,
        roster_marked_withdrawn,
    })
}

#[tauri::command]
async fn sync_event_roster(
    db: State<'_, Db>,
    payload: SyncEventRosterPayload,
) -> Result<SyncEventRosterResult, String> {
    db.require_license()?;
    sync_event_roster_internal(&db, payload).await
}

#[derive(serde::Deserialize)]
struct UpdateTeam {
    id: i64,
    rating: Option<f64>,
    status: Option<String>, // 'active' | 'inactive'
}

#[tauri::command]
async fn update_team(db: State<'_, Db>, t: UpdateTeam) -> Result<(), String> {
    db.require_license()?;
    // Lee event_id del team para validar lock
    let event_id: Option<i64> = sqlx::query_scalar("SELECT event_id FROM team WHERE id = ?1")
        .bind(t.id)
        .fetch_optional(&db.0)
        .await
        .map_err(|e| e.to_string())?;

    let Some(event_id) = event_id else {
        return Err("Team no encontrado.".into());
    };
    ensure_event_unlocked(&db.0, event_id).await?;

    // Construye UPDATE dinámico simple
    let mut tx: Transaction<'_, Sqlite> = db.0.begin().await.map_err(|e| e.to_string())?;
    if let Some(r) = t.rating {
        sqlx::query("UPDATE team SET rating = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2")
            .bind(r)
            .bind(t.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    if let Some(st) = t.status {
        if st != "active" && st != "inactive" {
            return Err("Status inválido: usa 'active' o 'inactive'.".into());
        }
        sqlx::query("UPDATE team SET status = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2")
            .bind(st)
            .bind(t.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    log_audit(&db.0, "update_team", "team", Some(t.id), None).await?;
    Ok(())
}

#[tauri::command]
async fn delete_team(db: State<'_, Db>, id: i64) -> Result<(), String> {
    db.require_license()?;
    // Obtén event_id y valida lock
    let event_id: Option<i64> = sqlx::query_scalar("SELECT event_id FROM team WHERE id = ?1")
        .bind(id)
        .fetch_optional(&db.0)
        .await
        .map_err(|e| e.to_string())?;
    let Some(event_id) = event_id else {
        return Err("Team no encontrado.".into());
    };
    ensure_event_unlocked(&db.0, event_id).await?;

    // Política: soft-delete para teams. Marcamos status = 'inactive'.
    let res = sqlx::query("UPDATE team SET status = 'inactive', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?1")
        .bind(id)
        .execute(&db.0)
        .await
        .map_err(|e| e.to_string())?;

    if res.rows_affected() == 0 {
        return Err("Team no encontrado.".into());
    }

    log_audit(&db.0, "delete_team", "team", Some(id), None).await?;
    Ok(())
}

#[tauri::command]
async fn delete_series(db: State<'_, Db>, id: i64) -> Result<(), String> {
    db.require_license()?;
    // verificar que la serie exista
    let exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM series WHERE id = ?1 AND is_deleted = 0")
            .bind(id)
            .fetch_optional(&db.0)
            .await
            .map_err(|e| e.to_string())?;
    let Some(_exists) = exists else {
        return Err("Serie no encontrada.".into());
    };

    // impedir borrado si hay eventos locked
    let locked_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM event WHERE series_id = ?1 AND status = 'locked' AND is_deleted = 0",
    )
    .bind(id)
    .fetch_one(&db.0)
    .await
    .map_err(|e| e.to_string())?;
    if locked_count > 0 {
        return Err("Hay eventos bloqueados en la serie; desbloquea los eventos antes de eliminar la serie.".into());
    }

    // soft-delete series y eventos asociados en una transacción
    let mut tx: Transaction<'_, Sqlite> = db.0.begin().await.map_err(|e| e.to_string())?;

    sqlx::query("UPDATE series SET is_deleted = 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    // además de marcar is_deleted, no cambiamos status a 'archived' para evitar error de constraint.
    sqlx::query("UPDATE event SET is_deleted = 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE series_id = ?1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;
    log_audit(&db.0, "delete_series", "series", Some(id), None).await?;
    Ok(())
}

/* ------------------- RUNS / DRAW ------------------- */
#[derive(serde::Serialize, sqlx::FromRow)]
struct RunRow {
    id: i64,
    event_id: i64,
    team_id: i64,
    round: i64,
    position: i64,
    time_sec: Option<f64>,
    penalty: f64,
    total_sec: Option<f64>,
    no_time: i64,
    dq: i64,
    status: String,
    captured_by: Option<i64>,
    created_at: String,
    updated_at: String,
}

#[tauri::command]
async fn get_runs(
    db: State<'_, Db>,
    event_id: i64,
    round: Option<i64>,
) -> Result<Vec<RunRow>, String> {
    db.require_license()?;
    reconcile_future_runs(&db.0, event_id).await?;
    if let Some(r) = round {
        sqlx::query_as::<_, RunRow>(
            r#"
            SELECT id, event_id, team_id, round, position, time_sec, penalty, total_sec,
                   no_time, dq, status, captured_by, created_at, updated_at
            FROM run
            WHERE event_id = ?1 AND round = ?2
            ORDER BY position ASC, id ASC
            "#,
        )
        .bind(event_id)
        .bind(r)
        .fetch_all(&db.0)
        .await
        .map_err(|e| e.to_string())
    } else {
        sqlx::query_as::<_, RunRow>(
            r#"
            SELECT id, event_id, team_id, round, position, time_sec, penalty, total_sec,
                   no_time, dq, status, captured_by, created_at, updated_at
            FROM run
            WHERE event_id = ?1
            ORDER BY round ASC, position ASC, id ASC
            "#,
        )
        .bind(event_id)
        .fetch_all(&db.0)
        .await
        .map_err(|e| e.to_string())
    }
}

#[derive(serde::Serialize, sqlx::FromRow)]
struct RunExpandedRow {
    id: i64,
    event_id: i64,
    team_id: i64,
    round: i64,
    position: i64,
    header_name: String,
    heeler_name: String,
    time_sec: Option<f64>,
    penalty: f64,
    total_sec: Option<f64>,
    no_time: i64,
    dq: i64,
    status: String,
}

#[tauri::command]
async fn get_runs_expanded(
    db: State<'_, Db>,
    event_id: i64,
    round: Option<i64>,
) -> Result<Vec<RunExpandedRow>, String> {
    db.require_license()?;
    reconcile_future_runs(&db.0, event_id).await?;
    let base_query = r#"
        SELECT
          r.id, r.event_id, r.team_id, r.round, r.position,
          r.time_sec, r.penalty, r.total_sec, r.status, r.no_time, r.dq,
          (rh.first_name || ' ' || rh.last_name) as header_name,
          (rhe.first_name || ' ' || rhe.last_name) as heeler_name
        FROM run r
        JOIN team t ON r.team_id = t.id
        JOIN roper rh ON t.header_id = rh.id
        JOIN roper rhe ON t.heeler_id = rhe.id
    "#;

    if let Some(r) = round {
        let q = format!(
            "{} WHERE r.event_id = ?1 AND r.round = ?2 ORDER BY r.position ASC",
            base_query
        );
        sqlx::query_as::<_, RunExpandedRow>(&q)
            .bind(event_id)
            .bind(r)
            .fetch_all(&db.0)
            .await
            .map_err(|e| e.to_string())
    } else {
        let q = format!(
            "{} WHERE r.event_id = ?1 ORDER BY r.round ASC, r.position ASC",
            base_query
        );
        sqlx::query_as::<_, RunExpandedRow>(&q)
            .bind(event_id)
            .fetch_all(&db.0)
            .await
            .map_err(|e| e.to_string())
    }
}

#[derive(serde::Deserialize)]
struct GenerateDrawOptions {
    event_id: i64,
    round: i64,
    reseed: Option<bool>,
    seed_runs: Option<bool>,
}

#[tauri::command]
async fn generate_draw(db: State<'_, Db>, opts: GenerateDrawOptions) -> Result<i64, String> {
    db.require_license()?;
    // 1) Relaxed check: Only block if event is fully finalized/completed, OR if THIS specific round is started.
    // We do NOT use ensure_event_unlocked because that blocks 'locked'/'active' events which are exactly where we want to generate next rounds.

    let event_status: Option<String> = sqlx::query_scalar("SELECT status FROM event WHERE id = ?1")
        .bind(opts.event_id)
        .fetch_optional(&db.0)
        .await
        .map_err(|e| e.to_string())?
        .flatten();

    if let Some(s) = event_status {
        if s == "completed" || s == "finalized" || s == "archived" {
            return Err(
                "El evento está finalizado o archivado. No se pueden modificar rondas.".into(),
            );
        }
    }

    // Check if THIS round has started (any completed runs)
    let round_started: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM run WHERE event_id = ?1 AND round = ?2 AND status = 'completed')"
    )
    .bind(opts.event_id)
    .bind(opts.round)
    .fetch_one(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    if round_started {
        return Err(format!(
            "La ronda {} ya ha comenzado (tiene tiempos capturados). No se puede regenerar.",
            opts.round
        ));
    }

    // Get the total number of rounds for this event to check if this is the final round
    let total_rounds: i64 = sqlx::query_scalar("SELECT rounds FROM event WHERE id = ?1")
        .bind(opts.event_id)
        .fetch_one(&db.0)
        .await
        .map_err(|e| e.to_string())?;

    let is_final_round = opts.round == total_rounds;

    // 2) obtener teams activos del evento que NO estén eliminados (NT o DQ previos)
    let mut teams: Vec<i64> = sqlx::query_scalar(
        r#"
        SELECT id FROM team 
        WHERE event_id = ?1 AND status = 'active'
          AND id NOT IN (
            SELECT team_id FROM run 
            WHERE event_id = ?1 AND (no_time = 1 OR dq = 1)
          )
        ORDER BY id ASC
        "#,
    )
    .bind(opts.event_id)
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    if teams.is_empty() {
        return Err("No hay equipos activos para generar el draw.".into());
    }

    if opts.round == 1 {
        // First round: random draw once
        if opts.reseed.unwrap_or(true) {
            teams.shuffle(&mut thread_rng());
        }
    } else if is_final_round {
        // Final round: order by accumulated time (highest to lowest)
        let base_order = load_round_order(&db.0, opts.event_id, 1).await?;
        let base_rank: HashMap<i64, usize> = base_order
            .into_iter()
            .enumerate()
            .map(|(idx, team_id)| (team_id, idx))
            .collect();

        let mut with_times: Vec<(i64, f64)> = Vec::new();
        let mut without_times: Vec<i64> = Vec::new();

        for &team_id in &teams {
            let total: Option<f64> = sqlx::query_scalar(
                r#"
                SELECT SUM(total_sec)
                FROM run
                WHERE event_id = ?1 
                  AND team_id = ?2
                  AND round < ?3
                  AND status = 'completed'
                  AND no_time = 0
                  AND dq = 0
                "#,
            )
            .bind(opts.event_id)
            .bind(team_id)
            .bind(opts.round)
            .fetch_one(&db.0)
            .await
            .map_err(|e| e.to_string())?;

            if let Some(value) = total {
                with_times.push((team_id, value));
            } else {
                without_times.push(team_id);
            }
        }

        with_times.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    let a_rank = base_rank.get(&a.0).copied().unwrap_or(usize::MAX);
                    let b_rank = base_rank.get(&b.0).copied().unwrap_or(usize::MAX);
                    a_rank.cmp(&b_rank)
                })
        });

        without_times.sort_by_key(|id| base_rank.get(&id).copied().unwrap_or(usize::MAX));

        teams = with_times
            .into_iter()
            .map(|(id, _)| id)
            .chain(without_times.into_iter())
            .collect();
    } else {
        // Intermediate rounds: preserve the initial draw order while filtering NT/DQ
        let base_order = load_round_order(&db.0, opts.event_id, 1).await?;
        if base_order.is_empty() {
            return Err(
                "La ronda 1 no ha sido generada. Genera el draw de la ronda 1 primero antes de generar rondas intermedias.".into()
            );
        } else {
            let active_set: HashSet<i64> = teams.iter().copied().collect();
            let mut seen: HashSet<i64> = HashSet::new();
            let mut ordered: Vec<i64> = Vec::with_capacity(active_set.len());

            for team_id in base_order.iter() {
                if active_set.contains(team_id) && seen.insert(*team_id) {
                    ordered.push(*team_id);
                }
            }

            if ordered.len() < active_set.len() {
                for team_id in teams.iter() {
                    if seen.insert(*team_id) {
                        ordered.push(*team_id);
                    }
                }
            }

            teams = ordered;
        }
    }

    let seed_runs = opts.seed_runs.unwrap_or(true);

    // 4) transacción: LIMPIAR ronda actual (si es seguro) y luego insertar
    let mut tx: Transaction<'_, Sqlite> = db.0.begin().await.map_err(|e| e.to_string())?;

    // Borramos runs y draw de esta ronda para asegurar que no queden "restos" de equipos eliminados (posiciones altas antiguas)
    sqlx::query("DELETE FROM run WHERE event_id = ?1 AND round = ?2")
        .bind(opts.event_id)
        .bind(opts.round)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM draw WHERE event_id = ?1 AND round = ?2")
        .bind(opts.event_id)
        .bind(opts.round)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    for (idx, team_id) in teams.iter().enumerate() {
        let position = (idx as i64) + 1;

        // draw insert (ya limpiamos, así que insert es seguro)
        sqlx::query(
            r#"
            INSERT INTO draw (event_id, round, position, team_id)
            VALUES (?1, ?2, ?3, ?4)
            "#,
        )
        .bind(opts.event_id)
        .bind(opts.round)
        .bind(position)
        .bind(team_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        if seed_runs {
            sqlx::query(
                r#"
                INSERT INTO run (event_id, team_id, round, position, time_sec, penalty, total_sec, no_time, dq, status)
                VALUES (?1, ?2, ?3, ?4, NULL, 0.0, NULL, 0, 0, 'pending')
                "#
            )
            .bind(opts.event_id)
            .bind(team_id)
            .bind(opts.round)
            .bind(position)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    tx.commit().await.map_err(|e| e.to_string())?;

    log_audit(
        &db.0,
        "generate_draw",
        "draw",
        None,
        Some(format!("Event {} Round {}", opts.event_id, opts.round)),
    )
    .await?;
    Ok(teams.len() as i64)
}

#[derive(serde::Deserialize)]
struct GenerateBatchDrawOptions {
    event_id: i64,
    rounds: i64,
    shuffle: bool,
}

#[tauri::command]
async fn generate_draw_batch(
    db: State<'_, Db>,
    opts: GenerateBatchDrawOptions,
) -> Result<i64, String> {
    db.require_license()?;
    ensure_event_unlocked(&db.0, opts.event_id).await?;

    // Get active teams with composition for smart shuffling (filtering eliminated)
    let teams: Vec<(i64, i64, i64)> = sqlx::query_as(
        r#"
        SELECT id, header_id, heeler_id FROM team 
        WHERE event_id = ?1 AND status = 'active'
          AND id NOT IN (
            SELECT team_id FROM run 
            WHERE event_id = ?1 AND (no_time = 1 OR dq = 1)
          )
        ORDER BY id ASC
        "#,
    )
    .bind(opts.event_id)
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    if teams.is_empty() {
        return Err("No hay equipos activos para generar el draw.".into());
    }

    let mut ordered_teams = teams.clone();
    if opts.shuffle {
        ordered_teams.shuffle(&mut thread_rng());

        let mut pool = ordered_teams.clone();
        let mut balanced: Vec<(i64, i64, i64)> = Vec::with_capacity(pool.len());

        while !pool.is_empty() {
            let mut best_idx = 0;
            let mut best_score = -1;

            for (i, candidate) in pool.iter().enumerate() {
                let mut min_distance = 999;

                for (distance, prev) in balanced.iter().rev().enumerate() {
                    let dist = distance + 1;
                    if prev.1 == candidate.1
                        || prev.1 == candidate.2
                        || prev.2 == candidate.1
                        || prev.2 == candidate.2
                    {
                        min_distance = dist;
                        break;
                    }
                }

                let score = min_distance as i64;
                if score > best_score {
                    best_score = score;
                    best_idx = i;
                    if score > 20 {
                        break;
                    }
                }
            }

            balanced.push(pool.remove(best_idx));
        }

        ordered_teams = balanced;
    }

    let frozen_sequence: Vec<i64> = ordered_teams.iter().map(|t| t.0).collect();

    let mut tx: Transaction<'_, Sqlite> = db.0.begin().await.map_err(|e| e.to_string())?;

    // For each round EXCEPT THE LAST ONE
    // The last round should be generated separately after all intermediate rounds are completed
    // so that ropers can be sorted by accumulated time (highest to lowest)
    let rounds_to_generate = if opts.rounds > 1 {
        opts.rounds - 1
    } else {
        opts.rounds
    };

    for r in 1..=rounds_to_generate {
        for (idx, team_id) in frozen_sequence.iter().enumerate() {
            let position = (idx as i64) + 1;

            // Insert into draw
            sqlx::query(
                r#"
                INSERT INTO draw (event_id, round, position, team_id)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(event_id, round, position) DO UPDATE SET
                  team_id = excluded.team_id
                "#,
            )
            .bind(opts.event_id)
            .bind(r)
            .bind(position)
            .bind(*team_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

            // Insert into run (pending)
            sqlx::query(
                r#"
                INSERT INTO run (event_id, team_id, round, position, time_sec, penalty, total_sec, no_time, dq, status)
                VALUES (?1, ?2, ?3, ?4, NULL, 0.0, NULL, 0, 0, 'pending')
                ON CONFLICT(event_id, round, team_id) DO UPDATE SET
                  position   = excluded.position,
                  updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                "#
            )
            .bind(opts.event_id)
            .bind(*team_id)
            .bind(r)
            .bind(position)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    tx.commit().await.map_err(|e| e.to_string())?;

    log_audit(
        &db.0,
        "generate_draw_batch",
        "draw",
        None,
        Some(format!(
            "Event {} Rounds 1-{} (Final round {} to be generated separately)",
            opts.event_id, rounds_to_generate, opts.rounds
        )),
    )
    .await?;
    Ok(teams.len() as i64 * rounds_to_generate)
}

/* ------------------- STANDINGS (LITE) ------------------- */
#[derive(serde::Serialize)]
struct StandingRow {
    rank: i64,
    team_id: i64,
    header_name: String,
    heeler_name: String,
    total_time: Option<f64>,
    completed_runs: i64,
    nt_cnt: i64,
    dq_cnt: i64,
    avg_time: Option<f64>,
    best_time: Option<f64>,
}

#[derive(sqlx::FromRow)]
struct StandingAgg {
    team_id: i64,
    header_name: String,
    heeler_name: String,
    total_time: Option<f64>,
    completed_runs: i64,
    nt_cnt: i64,
    dq_cnt: i64,
    avg_time: Option<f64>,
    best_time: Option<f64>,
}

fn compare_standing_agg(a: &StandingAgg, b: &StandingAgg) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let completed_runs = b.completed_runs.cmp(&a.completed_runs);
    if completed_runs != Ordering::Equal {
        return completed_runs;
    }

    match (&a.total_time, &b.total_time) {
        (Some(ta), Some(tb)) => {
            let total_time = ta.partial_cmp(tb).unwrap_or(Ordering::Equal);
            if total_time != Ordering::Equal {
                return total_time;
            }
        }
        (Some(_), None) => return Ordering::Less,
        (None, Some(_)) => return Ordering::Greater,
        (None, None) => {}
    }

    match (&a.best_time, &b.best_time) {
        (Some(ta), Some(tb)) => {
            let best_time = ta.partial_cmp(tb).unwrap_or(Ordering::Equal);
            if best_time != Ordering::Equal {
                return best_time;
            }
        }
        (Some(_), None) => return Ordering::Less,
        (None, Some(_)) => return Ordering::Greater,
        (None, None) => {}
    }

    a.team_id.cmp(&b.team_id)
}

async fn load_standing_aggs(pool: &SqlitePool, event_id: i64) -> Result<Vec<StandingAgg>, String> {
    sqlx::query_as::<_, StandingAgg>(
        r#"
        SELECT
          r.team_id                                        AS team_id,
          (rh.first_name || ' ' || rh.last_name)           AS header_name,
          (rhe.first_name || ' ' || rhe.last_name)         AS heeler_name,
          SUM(CASE WHEN r.status='completed' AND r.no_time=0 AND r.dq=0 THEN r.total_sec END) AS total_time,
          SUM(CASE WHEN r.status='completed' AND r.no_time=0 AND r.dq=0 THEN 1 ELSE 0 END)    AS completed_runs,
          SUM(CASE WHEN r.no_time=1 THEN 1 ELSE 0 END)                                       AS nt_cnt,
          SUM(CASE WHEN r.dq=1 THEN 1 ELSE 0 END)                                            AS dq_cnt,
          AVG(CASE WHEN r.status='completed' AND r.no_time=0 AND r.dq=0 THEN r.total_sec END) AS avg_time,
          MIN(CASE WHEN r.status='completed' AND r.no_time=0 AND r.dq=0 THEN r.total_sec END) AS best_time
        FROM run r
        JOIN team t ON r.team_id = t.id
        JOIN roper rh ON t.header_id = rh.id
        JOIN roper rhe ON t.heeler_id = rhe.id
        WHERE r.event_id = ?1
        GROUP BY r.team_id, header_name, heeler_name
        "#
    )
    .bind(event_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

async fn get_standings_internal(pool: &SqlitePool, event_id: i64) -> Result<Vec<StandingRow>, String> {
    let mut rows = load_standing_aggs(pool, event_id).await?;

    // Si no hay runs, regresamos vacío
    if rows.is_empty() {
        return Ok(vec![]);
    }

    rows.sort_by(compare_standing_agg);

    // Asigna rank (1-based).
    let standings: Vec<StandingRow> = rows
        .into_iter()
        .enumerate()
        .map(|(i, r)| StandingRow {
            rank: (i as i64) + 1,
            team_id: r.team_id,
            header_name: r.header_name,
            heeler_name: r.heeler_name,
            total_time: r.total_time,
            completed_runs: r.completed_runs,
            nt_cnt: r.nt_cnt,
            dq_cnt: r.dq_cnt,
            avg_time: r.avg_time,
            best_time: r.best_time,
        })
        .collect();

    Ok(standings)
}

#[tauri::command]
async fn get_standings(db: State<'_, Db>, event_id: i64) -> Result<Vec<StandingRow>, String> {
    db.require_license()?;
    get_standings_internal(&db.0, event_id).await
}

#[derive(sqlx::FromRow, Clone)]
struct ClosedSeriesEventRow {
    id: i64,
    name: String,
    teams_registered: i64,
}

#[derive(sqlx::FromRow, Clone)]
struct EventTeamDetailRow {
    team_id: i64,
    header_id: i64,
    heeler_id: i64,
    header_name: String,
    heeler_name: String,
    header_specialty: String,
    heeler_specialty: String,
}

#[derive(Clone)]
struct SeriesTeamPerformance {
    event_id: i64,
    event_name: String,
    header_id: i64,
    heeler_id: i64,
    header_name: String,
    heeler_name: String,
    header_specialty: String,
    heeler_specialty: String,
    finish_rank: i64,
    total_time: Option<f64>,
    completed_runs: i64,
    nt_cnt: i64,
    dq_cnt: i64,
    avg_time: Option<f64>,
    best_time: Option<f64>,
    team_payout: f64,
}

#[derive(serde::Serialize, Clone)]
struct SeriesSummaryTopRoper {
    roper_id: i64,
    name: String,
    avg_time: Option<f64>,
}

#[derive(serde::Serialize, Clone)]
struct SeriesResultsSummary {
    closed_events: i64,
    unique_ropers: i64,
    teams_registered: i64,
    valid_runs: i64,
    total_distributed: f64,
    avg_series_time: Option<f64>,
    clean_run_rate: Option<f64>,
    fastest_roper_name: Option<String>,
    fastest_avg_time: Option<f64>,
    most_wins_roper_name: Option<String>,
    most_wins_count: i64,
    top_ropers: Vec<SeriesSummaryTopRoper>,
}

#[derive(serde::Serialize, Clone)]
struct SeriesRoperRankingRow {
    roper_id: i64,
    roper_name: String,
    specialty: String,
    events_played: i64,
    partners_count: i64,
    valid_runs: i64,
    avg_time: Option<f64>,
    best_run: Option<f64>,
    wins: i64,
    podiums: i64,
    nt_count: i64,
    dq_count: i64,
    earnings: f64,
    rank: i64,
}

#[derive(serde::Serialize, Clone)]
struct SeriesRoperProfileHistoryEntry {
    event_id: i64,
    event_name: String,
    partner_name: String,
    finish_rank: Option<i64>,
    total_time: Option<f64>,
    avg_time: Option<f64>,
    earnings: f64,
}

#[derive(serde::Serialize, Clone)]
struct SeriesRoperProfile {
    roper_id: i64,
    roper_name: String,
    specialty: String,
    rank: i64,
    avg_time: Option<f64>,
    events_played: i64,
    wins: i64,
    podiums: i64,
    earnings: f64,
    best_partner_name: Option<String>,
    best_event_name: Option<String>,
    best_run: Option<f64>,
    history: Vec<SeriesRoperProfileHistoryEntry>,
}

struct RoperAccumulator {
    roper_id: i64,
    roper_name: String,
    specialty: String,
    event_ids: HashSet<i64>,
    partner_ids: HashSet<i64>,
    valid_runs: i64,
    total_time_sum: f64,
    best_run: Option<f64>,
    wins: i64,
    podiums: i64,
    nt_count: i64,
    dq_count: i64,
    earnings: f64,
    history: Vec<SeriesRoperProfileHistoryEntry>,
}

struct SeriesResultsDataset {
    summary: SeriesResultsSummary,
    rankings: Vec<SeriesRoperRankingRow>,
    profiles: HashMap<i64, SeriesRoperProfile>,
}

async fn load_closed_series_events(
    pool: &SqlitePool,
    series_id: i64,
) -> Result<Vec<ClosedSeriesEventRow>, String> {
    sqlx::query_as::<_, ClosedSeriesEventRow>(
        r#"
        SELECT
            e.id,
            e.name,
            (
                SELECT COUNT(*)
                FROM team t
                WHERE t.event_id = e.id AND t.status = 'active'
            ) AS teams_registered
        FROM event e
        WHERE e.series_id = ?1
          AND e.is_deleted = 0
          AND e.status IN ('completed', 'locked')
        ORDER BY e.date ASC, e.id ASC
        "#,
    )
    .bind(series_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

async fn load_event_team_details(
    pool: &SqlitePool,
    event_id: i64,
) -> Result<HashMap<i64, EventTeamDetailRow>, String> {
    let rows = sqlx::query_as::<_, EventTeamDetailRow>(
        r#"
        SELECT
            t.id AS team_id,
            t.header_id AS header_id,
            t.heeler_id AS heeler_id,
            (rh.first_name || ' ' || rh.last_name) AS header_name,
            (rhe.first_name || ' ' || rhe.last_name) AS heeler_name,
            rh.specialty AS header_specialty,
            rhe.specialty AS heeler_specialty
        FROM team t
        JOIN roper rh ON rh.id = t.header_id
        JOIN roper rhe ON rhe.id = t.heeler_id
        WHERE t.event_id = ?1
          AND t.status = 'active'
        "#,
    )
    .bind(event_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|row| (row.team_id, row))
        .collect::<HashMap<_, _>>())
}

fn update_best_time(current: Option<f64>, candidate: Option<f64>) -> Option<f64> {
    match (current, candidate) {
        (Some(current_value), Some(candidate_value)) => Some(current_value.min(candidate_value)),
        (None, Some(candidate_value)) => Some(candidate_value),
        (Some(current_value), None) => Some(current_value),
        (None, None) => None,
    }
}

fn compare_rankings(
    a: &SeriesRoperRankingRow,
    b: &SeriesRoperRankingRow,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    match (a.avg_time, b.avg_time) {
        (Some(a_time), Some(b_time)) => {
            let avg = a_time.partial_cmp(&b_time).unwrap_or(Ordering::Equal);
            if avg != Ordering::Equal {
                return avg;
            }
        }
        (Some(_), None) => return Ordering::Less,
        (None, Some(_)) => return Ordering::Greater,
        (None, None) => {}
    }

    let wins = b.wins.cmp(&a.wins);
    if wins != Ordering::Equal {
        return wins;
    }

    let podiums = b.podiums.cmp(&a.podiums);
    if podiums != Ordering::Equal {
        return podiums;
    }

    match (a.best_run, b.best_run) {
        (Some(a_run), Some(b_run)) => {
            let best = a_run.partial_cmp(&b_run).unwrap_or(Ordering::Equal);
            if best != Ordering::Equal {
                return best;
            }
        }
        (Some(_), None) => return Ordering::Less,
        (None, Some(_)) => return Ordering::Greater,
        (None, None) => {}
    }

    let earnings = b
        .earnings
        .partial_cmp(&a.earnings)
        .unwrap_or(Ordering::Equal);
    if earnings != Ordering::Equal {
        return earnings;
    }

    a.roper_id.cmp(&b.roper_id)
}

fn compare_history_entries(
    a: &SeriesRoperProfileHistoryEntry,
    b: &SeriesRoperProfileHistoryEntry,
) -> std::cmp::Ordering {
    let rank_a = a.finish_rank.unwrap_or(i64::MAX);
    let rank_b = b.finish_rank.unwrap_or(i64::MAX);
    rank_a
        .cmp(&rank_b)
        .then_with(|| match (a.avg_time, b.avg_time) {
            (Some(a_time), Some(b_time)) => a_time
                .partial_cmp(&b_time)
                .unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        })
        .then_with(|| {
            b.earnings
                .partial_cmp(&a.earnings)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn choose_best_partner(accumulator: &RoperAccumulator) -> Option<String> {
    accumulator
        .history
        .iter()
        .min_by(|a, b| compare_history_entries(a, b))
        .map(|entry| entry.partner_name.clone())
}

fn choose_best_event(accumulator: &RoperAccumulator) -> Option<String> {
    accumulator
        .history
        .iter()
        .min_by(|a, b| compare_history_entries(a, b))
        .map(|entry| entry.event_name.clone())
}

fn apply_roper_participation(
    accumulators: &mut HashMap<i64, RoperAccumulator>,
    performance: &SeriesTeamPerformance,
    roper_id: i64,
    roper_name: &str,
    specialty: &str,
    partner_id: i64,
    partner_name: &str,
    payout_share: f64,
) {
    let accumulator = accumulators.entry(roper_id).or_insert_with(|| RoperAccumulator {
        roper_id,
        roper_name: roper_name.to_string(),
        specialty: specialty.to_string(),
        event_ids: HashSet::new(),
        partner_ids: HashSet::new(),
        valid_runs: 0,
        total_time_sum: 0.0,
        best_run: None,
        wins: 0,
        podiums: 0,
        nt_count: 0,
        dq_count: 0,
        earnings: 0.0,
        history: Vec::new(),
    });

    accumulator.event_ids.insert(performance.event_id);
    accumulator.partner_ids.insert(partner_id);
    accumulator.valid_runs += performance.completed_runs;
    accumulator.total_time_sum += performance.total_time.unwrap_or(0.0);
    accumulator.best_run = update_best_time(accumulator.best_run, performance.best_time);
    accumulator.wins += if performance.finish_rank == 1 { 1 } else { 0 };
    accumulator.podiums += if performance.finish_rank <= 3 { 1 } else { 0 };
    accumulator.nt_count += performance.nt_cnt;
    accumulator.dq_count += performance.dq_cnt;
    accumulator.earnings += payout_share;
    accumulator.history.push(SeriesRoperProfileHistoryEntry {
        event_id: performance.event_id,
        event_name: performance.event_name.clone(),
        partner_name: partner_name.to_string(),
        finish_rank: Some(performance.finish_rank),
        total_time: performance.total_time,
        avg_time: performance.avg_time,
        earnings: payout_share,
    });
}

async fn build_series_results_dataset(
    pool: &SqlitePool,
    series_id: i64,
) -> Result<SeriesResultsDataset, String> {
    let closed_events = load_closed_series_events(pool, series_id).await?;
    let teams_registered = closed_events
        .iter()
        .map(|event| event.teams_registered)
        .sum::<i64>();

    let mut performances: Vec<SeriesTeamPerformance> = Vec::new();
    for event in &closed_events {
        let standings = get_standings_internal(pool, event.id).await?;
        if standings.is_empty() {
            continue;
        }
        let payout_breakdown = get_payout_breakdown_internal(pool, event.id).await?;
        let payout_map = payout_breakdown
            .payouts
            .into_iter()
            .map(|payout| (payout.place, payout.amount))
            .collect::<HashMap<_, _>>();
        let team_details = load_event_team_details(pool, event.id).await?;

        for standing in standings {
            if let Some(team_detail) = team_details.get(&standing.team_id) {
                performances.push(SeriesTeamPerformance {
                    event_id: event.id,
                    event_name: event.name.clone(),
                    header_id: team_detail.header_id,
                    heeler_id: team_detail.heeler_id,
                    header_name: team_detail.header_name.clone(),
                    heeler_name: team_detail.heeler_name.clone(),
                    header_specialty: team_detail.header_specialty.clone(),
                    heeler_specialty: team_detail.heeler_specialty.clone(),
                    finish_rank: standing.rank,
                    total_time: standing.total_time,
                    completed_runs: standing.completed_runs,
                    nt_cnt: standing.nt_cnt,
                    dq_cnt: standing.dq_cnt,
                    avg_time: standing.avg_time,
                    best_time: standing.best_time,
                    team_payout: *payout_map.get(&standing.rank).unwrap_or(&0.0),
                });
            }
        }
    }

    let mut accumulators: HashMap<i64, RoperAccumulator> = HashMap::new();
    let mut valid_runs = 0i64;
    let mut total_attempts = 0i64;
    let mut total_time_sum = 0.0f64;
    let mut total_distributed = 0.0f64;

    for performance in &performances {
        valid_runs += performance.completed_runs;
        total_attempts += performance.completed_runs + performance.nt_cnt + performance.dq_cnt;
        total_time_sum += performance.total_time.unwrap_or(0.0);
        total_distributed += performance.team_payout;

        let payout_share = performance.team_payout / 2.0;
        apply_roper_participation(
            &mut accumulators,
            performance,
            performance.header_id,
            &performance.header_name,
            &performance.header_specialty,
            performance.heeler_id,
            &performance.heeler_name,
            payout_share,
        );
        apply_roper_participation(
            &mut accumulators,
            performance,
            performance.heeler_id,
            &performance.heeler_name,
            &performance.heeler_specialty,
            performance.header_id,
            &performance.header_name,
            payout_share,
        );
    }

    let mut rankings = accumulators
        .values()
        .map(|accumulator| SeriesRoperRankingRow {
            roper_id: accumulator.roper_id,
            roper_name: accumulator.roper_name.clone(),
            specialty: accumulator.specialty.clone(),
            events_played: accumulator.event_ids.len() as i64,
            partners_count: accumulator.partner_ids.len() as i64,
            valid_runs: accumulator.valid_runs,
            avg_time: if accumulator.valid_runs > 0 {
                Some(accumulator.total_time_sum / accumulator.valid_runs as f64)
            } else {
                None
            },
            best_run: accumulator.best_run,
            wins: accumulator.wins,
            podiums: accumulator.podiums,
            nt_count: accumulator.nt_count,
            dq_count: accumulator.dq_count,
            earnings: accumulator.earnings,
            rank: 0,
        })
        .collect::<Vec<_>>();

    rankings.sort_by(compare_rankings);
    for (index, row) in rankings.iter_mut().enumerate() {
        row.rank = (index + 1) as i64;
    }

    let ranking_index = rankings
        .iter()
        .map(|row| (row.roper_id, row.clone()))
        .collect::<HashMap<_, _>>();

    let mut profiles = HashMap::new();
    for (roper_id, accumulator) in accumulators {
        let rank_row = ranking_index
            .get(&roper_id)
            .cloned()
            .ok_or_else(|| "No se pudo resolver el ranking del roper.".to_string())?;

        let mut history = accumulator.history.clone();
        history.sort_by(|a, b| {
            a.event_name
                .cmp(&b.event_name)
                .then_with(|| a.finish_rank.unwrap_or(i64::MAX).cmp(&b.finish_rank.unwrap_or(i64::MAX)))
        });

        profiles.insert(
            roper_id,
            SeriesRoperProfile {
                roper_id,
                roper_name: rank_row.roper_name.clone(),
                specialty: rank_row.specialty.clone(),
                rank: rank_row.rank,
                avg_time: rank_row.avg_time,
                events_played: rank_row.events_played,
                wins: rank_row.wins,
                podiums: rank_row.podiums,
                earnings: rank_row.earnings,
                best_partner_name: choose_best_partner(&accumulator),
                best_event_name: choose_best_event(&accumulator),
                best_run: rank_row.best_run,
                history,
            },
        );
    }

    let fastest = rankings.iter().find(|row| row.avg_time.is_some());
    let most_wins = rankings.iter().max_by_key(|row| row.wins);
    let summary = SeriesResultsSummary {
        closed_events: closed_events.len() as i64,
        unique_ropers: rankings.len() as i64,
        teams_registered,
        valid_runs,
        total_distributed,
        avg_series_time: if valid_runs > 0 {
            Some(total_time_sum / valid_runs as f64)
        } else {
            None
        },
        clean_run_rate: if total_attempts > 0 {
            Some((valid_runs as f64 / total_attempts as f64) * 100.0)
        } else {
            None
        },
        fastest_roper_name: fastest.map(|row| row.roper_name.clone()),
        fastest_avg_time: fastest.and_then(|row| row.avg_time),
        most_wins_roper_name: most_wins.map(|row| row.roper_name.clone()),
        most_wins_count: most_wins.map(|row| row.wins).unwrap_or(0),
        top_ropers: rankings
            .iter()
            .take(5)
            .map(|row| SeriesSummaryTopRoper {
                roper_id: row.roper_id,
                name: row.roper_name.clone(),
                avg_time: row.avg_time,
            })
            .collect(),
    };

    Ok(SeriesResultsDataset {
        summary,
        rankings,
        profiles,
    })
}

async fn get_series_results_summary_internal(
    pool: &SqlitePool,
    series_id: i64,
) -> Result<SeriesResultsSummary, String> {
    build_series_results_dataset(pool, series_id)
        .await
        .map(|dataset| dataset.summary)
}

async fn get_series_roper_rankings_internal(
    pool: &SqlitePool,
    series_id: i64,
) -> Result<Vec<SeriesRoperRankingRow>, String> {
    build_series_results_dataset(pool, series_id)
        .await
        .map(|dataset| dataset.rankings)
}

async fn get_series_roper_profile_internal(
    pool: &SqlitePool,
    series_id: i64,
    roper_id: i64,
) -> Result<Option<SeriesRoperProfile>, String> {
    build_series_results_dataset(pool, series_id)
        .await
        .map(|dataset| dataset.profiles.get(&roper_id).cloned())
}

#[tauri::command]
async fn get_series_results_summary(
    db: State<'_, Db>,
    series_id: i64,
) -> Result<SeriesResultsSummary, String> {
    db.require_license()?;
    get_series_results_summary_internal(&db.0, series_id).await
}

#[tauri::command]
async fn get_series_roper_rankings(
    db: State<'_, Db>,
    series_id: i64,
) -> Result<Vec<SeriesRoperRankingRow>, String> {
    db.require_license()?;
    get_series_roper_rankings_internal(&db.0, series_id).await
}

#[tauri::command]
async fn get_series_roper_profile(
    db: State<'_, Db>,
    series_id: i64,
    roper_id: i64,
) -> Result<Option<SeriesRoperProfile>, String> {
    db.require_license()?;
    get_series_roper_profile_internal(&db.0, series_id, roper_id).await
}

/* ------------------- DRAW READ ------------------- */
#[derive(serde::Serialize, sqlx::FromRow)]
struct DrawRow {
    id: i64,
    event_id: i64,
    round: i64,
    position: i64,
    team_id: i64,
    header_id: i64,
    heeler_id: i64,
}

#[tauri::command]
async fn get_draw(db: State<'_, Db>, event_id: i64, round: i64) -> Result<Vec<DrawRow>, String> {
    db.require_license()?;
    sqlx::query_as::<_, DrawRow>(
        r#"
        SELECT 
          d.id               AS id,
          d.event_id         AS event_id,
          d.round            AS round,
          d.position         AS position,
          d.team_id          AS team_id,
          t.header_id        AS header_id,
          t.heeler_id        AS heeler_id
        FROM draw d
        JOIN team t ON t.id = d.team_id
        WHERE d.event_id = ?1 AND d.round = ?2
        ORDER BY d.position ASC
        "#,
    )
    .bind(event_id)
    .bind(round)
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())
}

/* ------------------- DASHBOARD & ACTIVITY ------------------- */

#[derive(serde::Serialize, sqlx::FromRow)]
struct AuditLogItem {
    id: i64,
    action: String,
    entity_type: String,
    entity_id: Option<i64>,
    user_id: Option<i64>,
    metadata: Option<String>,
    created_at: String,
}

#[tauri::command]
async fn get_recent_activity(
    db: State<'_, Db>,
    limit: i64,
    offset: Option<i64>,
) -> Result<Vec<AuditLogItem>, String> {
    db.require_license()?;
    let off = offset.unwrap_or(0);
    sqlx::query_as::<_, AuditLogItem>(
        r#"
        SELECT id, action, entity_type, entity_id, user_id, metadata, created_at
        FROM audit_log
        ORDER BY created_at DESC
        LIMIT ?1 OFFSET ?2
        "#,
    )
    .bind(limit)
    .bind(off)
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct DashboardStats {
    total_series: i64,
    active_series: i64,
    total_events: i64,
    active_events: i64,
    completed_events: i64,
    upcoming_events: i64,
    locked_events: i64,
    total_teams: i64,
    total_pot: f64,
    upcoming_events_30d: i64,
    global_progress: f64,
}

#[tauri::command]
async fn get_dashboard_stats(db: State<'_, Db>) -> Result<DashboardStats, String> {
    db.require_license()?;
    let pool = &db.0;
    reconcile_all_future_runs(pool).await?;

    let total_series: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM series WHERE is_deleted = 0")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    let active_series: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM series WHERE is_deleted = 0 AND status = 'active'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    let total_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event WHERE is_deleted = 0")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    let active_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM event WHERE is_deleted = 0 AND status = 'active'")
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;

    let completed_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event WHERE is_deleted = 0 AND (status = 'completed' OR status = 'locked')")
        .fetch_one(pool).await.map_err(|e| e.to_string())?;

    let upcoming_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM event WHERE is_deleted = 0 AND status = 'upcoming'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    let locked_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM event WHERE is_deleted = 0 AND status = 'locked'")
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;

    let total_teams: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM team WHERE status = 'active'")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    // Calculate Total Pot: Sum of (entry_fee * unique_ropers) + prize_pool for all active/completed events
    let pot_opt: Option<f64> = sqlx::query_scalar(
        r#"
        SELECT SUM(
            COALESCE(e.prize_pool, 0) + 
            (COALESCE(e.entry_fee, 0) * (
                SELECT COUNT(DISTINCT roper_id) FROM (
                    SELECT header_id AS roper_id FROM team WHERE event_id = e.id AND status = 'active'
                    UNION
                    SELECT heeler_id AS roper_id FROM team WHERE event_id = e.id AND status = 'active'
                )
            ))
        )
        FROM event e
        WHERE e.is_deleted = 0 AND e.status IN ('active', 'completed', 'locked')
        "#
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    let total_pot = pot_opt.unwrap_or(0.0);

    // Upcoming events in next 30 days
    let upcoming_events_30d: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM event 
        WHERE is_deleted = 0 
          AND date >= date('now', 'localtime') 
          AND date <= date('now', '+30 days', 'localtime')
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    // Calculate Global Progress (Runs)
    let global_progress: f64 = sqlx::query_scalar(
        r#"
        SELECT 
            CASE WHEN COUNT(r.id) = 0 THEN 0.0
            ELSE CAST(SUM(CASE WHEN r.status IN ('completed', 'skipped') THEN 1 ELSE 0 END) AS REAL) / COUNT(r.id) * 100.0
            END
        FROM run r
        JOIN event e ON r.event_id = e.id
        WHERE e.is_deleted = 0
        "#
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(DashboardStats {
        total_series,
        active_series,
        total_events,
        active_events,
        completed_events,
        upcoming_events,
        locked_events,
        total_teams,
        total_pot,
        upcoming_events_30d,
        global_progress,
    })
}

/* ------------------- EVENT BACKUP ------------------- */
type BackupWorkbook = Sheets<BufReader<File>>;

const BACKUP_FORMAT: &str = "roping_event_backup";
const BACKUP_VERSION: i64 = 1;

#[derive(Debug, serde::Serialize)]
struct BackupInspection {
    format: String,
    version: i64,
    event_name: String,
    event_date: String,
    rounds: i64,
    ropers_count: usize,
    teams_count: usize,
    runs_count: usize,
    warnings: Vec<String>,
}

#[derive(serde::Deserialize)]
struct ImportEventBackupPayload {
    file_path: String,
    target_series_id: i64,
    restore_status_mode: Option<String>,
    dedupe_ropers: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
struct ImportEventBackupResult {
    event_id: i64,
    event_name: String,
    ropers_created: i64,
    ropers_reused: i64,
    teams_created: i64,
    runs_created: i64,
    warnings: Vec<String>,
}

#[derive(Clone)]
struct BackupManifestRow {
    format: String,
    version: i64,
    exported_at: String,
    app_version: String,
    event_id_original: i64,
    series_id_original: Option<i64>,
    event_name_original: String,
    checksum_mode: Option<String>,
}

#[derive(Clone)]
struct BackupEventMetaRow {
    name: String,
    date: String,
    status: String,
    rounds: i64,
    location: Option<String>,
    entry_fee: Option<f64>,
    prize_pool: Option<f64>,
    max_team_rating: Option<f64>,
    payoff_allocation: Option<String>,
    admin_pin: Option<String>,
    is_locked: Option<bool>,
}

#[derive(Clone)]
struct BackupRoperRow {
    backup_roper_key: String,
    first_name: String,
    last_name: String,
    specialty: String,
    rating: f64,
    phone: Option<String>,
    email: Option<String>,
    level: String,
    is_active: bool,
}

#[derive(Clone)]
struct BackupEventRosterSheetRow {
    backup_roper_key: String,
    status: String,
    rating_override: Option<f64>,
    notes: Option<String>,
    external_id: Option<String>,
    source_hash: Option<String>,
}

#[derive(Clone)]
struct BackupTeamRow {
    backup_team_key: String,
    header_roper_key: String,
    heeler_roper_key: String,
    team_rating: Option<f64>,
    status: String,
}

#[derive(Clone)]
struct BackupDrawRow {
    round: i64,
    position: i64,
    backup_team_key: String,
}

#[derive(Clone)]
struct BackupRunRow {
    round: i64,
    position: i64,
    backup_team_key: String,
    time_sec: Option<f64>,
    penalty: f64,
    total_sec: Option<f64>,
    no_time: bool,
    dq: bool,
    status: String,
    captured_at: Option<String>,
    captured_by: Option<String>,
}

#[derive(Clone)]
struct BackupPayoffRuleSheetRow {
    place: i64,
    percentage: f64,
}

#[derive(Clone)]
struct BackupPayoffSnapshotRow {
    place: i64,
    backup_team_key: Option<String>,
    amount: f64,
    per_person: Option<f64>,
}

struct ParsedEventBackup {
    manifest: BackupManifestRow,
    event_meta: BackupEventMetaRow,
    ropers: Vec<BackupRoperRow>,
    event_roster: Vec<BackupEventRosterSheetRow>,
    teams: Vec<BackupTeamRow>,
    draw: Vec<BackupDrawRow>,
    runs: Vec<BackupRunRow>,
    payoff_rules: Vec<BackupPayoffRuleSheetRow>,
    payoffs_snapshot: Vec<BackupPayoffSnapshotRow>,
    warnings: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct BackupEventExportRow {
    id: i64,
    series_id: i64,
    name: String,
    date: String,
    status: Option<String>,
    rounds: i64,
    location: Option<String>,
    entry_fee: Option<f64>,
    prize_pool: Option<f64>,
    max_team_rating: Option<f64>,
    payoff_allocation: Option<String>,
    admin_pin: Option<String>,
}

#[derive(sqlx::FromRow)]
struct BackupRoperExportRow {
    id: i64,
    first_name: String,
    last_name: String,
    specialty: String,
    rating: f64,
    phone: Option<String>,
    email: Option<String>,
    level: String,
    is_active: i64,
}

#[derive(sqlx::FromRow)]
struct BackupTeamExportRow {
    id: i64,
    header_id: i64,
    heeler_id: i64,
    rating: f64,
    status: String,
}

#[derive(sqlx::FromRow)]
struct BackupDrawExportRow {
    round: i64,
    position: i64,
    team_id: i64,
}

#[derive(sqlx::FromRow)]
struct BackupPayoffSnapshotExportRow {
    position: i64,
    team_id: i64,
    amount: f64,
}

fn backup_error(code: &str, detail: impl Into<String>) -> String {
    format!("{}: {}", code, detail.into())
}

fn write_headers(worksheet: &mut Worksheet, headers: &[&str]) -> Result<(), String> {
    for (idx, header) in headers.iter().enumerate() {
        worksheet
            .write_string(0, idx as u16, *header)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn workbook_sheet_names(workbook: &BackupWorkbook) -> HashSet<String> {
    workbook.sheet_names().iter().cloned().collect()
}

fn open_backup_workbook(file_path: &str) -> Result<BackupWorkbook, String> {
    open_workbook_auto(file_path).map_err(|e| {
        backup_error(
            "BACKUP_INVALID_FORMAT",
            format!("No se pudo abrir el XLSX de backup: {}", e),
        )
    })
}

fn required_sheet(
    workbook: &mut BackupWorkbook,
    sheet_name: &str,
) -> Result<calamine::Range<Data>, String> {
    if !workbook.sheet_names().iter().any(|name| name == sheet_name) {
        return Err(backup_error(
            "BACKUP_MISSING_SHEET",
            format!("Falta la hoja requerida '{}'", sheet_name),
        ));
    }
    match workbook.worksheet_range(sheet_name) {
        Ok(range) => Ok(range),
        Err(err) => Err(backup_error(
            "BACKUP_INVALID_FORMAT",
            format!("No se pudo leer la hoja '{}': {}", sheet_name, err),
        )),
    }
}

fn optional_sheet(
    workbook: &mut BackupWorkbook,
    sheet_name: &str,
) -> Result<Option<calamine::Range<Data>>, String> {
    if !workbook.sheet_names().iter().any(|name| name == sheet_name) {
        return Ok(None);
    }
    match workbook.worksheet_range(sheet_name) {
        Ok(range) => Ok(Some(range)),
        Err(err) => Err(backup_error(
            "BACKUP_INVALID_FORMAT",
            format!("No se pudo leer la hoja '{}': {}", sheet_name, err),
        )),
    }
}

fn row_has_values(row: &[Data]) -> bool {
    row.iter().any(|cell| !matches!(cell, Data::Empty))
}

fn cell_to_string(cell: &Data) -> Option<String> {
    if matches!(cell, Data::Empty) {
        return None;
    }
    let value = cell.to_string();
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn header_map(
    range: &calamine::Range<Data>,
    sheet_name: &str,
    required_columns: &[&str],
) -> Result<HashMap<String, usize>, String> {
    let header_row = range.rows().next().ok_or_else(|| {
        backup_error(
            "BACKUP_INVALID_FORMAT",
            format!("La hoja '{}' no contiene encabezados", sheet_name),
        )
    })?;

    let headers: HashMap<String, usize> = header_row
        .iter()
        .enumerate()
        .filter_map(|(idx, cell)| cell_to_string(cell).map(|value| (value, idx)))
        .collect();

    for column in required_columns {
        if !headers.contains_key(*column) {
            return Err(backup_error(
                "BACKUP_MISSING_COLUMN",
                format!("Falta la columna '{}' en la hoja '{}'", column, sheet_name),
            ));
        }
    }

    Ok(headers)
}

fn cell_at<'a>(
    row: &'a [Data],
    headers: &HashMap<String, usize>,
    column: &str,
) -> Option<&'a Data> {
    headers.get(column).and_then(|idx| row.get(*idx))
}

fn required_string(
    row: &[Data],
    headers: &HashMap<String, usize>,
    sheet_name: &str,
    row_number: usize,
    column: &str,
) -> Result<String, String> {
    cell_at(row, headers, column)
        .and_then(cell_to_string)
        .ok_or_else(|| {
            backup_error(
                "BACKUP_INVALID_VALUE",
                format!(
                    "La columna '{}' es obligatoria en la hoja '{}' (fila {})",
                    column, sheet_name, row_number
                ),
            )
        })
}

fn optional_string(
    row: &[Data],
    headers: &HashMap<String, usize>,
    column: &str,
) -> Option<String> {
    cell_at(row, headers, column).and_then(cell_to_string)
}

fn required_i64(
    row: &[Data],
    headers: &HashMap<String, usize>,
    sheet_name: &str,
    row_number: usize,
    column: &str,
) -> Result<i64, String> {
    let raw = required_string(row, headers, sheet_name, row_number, column)?;
    raw.parse::<i64>().map_err(|_| {
        backup_error(
            "BACKUP_INVALID_VALUE",
            format!(
                "La columna '{}' en la hoja '{}' (fila {}) debe ser entero",
                column, sheet_name, row_number
            ),
        )
    })
}

fn optional_i64(
    row: &[Data],
    headers: &HashMap<String, usize>,
    sheet_name: &str,
    row_number: usize,
    column: &str,
) -> Result<Option<i64>, String> {
    match optional_string(row, headers, column) {
        Some(raw) => raw.parse::<i64>().map(Some).map_err(|_| {
            backup_error(
                "BACKUP_INVALID_VALUE",
                format!(
                    "La columna '{}' en la hoja '{}' (fila {}) debe ser entero",
                    column, sheet_name, row_number
                ),
            )
        }),
        None => Ok(None),
    }
}

fn required_f64(
    row: &[Data],
    headers: &HashMap<String, usize>,
    sheet_name: &str,
    row_number: usize,
    column: &str,
) -> Result<f64, String> {
    let raw = required_string(row, headers, sheet_name, row_number, column)?;
    raw.parse::<f64>().map_err(|_| {
        backup_error(
            "BACKUP_INVALID_VALUE",
            format!(
                "La columna '{}' en la hoja '{}' (fila {}) debe ser numérica",
                column, sheet_name, row_number
            ),
        )
    })
}

fn optional_f64(
    row: &[Data],
    headers: &HashMap<String, usize>,
    sheet_name: &str,
    row_number: usize,
    column: &str,
) -> Result<Option<f64>, String> {
    match optional_string(row, headers, column) {
        Some(raw) => raw.parse::<f64>().map(Some).map_err(|_| {
            backup_error(
                "BACKUP_INVALID_VALUE",
                format!(
                    "La columna '{}' en la hoja '{}' (fila {}) debe ser numérica",
                    column, sheet_name, row_number
                ),
            )
        }),
        None => Ok(None),
    }
}

fn parse_bool_value(raw: &str) -> Option<bool> {
    match raw.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" | "y" | "si" | "sí" => Some(true),
        "false" | "0" | "no" | "n" => Some(false),
        _ => None,
    }
}

fn required_bool(
    row: &[Data],
    headers: &HashMap<String, usize>,
    sheet_name: &str,
    row_number: usize,
    column: &str,
) -> Result<bool, String> {
    let raw = required_string(row, headers, sheet_name, row_number, column)?;
    parse_bool_value(&raw).ok_or_else(|| {
        backup_error(
            "BACKUP_INVALID_VALUE",
            format!(
                "La columna '{}' en la hoja '{}' (fila {}) debe ser booleana",
                column, sheet_name, row_number
            ),
        )
    })
}

fn optional_bool(
    row: &[Data],
    headers: &HashMap<String, usize>,
    sheet_name: &str,
    row_number: usize,
    column: &str,
) -> Result<Option<bool>, String> {
    match optional_string(row, headers, column) {
        Some(raw) => parse_bool_value(&raw).map(Some).ok_or_else(|| {
            backup_error(
                "BACKUP_INVALID_VALUE",
                format!(
                    "La columna '{}' en la hoja '{}' (fila {}) debe ser booleana",
                    column, sheet_name, row_number
                ),
            )
        }),
        None => Ok(None),
    }
}

fn validate_in_enum(
    value: &str,
    allowed: &[&str],
    sheet_name: &str,
    row_number: usize,
    column: &str,
) -> Result<(), String> {
    if allowed.iter().any(|candidate| candidate == &value) {
        Ok(())
    } else {
        Err(backup_error(
            "BACKUP_INVALID_VALUE",
            format!(
                "Valor '{}' inválido en '{}.{}' (fila {}). Permitidos: {}",
                value,
                sheet_name,
                column,
                row_number,
                allowed.join(", ")
            ),
        ))
    }
}

fn backup_roper_key(index: usize) -> String {
    format!("backup_roper_{:04}", index + 1)
}

fn backup_team_key(index: usize) -> String {
    format!("backup_team_{:04}", index + 1)
}

fn resolve_import_status(meta: &BackupEventMetaRow, mode: &str) -> Result<String, String> {
    match mode {
        "preserve" => {
            if meta.is_locked == Some(true) {
                Ok("locked".to_string())
            } else {
                Ok(meta.status.clone())
            }
        }
        "force_upcoming" => Ok("upcoming".to_string()),
        "force_locked" => Ok("locked".to_string()),
        other => Err(backup_error(
            "BACKUP_INVALID_VALUE",
            format!("restore_status_mode '{}' no es válido", other),
        )),
    }
}

fn compute_total_from_backup_run(run: &BackupRunRow) -> Option<f64> {
    if run.no_time || run.dq || run.status != "completed" {
        None
    } else {
        run.time_sec.map(|value| value + run.penalty)
    }
}

fn parse_manifest(range: &calamine::Range<Data>) -> Result<BackupManifestRow, String> {
    let headers = header_map(
        range,
        "manifest",
        &[
            "format",
            "version",
            "exported_at",
            "app_version",
            "event_id_original",
            "event_name_original",
        ],
    )?;
    let data_row = range
        .rows()
        .skip(1)
        .find(|row| row_has_values(row))
        .ok_or_else(|| backup_error("BACKUP_INVALID_FORMAT", "La hoja 'manifest' no contiene datos"))?;

    let format = required_string(data_row, &headers, "manifest", 2, "format")?;
    let version = required_i64(data_row, &headers, "manifest", 2, "version")?;

    if format != BACKUP_FORMAT {
        return Err(backup_error(
            "BACKUP_INVALID_FORMAT",
            format!("Se esperaba format='{}' y llegó '{}'", BACKUP_FORMAT, format),
        ));
    }
    if version != BACKUP_VERSION {
        return Err(backup_error(
            "BACKUP_UNSUPPORTED_VERSION",
            format!(
                "La versión '{}' no es compatible. Versión soportada: {}",
                version, BACKUP_VERSION
            ),
        ));
    }

    Ok(BackupManifestRow {
        format,
        version,
        exported_at: required_string(data_row, &headers, "manifest", 2, "exported_at")?,
        app_version: required_string(data_row, &headers, "manifest", 2, "app_version")?,
        event_id_original: required_i64(data_row, &headers, "manifest", 2, "event_id_original")?,
        series_id_original: optional_i64(data_row, &headers, "manifest", 2, "series_id_original")?,
        event_name_original: required_string(
            data_row,
            &headers,
            "manifest",
            2,
            "event_name_original",
        )?,
        checksum_mode: optional_string(data_row, &headers, "checksum_mode"),
    })
}

fn parse_event_meta(range: &calamine::Range<Data>) -> Result<BackupEventMetaRow, String> {
    let headers = header_map(
        range,
        "event_meta",
        &["name", "date", "status", "rounds"],
    )?;
    let data_row = range
        .rows()
        .skip(1)
        .find(|row| row_has_values(row))
        .ok_or_else(|| backup_error("BACKUP_INVALID_FORMAT", "La hoja 'event_meta' no contiene datos"))?;

    let status = required_string(data_row, &headers, "event_meta", 2, "status")?;
    validate_in_enum(
        &status,
        &["active", "upcoming", "completed", "locked"],
        "event_meta",
        2,
        "status",
    )?;

    let rounds = required_i64(data_row, &headers, "event_meta", 2, "rounds")?;
    if !(1..=10).contains(&rounds) {
        return Err(backup_error(
            "BACKUP_INVALID_VALUE",
            format!("event_meta.rounds debe estar entre 1 y 10, llegó {}", rounds),
        ));
    }

    let admin_pin = optional_string(data_row, &headers, "admin_pin");
    if let Some(pin) = admin_pin.as_ref() {
        if !pin.chars().all(|char| char.is_ascii_digit()) || pin.len() != 4 {
            return Err(backup_error(
                "BACKUP_INVALID_VALUE",
                "event_meta.admin_pin debe contener exactamente 4 dígitos",
            ));
        }
    }

    Ok(BackupEventMetaRow {
        name: required_string(data_row, &headers, "event_meta", 2, "name")?,
        date: required_string(data_row, &headers, "event_meta", 2, "date")?,
        status,
        rounds,
        location: optional_string(data_row, &headers, "location"),
        entry_fee: optional_f64(data_row, &headers, "event_meta", 2, "entry_fee")?,
        prize_pool: optional_f64(data_row, &headers, "event_meta", 2, "prize_pool")?,
        max_team_rating: optional_f64(
            data_row,
            &headers,
            "event_meta",
            2,
            "max_team_rating",
        )?,
        payoff_allocation: optional_string(data_row, &headers, "payoff_allocation"),
        admin_pin,
        is_locked: optional_bool(data_row, &headers, "event_meta", 2, "is_locked")?,
    })
}

fn parse_ropers(range: &calamine::Range<Data>) -> Result<Vec<BackupRoperRow>, String> {
    let headers = header_map(
        range,
        "ropers",
        &[
            "backup_roper_key",
            "first_name",
            "last_name",
            "specialty",
            "rating",
            "level",
            "is_active",
        ],
    )?;

    let mut rows = Vec::new();
    let mut keys = HashSet::new();

    for (index, row) in range.rows().skip(1).enumerate() {
        if !row_has_values(row) {
            continue;
        }
        let row_number = index + 2;
        let backup_roper_key =
            required_string(row, &headers, "ropers", row_number, "backup_roper_key")?;
        if !keys.insert(backup_roper_key.clone()) {
            return Err(backup_error(
                "BACKUP_DUPLICATE_KEY",
                format!("backup_roper_key duplicado '{}'", backup_roper_key),
            ));
        }

        let specialty = required_string(row, &headers, "ropers", row_number, "specialty")?;
        validate_in_enum(
            &specialty,
            &["header", "heeler", "both"],
            "ropers",
            row_number,
            "specialty",
        )?;

        let level = required_string(row, &headers, "ropers", row_number, "level")?;
        validate_in_enum(
            &level,
            &["pro", "amateur", "principiante"],
            "ropers",
            row_number,
            "level",
        )?;

        rows.push(BackupRoperRow {
            backup_roper_key,
            first_name: required_string(row, &headers, "ropers", row_number, "first_name")?,
            last_name: required_string(row, &headers, "ropers", row_number, "last_name")?,
            specialty,
            rating: required_f64(row, &headers, "ropers", row_number, "rating")?,
            phone: optional_string(row, &headers, "phone"),
            email: optional_string(row, &headers, "email"),
            level,
            is_active: required_bool(row, &headers, "ropers", row_number, "is_active")?,
        });
    }

    Ok(rows)
}

fn parse_event_roster_sheet(
    range: &calamine::Range<Data>,
) -> Result<Vec<BackupEventRosterSheetRow>, String> {
    let headers = header_map(
        range,
        "event_roster",
        &["backup_roper_key", "status"],
    )?;
    let mut rows = Vec::new();
    let mut seen = HashSet::new();

    for (index, row) in range.rows().skip(1).enumerate() {
        if !row_has_values(row) {
            continue;
        }
        let row_number = index + 2;
        let backup_roper_key =
            required_string(row, &headers, "event_roster", row_number, "backup_roper_key")?;
        if !seen.insert(backup_roper_key.clone()) {
            return Err(backup_error(
                "BACKUP_DUPLICATE_KEY",
                format!(
                    "backup_roper_key duplicado '{}' en hoja event_roster",
                    backup_roper_key
                ),
            ));
        }
        let status = required_string(row, &headers, "event_roster", row_number, "status")?;
        validate_in_enum(
            &status,
            &["registered", "confirmed", "withdrawn"],
            "event_roster",
            row_number,
            "status",
        )?;

        rows.push(BackupEventRosterSheetRow {
            backup_roper_key,
            status,
            rating_override: optional_f64(
                row,
                &headers,
                "event_roster",
                row_number,
                "rating_override",
            )?,
            notes: optional_string(row, &headers, "notes"),
            external_id: optional_string(row, &headers, "external_id"),
            source_hash: optional_string(row, &headers, "source_hash"),
        });
    }

    Ok(rows)
}

fn parse_teams(range: &calamine::Range<Data>) -> Result<Vec<BackupTeamRow>, String> {
    let headers = header_map(
        range,
        "teams",
        &[
            "backup_team_key",
            "header_roper_key",
            "heeler_roper_key",
            "status",
        ],
    )?;
    let mut rows = Vec::new();
    let mut keys = HashSet::new();

    for (index, row) in range.rows().skip(1).enumerate() {
        if !row_has_values(row) {
            continue;
        }
        let row_number = index + 2;
        let backup_team_key =
            required_string(row, &headers, "teams", row_number, "backup_team_key")?;
        if !keys.insert(backup_team_key.clone()) {
            return Err(backup_error(
                "BACKUP_DUPLICATE_KEY",
                format!("backup_team_key duplicado '{}'", backup_team_key),
            ));
        }
        let status = required_string(row, &headers, "teams", row_number, "status")?;
        validate_in_enum(
            &status,
            &["active", "inactive"],
            "teams",
            row_number,
            "status",
        )?;
        let header_roper_key =
            required_string(row, &headers, "teams", row_number, "header_roper_key")?;
        let heeler_roper_key =
            required_string(row, &headers, "teams", row_number, "heeler_roper_key")?;
        if header_roper_key == heeler_roper_key {
            return Err(backup_error(
                "BACKUP_INVALID_VALUE",
                format!(
                    "El equipo '{}' no puede tener el mismo roper como header y heeler",
                    backup_team_key
                ),
            ));
        }
        rows.push(BackupTeamRow {
            backup_team_key,
            header_roper_key,
            heeler_roper_key,
            team_rating: optional_f64(row, &headers, "teams", row_number, "team_rating")?,
            status,
        });
    }

    Ok(rows)
}

fn parse_draw_sheet(range: &calamine::Range<Data>) -> Result<Vec<BackupDrawRow>, String> {
    let headers = header_map(range, "draw", &["round", "position", "backup_team_key"])?;
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for (index, row) in range.rows().skip(1).enumerate() {
        if !row_has_values(row) {
            continue;
        }
        let row_number = index + 2;
        let round = required_i64(row, &headers, "draw", row_number, "round")?;
        let position = required_i64(row, &headers, "draw", row_number, "position")?;
        let key = (round, position);
        if !seen.insert(key) {
            return Err(backup_error(
                "BACKUP_DUPLICATE_KEY",
                format!("draw duplicado para round={} position={}", round, position),
            ));
        }
        rows.push(BackupDrawRow {
            round,
            position,
            backup_team_key: required_string(
                row,
                &headers,
                "draw",
                row_number,
                "backup_team_key",
            )?,
        });
    }
    Ok(rows)
}

fn parse_runs_sheet(range: &calamine::Range<Data>) -> Result<Vec<BackupRunRow>, String> {
    let headers = header_map(
        range,
        "runs",
        &[
            "round",
            "position",
            "backup_team_key",
            "penalty",
            "no_time",
            "dq",
            "status",
        ],
    )?;

    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for (index, row) in range.rows().skip(1).enumerate() {
        if !row_has_values(row) {
            continue;
        }
        let row_number = index + 2;
        let round = required_i64(row, &headers, "runs", row_number, "round")?;
        let position = required_i64(row, &headers, "runs", row_number, "position")?;
        let backup_team_key =
            required_string(row, &headers, "runs", row_number, "backup_team_key")?;
        let unique_key = (round, position, backup_team_key.clone());
        if !seen.insert(unique_key) {
            return Err(backup_error(
                "BACKUP_DUPLICATE_KEY",
                format!(
                    "run duplicado para round={} position={} team={}",
                    round, position, backup_team_key
                ),
            ));
        }
        let status = required_string(row, &headers, "runs", row_number, "status")?;
        validate_in_enum(
            &status,
            &["pending", "completed", "skipped"],
            "runs",
            row_number,
            "status",
        )?;
        let no_time = required_bool(row, &headers, "runs", row_number, "no_time")?;
        let dq = required_bool(row, &headers, "runs", row_number, "dq")?;
        let time_sec = optional_f64(row, &headers, "runs", row_number, "time_sec")?;
        if status == "completed" && !no_time && !dq && time_sec.is_none() {
            return Err(backup_error(
                "BACKUP_INVALID_VALUE",
                format!(
                    "El run round={} position={} requiere time_sec al estar completed",
                    round, position
                ),
            ));
        }
        rows.push(BackupRunRow {
            round,
            position,
            backup_team_key,
            time_sec,
            penalty: required_f64(row, &headers, "runs", row_number, "penalty")?,
            total_sec: optional_f64(row, &headers, "runs", row_number, "total_sec")?,
            no_time,
            dq,
            status,
            captured_at: optional_string(row, &headers, "captured_at"),
            captured_by: optional_string(row, &headers, "captured_by"),
        });
    }
    Ok(rows)
}

fn parse_payoff_rules_sheet(
    range: &calamine::Range<Data>,
) -> Result<Vec<BackupPayoffRuleSheetRow>, String> {
    let headers = header_map(range, "payoff_rules", &["place", "percentage"])?;
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for (index, row) in range.rows().skip(1).enumerate() {
        if !row_has_values(row) {
            continue;
        }
        let row_number = index + 2;
        let place = required_i64(row, &headers, "payoff_rules", row_number, "place")?;
        if !seen.insert(place) {
            return Err(backup_error(
                "BACKUP_DUPLICATE_KEY",
                format!("payoff_rules.place duplicado '{}'", place),
            ));
        }
        rows.push(BackupPayoffRuleSheetRow {
            place,
            percentage: required_f64(row, &headers, "payoff_rules", row_number, "percentage")?,
        });
    }
    Ok(rows)
}

fn parse_payoffs_snapshot_sheet(
    range: &calamine::Range<Data>,
) -> Result<Vec<BackupPayoffSnapshotRow>, String> {
    let headers = header_map(range, "payoffs_snapshot", &["place", "amount"])?;
    let mut rows = Vec::new();
    for (index, row) in range.rows().skip(1).enumerate() {
        if !row_has_values(row) {
            continue;
        }
        let row_number = index + 2;
        rows.push(BackupPayoffSnapshotRow {
            place: required_i64(row, &headers, "payoffs_snapshot", row_number, "place")?,
            backup_team_key: optional_string(row, &headers, "backup_team_key"),
            amount: required_f64(row, &headers, "payoffs_snapshot", row_number, "amount")?,
            per_person: optional_f64(
                row,
                &headers,
                "payoffs_snapshot",
                row_number,
                "per_person",
            )?,
        });
    }
    Ok(rows)
}

fn validate_parsed_backup(backup: &ParsedEventBackup) -> Result<(), String> {
    if backup.event_meta.is_locked == Some(true) && backup.event_meta.status != "locked" {
        return Err(backup_error(
            "BACKUP_INVALID_VALUE",
            "event_meta.is_locked=true requiere status='locked'",
        ));
    }

    let roper_keys: HashSet<&str> = backup
        .ropers
        .iter()
        .map(|row| row.backup_roper_key.as_str())
        .collect();
    let team_keys: HashSet<&str> = backup
        .teams
        .iter()
        .map(|row| row.backup_team_key.as_str())
        .collect();

    for row in &backup.event_roster {
        if !roper_keys.contains(row.backup_roper_key.as_str()) {
            return Err(backup_error(
                "BACKUP_BROKEN_REFERENCE",
                format!(
                    "event_roster referencia backup_roper_key inexistente '{}'",
                    row.backup_roper_key
                ),
            ));
        }
    }

    for row in &backup.teams {
        if !roper_keys.contains(row.header_roper_key.as_str()) {
            return Err(backup_error(
                "BACKUP_BROKEN_REFERENCE",
                format!(
                    "team '{}' referencia header_roper_key inexistente '{}'",
                    row.backup_team_key, row.header_roper_key
                ),
            ));
        }
        if !roper_keys.contains(row.heeler_roper_key.as_str()) {
            return Err(backup_error(
                "BACKUP_BROKEN_REFERENCE",
                format!(
                    "team '{}' referencia heeler_roper_key inexistente '{}'",
                    row.backup_team_key, row.heeler_roper_key
                ),
            ));
        }
    }

    for row in &backup.draw {
        if row.round < 1 || row.round > backup.event_meta.rounds {
            return Err(backup_error(
                "BACKUP_INVALID_VALUE",
                format!("draw.round={} está fuera del rango 1..={}", row.round, backup.event_meta.rounds),
            ));
        }
        if row.position < 1 {
            return Err(backup_error(
                "BACKUP_INVALID_VALUE",
                format!("draw.position={} debe ser >= 1", row.position),
            ));
        }
        if !team_keys.contains(row.backup_team_key.as_str()) {
            return Err(backup_error(
                "BACKUP_BROKEN_REFERENCE",
                format!(
                    "draw referencia backup_team_key inexistente '{}'",
                    row.backup_team_key
                ),
            ));
        }
    }

    for row in &backup.runs {
        if row.round < 1 || row.round > backup.event_meta.rounds {
            return Err(backup_error(
                "BACKUP_INVALID_VALUE",
                format!("runs.round={} está fuera del rango 1..={}", row.round, backup.event_meta.rounds),
            ));
        }
        if row.position < 1 {
            return Err(backup_error(
                "BACKUP_INVALID_VALUE",
                format!("runs.position={} debe ser >= 1", row.position),
            ));
        }
        if !team_keys.contains(row.backup_team_key.as_str()) {
            return Err(backup_error(
                "BACKUP_BROKEN_REFERENCE",
                format!(
                    "runs referencia backup_team_key inexistente '{}'",
                    row.backup_team_key
                ),
            ));
        }
    }

    for row in &backup.payoff_rules {
        if row.place < 1 {
            return Err(backup_error(
                "BACKUP_INVALID_VALUE",
                format!("payoff_rules.place={} debe ser >= 1", row.place),
            ));
        }
        if !(0.0..=1.0).contains(&row.percentage) {
            return Err(backup_error(
                "BACKUP_INVALID_VALUE",
                format!(
                    "payoff_rules.percentage={} debe estar entre 0.0 y 1.0",
                    row.percentage
                ),
            ));
        }
    }

    for row in &backup.payoffs_snapshot {
        if row.place < 1 {
            return Err(backup_error(
                "BACKUP_INVALID_VALUE",
                format!("payoffs_snapshot.place={} debe ser >= 1", row.place),
            ));
        }
        if row.amount < 0.0 {
            return Err(backup_error(
                "BACKUP_INVALID_VALUE",
                format!("payoffs_snapshot.amount={} debe ser >= 0.0", row.amount),
            ));
        }
        if let Some(per_person) = row.per_person {
            if per_person < 0.0 {
                return Err(backup_error(
                    "BACKUP_INVALID_VALUE",
                    format!(
                        "payoffs_snapshot.per_person={} debe ser >= 0.0",
                        per_person
                    ),
                ));
            }
        }
        if let Some(team_key) = row.backup_team_key.as_ref() {
            if !team_keys.contains(team_key.as_str()) {
                return Err(backup_error(
                    "BACKUP_BROKEN_REFERENCE",
                    format!(
                        "payoffs_snapshot referencia backup_team_key inexistente '{}'",
                        team_key
                    ),
                ));
            }
        }
    }

    Ok(())
}

fn parse_event_backup(file_path: &str) -> Result<ParsedEventBackup, String> {
    let mut workbook = open_backup_workbook(file_path)?;
    let sheet_names = workbook_sheet_names(&workbook);

    for required in ["manifest", "event_meta", "ropers", "teams", "runs"] {
        if !sheet_names.contains(required) {
            return Err(backup_error(
                "BACKUP_MISSING_SHEET",
                format!("Falta la hoja requerida '{}'", required),
            ));
        }
    }

    let manifest = parse_manifest(&required_sheet(&mut workbook, "manifest")?)?;
    let event_meta = parse_event_meta(&required_sheet(&mut workbook, "event_meta")?)?;
    let ropers = parse_ropers(&required_sheet(&mut workbook, "ropers")?)?;
    let teams = parse_teams(&required_sheet(&mut workbook, "teams")?)?;
    let runs = parse_runs_sheet(&required_sheet(&mut workbook, "runs")?)?;
    let event_roster = match optional_sheet(&mut workbook, "event_roster")? {
        Some(range) => parse_event_roster_sheet(&range)?,
        None => Vec::new(),
    };
    let draw = match optional_sheet(&mut workbook, "draw")? {
        Some(range) => parse_draw_sheet(&range)?,
        None => Vec::new(),
    };
    let payoff_rules = match optional_sheet(&mut workbook, "payoff_rules")? {
        Some(range) => parse_payoff_rules_sheet(&range)?,
        None => Vec::new(),
    };
    let payoffs_snapshot = match optional_sheet(&mut workbook, "payoffs_snapshot")? {
        Some(range) => parse_payoffs_snapshot_sheet(&range)?,
        None => Vec::new(),
    };

    let mut warnings = Vec::new();

    if manifest
        .checksum_mode
        .as_deref()
        .is_some_and(|mode| mode != "none")
    {
        warnings.push(format!(
            "checksum_mode='{}' es informativo en v1 y no se valida durante la restauración",
            manifest.checksum_mode.as_deref().unwrap_or("none")
        ));
    }

    if event_roster.iter().any(|row| row.external_id.is_some()) {
        warnings.push(
            "event_roster.external_id es informativo en v1 y no modifica la restauración"
                .to_string(),
        );
    }

    if runs
        .iter()
        .any(|row| row.captured_at.is_some() || row.captured_by.is_some())
    {
        warnings.push(
            "runs.captured_at y runs.captured_by se conservan en el backup solo como referencia"
                .to_string(),
        );
    }

    if !payoffs_snapshot.is_empty() {
        warnings.push(
            "payoffs_snapshot es informativo y no se usa como fuente de verdad al restaurar"
                .to_string(),
        );
    }

    let parsed = ParsedEventBackup {
        manifest,
        event_meta,
        ropers,
        event_roster,
        teams,
        draw,
        runs,
        payoff_rules,
        payoffs_snapshot,
        warnings,
    };

    validate_parsed_backup(&parsed)?;
    Ok(parsed)
}

async fn export_event_backup_impl(db: &Db, event_id: i64, file_path: &str) -> Result<(), String> {
    let event: BackupEventExportRow = sqlx::query_as(
        r#"
        SELECT
            id,
            series_id,
            name,
            date,
            status,
            rounds,
            location,
            entry_fee,
            prize_pool,
            max_team_rating,
            payoff_allocation,
            admin_pin
        FROM event
        WHERE id = ?1 AND is_deleted = 0
        "#,
    )
    .bind(event_id)
    .fetch_one(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    let ropers: Vec<BackupRoperExportRow> = sqlx::query_as(
        r#"
        SELECT DISTINCT
            r.id,
            r.first_name,
            r.last_name,
            r.specialty,
            CAST(r.rating AS REAL) AS rating,
            r.phone,
            r.email,
            r.level,
            r.is_active
        FROM roper r
        WHERE r.id IN (
            SELECT header_id FROM team WHERE event_id = ?1
            UNION
            SELECT heeler_id FROM team WHERE event_id = ?1
            UNION
            SELECT roper_id FROM event_roster WHERE event_id = ?1
        )
        ORDER BY r.last_name ASC, r.first_name ASC, r.id ASC
        "#,
    )
    .bind(event_id)
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    let teams: Vec<BackupTeamExportRow> = sqlx::query_as(
        r#"
        SELECT id, header_id, heeler_id, rating, status
        FROM team
        WHERE event_id = ?1
        ORDER BY id ASC
        "#,
    )
    .bind(event_id)
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    let draw_rows: Vec<BackupDrawExportRow> = sqlx::query_as(
        r#"
        SELECT round, position, team_id
        FROM draw
        WHERE event_id = ?1
        ORDER BY round ASC, position ASC
        "#,
    )
    .bind(event_id)
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    let runs: Vec<RunRow> = sqlx::query_as(
        r#"
        SELECT id, event_id, team_id, round, position, time_sec, penalty, total_sec,
               no_time, dq, status, captured_by, created_at, updated_at
        FROM run
        WHERE event_id = ?1
        ORDER BY round ASC, position ASC, id ASC
        "#,
    )
    .bind(event_id)
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    let roster_rows: Vec<(
        i64,
        String,
        Option<f64>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        r#"
        SELECT
            roper_id,
            status,
            rating_override,
            notes,
            source_hash,
            (SELECT external_id FROM roper WHERE id = event_roster.roper_id) AS external_id
        FROM event_roster
        WHERE event_id = ?1
        ORDER BY roper_id ASC
        "#,
    )
    .bind(event_id)
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    let payoff_rules: Vec<PayoffRuleRow> = sqlx::query_as(
        r#"
        SELECT id, event_id, position, percentage, is_active, created_at
        FROM payoff_rule
        WHERE event_id = ?1 AND is_active = 1
        ORDER BY position ASC
        "#,
    )
    .bind(event_id)
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    let payoff_snapshot: Vec<BackupPayoffSnapshotExportRow> = sqlx::query_as(
        r#"
        SELECT position, team_id, amount
        FROM payoff
        WHERE event_id = ?1
        ORDER BY position ASC
        "#,
    )
    .bind(event_id)
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    let mut roper_key_by_id = HashMap::new();
    for (index, roper) in ropers.iter().enumerate() {
        roper_key_by_id.insert(roper.id, backup_roper_key(index));
    }

    let mut team_key_by_id = HashMap::new();
    for (index, team) in teams.iter().enumerate() {
        team_key_by_id.insert(team.id, backup_team_key(index));
    }

    let mut workbook = Workbook::new();

    {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("manifest").map_err(|e| e.to_string())?;
        write_headers(
            worksheet,
            &[
                "format",
                "version",
                "exported_at",
                "app_version",
                "event_id_original",
                "series_id_original",
                "event_name_original",
                "checksum_mode",
            ],
        )?;
        worksheet.write_string(1, 0, BACKUP_FORMAT).map_err(|e| e.to_string())?;
        worksheet
            .write_number(1, 1, BACKUP_VERSION as f64)
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(1, 2, &Utc::now().to_rfc3339())
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(1, 3, env!("CARGO_PKG_VERSION"))
            .map_err(|e| e.to_string())?;
        worksheet
            .write_number(1, 4, event.id as f64)
            .map_err(|e| e.to_string())?;
        worksheet
            .write_number(1, 5, event.series_id as f64)
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(1, 6, &event.name)
            .map_err(|e| e.to_string())?;
        worksheet.write_string(1, 7, "none").map_err(|e| e.to_string())?;
    }

    {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("event_meta").map_err(|e| e.to_string())?;
        write_headers(
            worksheet,
            &[
                "name",
                "date",
                "status",
                "rounds",
                "location",
                "entry_fee",
                "prize_pool",
                "max_team_rating",
                "payoff_allocation",
                "admin_pin",
                "is_locked",
            ],
        )?;
        worksheet.write_string(1, 0, &event.name).map_err(|e| e.to_string())?;
        worksheet.write_string(1, 1, &event.date).map_err(|e| e.to_string())?;
        worksheet
            .write_string(1, 2, event.status.as_deref().unwrap_or("upcoming"))
            .map_err(|e| e.to_string())?;
        worksheet
            .write_number(1, 3, event.rounds as f64)
            .map_err(|e| e.to_string())?;
        if let Some(location) = event.location.as_ref() {
            worksheet.write_string(1, 4, location).map_err(|e| e.to_string())?;
        }
        if let Some(entry_fee) = event.entry_fee {
            worksheet.write_number(1, 5, entry_fee).map_err(|e| e.to_string())?;
        }
        if let Some(prize_pool) = event.prize_pool {
            worksheet.write_number(1, 6, prize_pool).map_err(|e| e.to_string())?;
        }
        if let Some(max_team_rating) = event.max_team_rating {
            worksheet
                .write_number(1, 7, max_team_rating)
                .map_err(|e| e.to_string())?;
        }
        if let Some(payoff_allocation) = event.payoff_allocation.as_ref() {
            worksheet
                .write_string(1, 8, payoff_allocation)
                .map_err(|e| e.to_string())?;
        }
        if let Some(admin_pin) = event.admin_pin.as_ref() {
            worksheet.write_string(1, 9, admin_pin).map_err(|e| e.to_string())?;
        }
        worksheet
            .write_boolean(1, 10, event.status.as_deref() == Some("locked"))
            .map_err(|e| e.to_string())?;
    }

    {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("ropers").map_err(|e| e.to_string())?;
        write_headers(
            worksheet,
            &[
                "backup_roper_key",
                "first_name",
                "last_name",
                "specialty",
                "rating",
                "phone",
                "email",
                "level",
                "is_active",
            ],
        )?;
        for (index, roper) in ropers.iter().enumerate() {
            let row = (index + 1) as u32;
            worksheet
                .write_string(row, 0, roper_key_by_id.get(&roper.id).unwrap())
                .map_err(|e| e.to_string())?;
            worksheet.write_string(row, 1, &roper.first_name).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 2, &roper.last_name).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 3, &roper.specialty).map_err(|e| e.to_string())?;
            worksheet.write_number(row, 4, roper.rating).map_err(|e| e.to_string())?;
            if let Some(phone) = roper.phone.as_ref() {
                worksheet.write_string(row, 5, phone).map_err(|e| e.to_string())?;
            }
            if let Some(email) = roper.email.as_ref() {
                worksheet.write_string(row, 6, email).map_err(|e| e.to_string())?;
            }
            worksheet.write_string(row, 7, &roper.level).map_err(|e| e.to_string())?;
            worksheet
                .write_boolean(row, 8, roper.is_active == 1)
                .map_err(|e| e.to_string())?;
        }
    }

    {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("teams").map_err(|e| e.to_string())?;
        write_headers(
            worksheet,
            &[
                "backup_team_key",
                "header_roper_key",
                "heeler_roper_key",
                "team_rating",
                "status",
            ],
        )?;
        for (index, team) in teams.iter().enumerate() {
            let row = (index + 1) as u32;
            worksheet
                .write_string(row, 0, team_key_by_id.get(&team.id).unwrap())
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 1, roper_key_by_id.get(&team.header_id).unwrap())
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 2, roper_key_by_id.get(&team.heeler_id).unwrap())
                .map_err(|e| e.to_string())?;
            worksheet.write_number(row, 3, team.rating).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 4, &team.status).map_err(|e| e.to_string())?;
        }
    }

    {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("runs").map_err(|e| e.to_string())?;
        write_headers(
            worksheet,
            &[
                "round",
                "position",
                "backup_team_key",
                "time_sec",
                "penalty",
                "total_sec",
                "no_time",
                "dq",
                "status",
                "captured_at",
                "captured_by",
            ],
        )?;
        for (index, run) in runs.iter().enumerate() {
            let row = (index + 1) as u32;
            worksheet
                .write_number(row, 0, run.round as f64)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_number(row, 1, run.position as f64)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 2, team_key_by_id.get(&run.team_id).unwrap())
                .map_err(|e| e.to_string())?;
            if let Some(time_sec) = run.time_sec {
                worksheet.write_number(row, 3, time_sec).map_err(|e| e.to_string())?;
            }
            worksheet.write_number(row, 4, run.penalty).map_err(|e| e.to_string())?;
            if let Some(total_sec) = run.total_sec {
                worksheet.write_number(row, 5, total_sec).map_err(|e| e.to_string())?;
            }
            worksheet
                .write_boolean(row, 6, run.no_time == 1)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_boolean(row, 7, run.dq == 1)
                .map_err(|e| e.to_string())?;
            worksheet.write_string(row, 8, &run.status).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 9, &run.updated_at).map_err(|e| e.to_string())?;
            if let Some(captured_by) = run.captured_by {
                worksheet
                    .write_string(row, 10, &captured_by.to_string())
                    .map_err(|e| e.to_string())?;
            }
        }
    }

    if !roster_rows.is_empty() {
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name("event_roster")
            .map_err(|e| e.to_string())?;
        write_headers(
            worksheet,
            &[
                "backup_roper_key",
                "status",
                "rating_override",
                "notes",
                "external_id",
                "source_hash",
            ],
        )?;
        for (index, (roper_id, status, rating_override, notes, source_hash, external_id)) in
            roster_rows.iter().enumerate()
        {
            let row = (index + 1) as u32;
            worksheet
                .write_string(row, 0, roper_key_by_id.get(roper_id).unwrap())
                .map_err(|e| e.to_string())?;
            worksheet.write_string(row, 1, status).map_err(|e| e.to_string())?;
            if let Some(value) = rating_override {
                worksheet.write_number(row, 2, *value).map_err(|e| e.to_string())?;
            }
            if let Some(value) = notes.as_ref() {
                worksheet.write_string(row, 3, value).map_err(|e| e.to_string())?;
            }
            if let Some(value) = external_id.as_ref() {
                worksheet.write_string(row, 4, value).map_err(|e| e.to_string())?;
            }
            if let Some(value) = source_hash.as_ref() {
                worksheet.write_string(row, 5, value).map_err(|e| e.to_string())?;
            }
        }
    }

    if !draw_rows.is_empty() {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("draw").map_err(|e| e.to_string())?;
        write_headers(worksheet, &["round", "position", "backup_team_key"])?;
        for (index, draw) in draw_rows.iter().enumerate() {
            let row = (index + 1) as u32;
            worksheet
                .write_number(row, 0, draw.round as f64)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_number(row, 1, draw.position as f64)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 2, team_key_by_id.get(&draw.team_id).unwrap())
                .map_err(|e| e.to_string())?;
        }
    }

    if !payoff_rules.is_empty() {
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name("payoff_rules")
            .map_err(|e| e.to_string())?;
        write_headers(worksheet, &["place", "percentage"])?;
        for (index, rule) in payoff_rules.iter().enumerate() {
            let row = (index + 1) as u32;
            worksheet
                .write_number(row, 0, rule.position as f64)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_number(row, 1, rule.percentage)
                .map_err(|e| e.to_string())?;
        }
    }

    if !payoff_snapshot.is_empty() {
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name("payoffs_snapshot")
            .map_err(|e| e.to_string())?;
        write_headers(worksheet, &["place", "backup_team_key", "amount", "per_person"])?;
        for (index, row_data) in payoff_snapshot.iter().enumerate() {
            let row = (index + 1) as u32;
            worksheet
                .write_number(row, 0, row_data.position as f64)
                .map_err(|e| e.to_string())?;
            if let Some(team_key) = team_key_by_id.get(&row_data.team_id) {
                worksheet.write_string(row, 1, team_key).map_err(|e| e.to_string())?;
            }
            worksheet
                .write_number(row, 2, row_data.amount)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_number(row, 3, row_data.amount / 2.0)
                .map_err(|e| e.to_string())?;
        }
    }

    workbook.save(file_path).map_err(|e| e.to_string())?;
    log_audit(
        &db.0,
        "export_event_backup",
        "event",
        Some(event_id),
        Some(format!("Backup XLSX v{} exportado", BACKUP_VERSION)),
    )
    .await?;
    Ok(())
}

async fn inspect_event_backup_impl(_db: &Db, file_path: &str) -> Result<BackupInspection, String> {
    let parsed = parse_event_backup(file_path)?;
    Ok(BackupInspection {
        format: parsed.manifest.format,
        version: parsed.manifest.version,
        event_name: parsed.event_meta.name,
        event_date: parsed.event_meta.date,
        rounds: parsed.event_meta.rounds,
        ropers_count: parsed.ropers.len(),
        teams_count: parsed.teams.len(),
        runs_count: parsed.runs.len(),
        warnings: parsed.warnings,
    })
}

async fn find_reusable_roper_id(
    tx: &mut Transaction<'_, Sqlite>,
    roper: &BackupRoperRow,
) -> Result<Option<i64>, String> {
    let matches: Vec<i64> = sqlx::query_scalar(
        r#"
        SELECT id
        FROM roper
        WHERE LOWER(first_name) = LOWER(?1)
          AND LOWER(last_name) = LOWER(?2)
          AND specialty = ?3
          AND ABS(CAST(rating AS REAL) - ?4) < 0.0001
        LIMIT 2
        "#,
    )
    .bind(&roper.first_name)
    .bind(&roper.last_name)
    .bind(&roper.specialty)
    .bind(roper.rating)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;

    if matches.len() == 1 {
        Ok(matches.into_iter().next())
    } else {
        Ok(None)
    }
}

async fn reconcile_future_runs_tx(tx: &mut Transaction<'_, Sqlite>, event_id: i64) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE run
        SET
          status = 'skipped',
          updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
        WHERE event_id = ?1
          AND status != 'completed'
          AND EXISTS (
            SELECT 1
            FROM run prior
            WHERE prior.event_id = run.event_id
              AND prior.team_id = run.team_id
              AND prior.round < run.round
              AND prior.status = 'completed'
              AND (prior.no_time = 1 OR prior.dq = 1)
          )
        "#,
    )
    .bind(event_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;

    sqlx::query(
        r#"
        UPDATE run
        SET
          status = 'pending',
          updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
        WHERE event_id = ?1
          AND status = 'skipped'
          AND NOT EXISTS (
            SELECT 1
            FROM run prior
            WHERE prior.event_id = run.event_id
              AND prior.team_id = run.team_id
              AND prior.round < run.round
              AND prior.status = 'completed'
              AND (prior.no_time = 1 OR prior.dq = 1)
          )
        "#,
    )
    .bind(event_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;

    Ok(())
}

async fn import_event_backup_impl(
    db: &Db,
    payload: ImportEventBackupPayload,
) -> Result<ImportEventBackupResult, String> {
    let parsed = parse_event_backup(&payload.file_path)?;
    let restore_status_mode = payload
        .restore_status_mode
        .as_deref()
        .unwrap_or("preserve");
    let target_status = resolve_import_status(&parsed.event_meta, restore_status_mode)?;

    let target_series_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM series WHERE id = ?1 AND is_deleted = 0)",
    )
    .bind(payload.target_series_id)
    .fetch_one(&db.0)
    .await
    .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;

    if !target_series_exists {
        return Err(backup_error(
            "BACKUP_INVALID_VALUE",
            format!("La serie destino {} no existe o está eliminada", payload.target_series_id),
        ));
    }

    let mut tx = db
        .0
        .begin()
        .await
        .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;

    let event_insert = sqlx::query(
        r#"
        INSERT INTO event (
            series_id,
            name,
            date,
            status,
            rounds,
            location,
            entry_fee,
            prize_pool,
            max_team_rating,
            payoff_allocation,
            admin_pin
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        "#,
    )
    .bind(payload.target_series_id)
    .bind(&parsed.event_meta.name)
    .bind(&parsed.event_meta.date)
    .bind(&target_status)
    .bind(parsed.event_meta.rounds)
    .bind(&parsed.event_meta.location)
    .bind(parsed.event_meta.entry_fee)
    .bind(parsed.event_meta.prize_pool)
    .bind(parsed.event_meta.max_team_rating)
    .bind(&parsed.event_meta.payoff_allocation)
    .bind(&parsed.event_meta.admin_pin)
    .execute(&mut *tx)
    .await
    .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;
    let new_event_id = event_insert.last_insert_rowid();

    let dedupe_ropers = payload.dedupe_ropers.unwrap_or(false);
    let mut roper_map = HashMap::new();
    let mut ropers_created = 0_i64;
    let mut ropers_reused = 0_i64;

    for roper in &parsed.ropers {
        let roper_id = if dedupe_ropers {
            if let Some(existing_id) = find_reusable_roper_id(&mut tx, roper).await? {
                ropers_reused += 1;
                existing_id
            } else {
                let result = sqlx::query(
                    r#"
                    INSERT INTO roper (
                        first_name,
                        last_name,
                        specialty,
                        rating,
                        phone,
                        email,
                        level,
                        is_active
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                    "#,
                )
                .bind(&roper.first_name)
                .bind(&roper.last_name)
                .bind(&roper.specialty)
                .bind(roper.rating)
                .bind(&roper.phone)
                .bind(&roper.email)
                .bind(&roper.level)
                .bind(if roper.is_active { 1 } else { 0 })
                .execute(&mut *tx)
                .await
                .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;
                ropers_created += 1;
                result.last_insert_rowid()
            }
        } else {
            let result = sqlx::query(
                r#"
                INSERT INTO roper (
                    first_name,
                    last_name,
                    specialty,
                    rating,
                    phone,
                    email,
                    level,
                    is_active
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
            )
            .bind(&roper.first_name)
            .bind(&roper.last_name)
            .bind(&roper.specialty)
            .bind(roper.rating)
            .bind(&roper.phone)
            .bind(&roper.email)
            .bind(&roper.level)
            .bind(if roper.is_active { 1 } else { 0 })
            .execute(&mut *tx)
            .await
            .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;
            ropers_created += 1;
            result.last_insert_rowid()
        };
        roper_map.insert(roper.backup_roper_key.clone(), roper_id);
    }

    if !parsed.event_roster.is_empty() {
        for roster in &parsed.event_roster {
            let roper_id = *roper_map.get(&roster.backup_roper_key).ok_or_else(|| {
                backup_error(
                    "BACKUP_BROKEN_REFERENCE",
                    format!(
                        "No se pudo resolver backup_roper_key '{}' durante import",
                        roster.backup_roper_key
                    ),
                )
            })?;
            sqlx::query(
                r#"
                INSERT INTO event_roster (
                    event_id,
                    roper_id,
                    status,
                    rating_override,
                    source_hash,
                    notes
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
            )
            .bind(new_event_id)
            .bind(roper_id)
            .bind(&roster.status)
            .bind(roster.rating_override)
            .bind(&roster.source_hash)
            .bind(&roster.notes)
            .execute(&mut *tx)
            .await
            .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;
        }
    }

    let mut team_map = HashMap::new();
    let mut teams_created = 0_i64;
    for team in &parsed.teams {
        let header_id = *roper_map.get(&team.header_roper_key).ok_or_else(|| {
            backup_error(
                "BACKUP_BROKEN_REFERENCE",
                format!(
                    "No se pudo resolver header_roper_key '{}' durante import",
                    team.header_roper_key
                ),
            )
        })?;
        let heeler_id = *roper_map.get(&team.heeler_roper_key).ok_or_else(|| {
            backup_error(
                "BACKUP_BROKEN_REFERENCE",
                format!(
                    "No se pudo resolver heeler_roper_key '{}' durante import",
                    team.heeler_roper_key
                ),
            )
        })?;

        let result = sqlx::query(
            r#"
            INSERT INTO team (event_id, header_id, heeler_id, rating, status)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
        )
        .bind(new_event_id)
        .bind(header_id)
        .bind(heeler_id)
        .bind(team.team_rating.unwrap_or(0.0))
        .bind(&team.status)
        .execute(&mut *tx)
        .await
        .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;

        let team_id = result.last_insert_rowid();
        team_map.insert(team.backup_team_key.clone(), team_id);
        teams_created += 1;

        if parsed.event_roster.is_empty() {
            sqlx::query(
                r#"
                INSERT OR IGNORE INTO event_roster (event_id, roper_id, status)
                VALUES (?1, ?2, 'registered'), (?1, ?3, 'registered')
                "#,
            )
            .bind(new_event_id)
            .bind(header_id)
            .bind(heeler_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;
        }
    }

    for draw in &parsed.draw {
        let team_id = *team_map.get(&draw.backup_team_key).ok_or_else(|| {
            backup_error(
                "BACKUP_BROKEN_REFERENCE",
                format!(
                    "No se pudo resolver backup_team_key '{}' para draw",
                    draw.backup_team_key
                ),
            )
        })?;
        sqlx::query(
            r#"
            INSERT INTO draw (event_id, round, position, team_id)
            VALUES (?1, ?2, ?3, ?4)
            "#,
        )
        .bind(new_event_id)
        .bind(draw.round)
        .bind(draw.position)
        .bind(team_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;
    }

    let mut runs_created = 0_i64;
    for run in &parsed.runs {
        let team_id = *team_map.get(&run.backup_team_key).ok_or_else(|| {
            backup_error(
                "BACKUP_BROKEN_REFERENCE",
                format!(
                    "No se pudo resolver backup_team_key '{}' para runs",
                    run.backup_team_key
                ),
            )
        })?;
        let total_sec = run.total_sec.or_else(|| compute_total_from_backup_run(run));
        sqlx::query(
            r#"
            INSERT INTO run (
                event_id,
                team_id,
                round,
                position,
                time_sec,
                penalty,
                total_sec,
                no_time,
                dq,
                status,
                captured_by
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)
            "#,
        )
        .bind(new_event_id)
        .bind(team_id)
        .bind(run.round)
        .bind(run.position)
        .bind(run.time_sec)
        .bind(run.penalty)
        .bind(total_sec)
        .bind(if run.no_time { 1 } else { 0 })
        .bind(if run.dq { 1 } else { 0 })
        .bind(&run.status)
        .execute(&mut *tx)
        .await
        .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;
        runs_created += 1;
    }

    for rule in &parsed.payoff_rules {
        sqlx::query(
            r#"
            INSERT INTO payoff_rule (event_id, position, percentage, is_active)
            VALUES (?1, ?2, ?3, 1)
            "#,
        )
        .bind(new_event_id)
        .bind(rule.place)
        .bind(rule.percentage)
        .execute(&mut *tx)
        .await
        .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;
    }

    reconcile_future_runs_tx(&mut tx, new_event_id).await?;
    tx.commit()
        .await
        .map_err(|e| backup_error("BACKUP_IMPORT_FAILED", e.to_string()))?;

    log_audit(
        &db.0,
        "import_event_backup",
        "event",
        Some(new_event_id),
        Some(format!(
            "Backup '{}' importado (v{}, source_event_id={}, source_series_id={}, exported_at={}, app_version={}, checksum_mode={})",
            parsed.manifest.event_name_original,
            parsed.manifest.version,
            parsed.manifest.event_id_original,
            parsed
                .manifest
                .series_id_original
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
            parsed.manifest.exported_at,
            parsed.manifest.app_version,
            parsed
                .manifest
                .checksum_mode
                .clone()
                .unwrap_or_else(|| "none".to_string())
        )),
    )
    .await?;

    Ok(ImportEventBackupResult {
        event_id: new_event_id,
        event_name: parsed.event_meta.name,
        ropers_created,
        ropers_reused,
        teams_created,
        runs_created,
        warnings: parsed.warnings,
    })
}

#[tauri::command]
async fn export_event_backup(
    db: State<'_, Db>,
    event_id: i64,
    file_path: String,
) -> Result<(), String> {
    db.require_license()?;
    export_event_backup_impl(&db, event_id, &file_path).await
}

#[tauri::command]
async fn inspect_event_backup(
    db: State<'_, Db>,
    file_path: String,
) -> Result<BackupInspection, String> {
    db.require_license()?;
    inspect_event_backup_impl(&db, &file_path).await
}

#[tauri::command]
async fn import_event_backup(
    db: State<'_, Db>,
    payload: ImportEventBackupPayload,
) -> Result<ImportEventBackupResult, String> {
    db.require_license()?;
    import_event_backup_impl(&db, payload).await
}

/* ------------------- EXPORT ------------------- */
#[derive(serde::Deserialize)]
struct ExportOptions {
    overview: bool,
    teams: bool,
    run_order: bool,
    standings: bool,
    payoffs: bool,
    event_logs: bool,
    file_path: String,
}

#[tauri::command]
async fn export_event_to_excel(
    db: State<'_, Db>,
    event_id: i64,
    options: ExportOptions,
) -> Result<(), String> {
    db.require_license()?;
    let mut workbook = Workbook::new();

    // 1. Overview
    if options.overview {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Overview").map_err(|e| e.to_string())?;
        // Fetch event info
        let event: EventRow = sqlx::query_as(
            r#"
            SELECT 
                id, series_id, name, date, status, rounds, location, 
                entry_fee, prize_pool, max_team_rating, created_at, updated_at,
                payoff_allocation, admin_pin,
                0 as teams_count,
                0.0 as pot
            FROM event 
            WHERE id = ?1
            "#,
        )
        .bind(event_id)
        .fetch_one(&db.0)
        .await
        .map_err(|e| e.to_string())?;

        worksheet
            .write_string(0, 0, "Event Name")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 1, &event.name)
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(1, 0, "Date")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(1, 1, &event.date)
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(2, 0, "Status")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(2, 1, event.status.as_deref().unwrap_or(""))
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(3, 0, "Location")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(3, 1, event.location.as_deref().unwrap_or(""))
            .map_err(|e| e.to_string())?;
    }

    // 2. Teams
    if options.teams {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Teams").map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 0, "ID")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 1, "Header")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 2, "Heeler")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 3, "Rating")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 4, "Status")
            .map_err(|e| e.to_string())?;

        let teams_expanded: Vec<(i64, String, String, f64, String)> = sqlx::query_as(
            r#"
            SELECT t.id, 
                   (rh.first_name || ' ' || rh.last_name),
                   (rhe.first_name || ' ' || rhe.last_name),
                   t.rating, t.status
            FROM team t
            JOIN roper rh ON t.header_id = rh.id
            JOIN roper rhe ON t.heeler_id = rhe.id
            WHERE t.event_id = ?1
            ORDER BY t.id
            "#,
        )
        .bind(event_id)
        .fetch_all(&db.0)
        .await
        .map_err(|e| e.to_string())?;

        for (i, (id, header, heeler, rating, status)) in teams_expanded.iter().enumerate() {
            let row = (i + 1) as u32;
            worksheet
                .write_number(row, 0, *id as f64)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 1, header)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 2, heeler)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_number(row, 3, *rating)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 4, status)
                .map_err(|e| e.to_string())?;
        }
    }

    // 3. Run Order
    if options.run_order {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Run Order").map_err(|e| e.to_string())?;
        let runs = get_runs_expanded(db.clone(), event_id, None).await?;
        worksheet
            .write_string(0, 0, "Round")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 1, "Position")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 2, "Header")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 3, "Heeler")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 4, "Time")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 5, "Penalty")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 6, "Total")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 7, "Status")
            .map_err(|e| e.to_string())?;

        for (i, run) in runs.iter().enumerate() {
            let row = (i + 1) as u32;
            worksheet
                .write_number(row, 0, run.round as f64)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_number(row, 1, run.position as f64)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 2, &run.header_name)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 3, &run.heeler_name)
                .map_err(|e| e.to_string())?;
            if let Some(t) = run.time_sec {
                worksheet
                    .write_number(row, 4, t)
                    .map_err(|e| e.to_string())?;
            }
            worksheet
                .write_number(row, 5, run.penalty)
                .map_err(|e| e.to_string())?;
            if let Some(t) = run.total_sec {
                worksheet
                    .write_number(row, 6, t)
                    .map_err(|e| e.to_string())?;
            }
            worksheet
                .write_string(row, 7, &run.status)
                .map_err(|e| e.to_string())?;
        }
    }

    // 4. Standings
    if options.standings {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Standings").map_err(|e| e.to_string())?;
        let standings = get_standings(db.clone(), event_id).await?;
        worksheet
            .write_string(0, 0, "Rank")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 1, "Header")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 2, "Heeler")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 3, "Total Time")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 4, "Caught")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 5, "Avg Time")
            .map_err(|e| e.to_string())?;

        for (i, s) in standings.iter().enumerate() {
            let row = (i + 1) as u32;
            worksheet
                .write_number(row, 0, s.rank as f64)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 1, &s.header_name)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 2, &s.heeler_name)
                .map_err(|e| e.to_string())?;
            if let Some(t) = s.total_time {
                worksheet
                    .write_number(row, 3, t)
                    .map_err(|e| e.to_string())?;
            }
            worksheet
                .write_number(row, 4, s.completed_runs as f64)
                .map_err(|e| e.to_string())?;
            if let Some(t) = s.avg_time {
                worksheet
                    .write_number(row, 5, t)
                    .map_err(|e| e.to_string())?;
            }
        }
    }

    // 5. Payoffs
    if options.payoffs {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Payoffs").map_err(|e| e.to_string())?;
        let breakdown = get_payout_breakdown(db.clone(), event_id).await?;

        worksheet
            .write_string(0, 0, "Total Pot")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_number(0, 1, breakdown.total_pot)
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(1, 0, "Deductions")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_number(1, 1, breakdown.deductions)
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(2, 0, "Net Pot")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_number(2, 1, breakdown.net_pot)
            .map_err(|e| e.to_string())?;

        worksheet
            .write_string(4, 0, "Place")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(4, 1, "Percentage")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(4, 2, "Amount")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(4, 3, "Per Person")
            .map_err(|e| e.to_string())?;

        for (i, p) in breakdown.payouts.iter().enumerate() {
            let row = (i + 5) as u32;
            worksheet
                .write_number(row, 0, p.place as f64)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_number(row, 1, p.percentage)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_number(row, 2, p.amount)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_number(row, 3, p.amount / 2.0)
                .map_err(|e| e.to_string())?;
        }
    }

    // 6. Event Logs
    if options.event_logs {
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name("Event Logs")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 0, "Date")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 1, "Action")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 2, "User")
            .map_err(|e| e.to_string())?;
        worksheet
            .write_string(0, 3, "Details")
            .map_err(|e| e.to_string())?;

        let logs: Vec<(String, String, Option<i64>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT created_at, action, user_id, metadata
            FROM audit_log
            WHERE entity_type = 'event' AND entity_id = ?1
            ORDER BY created_at DESC
            "#,
        )
        .bind(event_id)
        .fetch_all(&db.0)
        .await
        .map_err(|e| e.to_string())?;

        for (i, (date, action, user_id, metadata)) in logs.iter().enumerate() {
            let row = (i + 1) as u32;
            worksheet
                .write_string(row, 0, date)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 1, action)
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 2, user_id.map(|u| u.to_string()).unwrap_or_default())
                .map_err(|e| e.to_string())?;
            worksheet
                .write_string(row, 3, metadata.as_deref().unwrap_or(""))
                .map_err(|e| e.to_string())?;
        }
    }

    workbook
        .save(&options.file_path)
        .map_err(|e| e.to_string())?;
    log_audit(
        &db.0,
        "export_event",
        "event",
        Some(event_id),
        Some("Exported to Excel".into()),
    )
    .await?;
    Ok(())
}

/* ------------------- TIMER CAPTURE COMMANDS ------------------- */

#[derive(serde::Serialize)]
struct SerialPortInfo {
    port_name: String,
    port_type: String,
}

#[tauri::command]
async fn list_serial_ports(db: State<'_, Db>) -> Result<Vec<SerialPortInfo>, String> {
    db.require_license()?;
    let ports = PolarisTimerCapture::list_ports().map_err(|e| e.to_string())?;

    Ok(ports
        .into_iter()
        .map(|p| SerialPortInfo {
            port_name: p.port_name.clone(),
            port_type: match p.port_type {
                serialport::SerialPortType::UsbPort(_) => "USB".to_string(),
                serialport::SerialPortType::BluetoothPort => "Bluetooth".to_string(),
                serialport::SerialPortType::PciPort => "PCI".to_string(),
                serialport::SerialPortType::Unknown => "Unknown".to_string(),
            },
        })
        .collect())
}

#[tauri::command]
async fn connect_timer(db: State<'_, Db>, port_name: String) -> Result<(), String> {
    db.require_license()?;
    let timer = TIMER_CAPTURE.lock().unwrap();
    timer.connect(&port_name).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn disconnect_timer(db: State<'_, Db>) -> Result<(), String> {
    db.require_license()?;
    let timer = TIMER_CAPTURE.lock().unwrap();
    timer.disconnect();
    Ok(())
}

#[tauri::command]
async fn is_timer_connected(db: State<'_, Db>) -> Result<bool, String> {
    db.require_license()?;
    let timer = TIMER_CAPTURE.lock().unwrap();
    Ok(timer.is_connected())
}

#[tauri::command]
async fn start_timer_capture(
    db: State<'_, Db>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    db.require_license()?;
    let timer = TIMER_CAPTURE.lock().unwrap();

    // Start capture (gets receiver)
    let mut rx = timer.start_capture().map_err(|e| e.to_string())?;
    drop(timer); // Release lock

    // Spawn task to forward events to frontend
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            // Emit event to frontend
            let _ = app_handle.emit("timer-event", &event);
            tracing::info!(
                "Timer event: {} sec ({})",
                event.time_seconds,
                event.raw_text.trim()
            );
        }
        tracing::info!("Timer capture ended");
    });

    Ok(())
}

/* ------------------- BOOTSTRAP ------------------- */
fn resolve_db_path(app: &tauri::AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("roping_manager.db"))
}

pub fn run() {
    // Initialize tracing subscriber so tracing::info/error logs are visible
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle();
            let db_path = resolve_db_path(handle)?;
            // Asegura el directorio padre por si acaso (aunque resolve_db_path crea la carpeta)
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // DEBUG: muestra la ruta real que usaremos
            eprintln!("DB path -> {}", db_path.display());

            tauri::async_runtime::block_on(async {
                // Evita pasar la ruta como URL; usa SqliteConnectOptions::filename para
                // evitar problemas con espacios en rutas (p.ej. "Application Support").
                let options = SqliteConnectOptions::new()
                    .filename(&db_path)
                    .create_if_missing(true)
                    .journal_mode(SqliteJournalMode::Wal)
                    .foreign_keys(true);

                let pool = SqlitePoolOptions::new()
                    .max_connections(5)
                    .connect_with(options)
                    .await?;

                sqlx::migrate!("./migrations").run(&pool).await?;
                let runtime = license::bootstrap(handle, &pool).await?;
                let license_state = runtime.license_state();
                app.manage(Db(pool.clone(), runtime.clone()));
                app.manage(runtime);
                app.manage(license_state);
                Ok::<(), anyhow::Error>(())
            })?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health_check,
            list_series,
            create_series,
            update_series,
            delete_series,
            list_events,
            list_all_events_raw,
            create_event,
            lock_event,
            update_event,
            delete_event,
            duplicate_event,
            save_run,
            // teams
            list_teams,
            create_team,
            update_team,
            delete_team,
            hard_delete_teams_for_event,
            // ropers
            list_ropers,
            create_roper,
            update_roper,
            delete_roper,
            delete_all_ropers,
            list_event_roster,
            update_event_roster_entry,
            sync_event_roster,
            // payoff rules
            list_payoff_rules,
            delete_payoff_rule,
            create_payoff_rule,
            get_payout_breakdown,
            // runs/draw
            get_runs,
            get_runs_expanded,
            generate_draw,
            generate_draw_batch,
            // standings
            get_standings,
            get_series_results_summary,
            get_series_roper_rankings,
            get_series_roper_profile,
            // draw
            get_draw,
            update_event_status,
            export_event_to_excel,
            export_event_backup,
            inspect_event_backup,
            import_event_backup,
            // dashboard
            get_recent_activity,
            get_series_logs,
            get_dashboard_stats,
            // timer capture
            list_serial_ports,
            connect_timer,
            disconnect_timer,
            is_timer_connected,
            start_timer_capture,
            // licensing
            license::commands::get_device_hash,
            license::commands::generate_license_request,
            license::commands::install_license,
            license::commands::license_status,
            license::commands::remove_license
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::license::modern;
    use crate::license::validator::DEFAULT_APP_ID;
    use ed25519_dalek::{Keypair, PublicKey, SecretKey};
    use license::runtime::{
        device_binding::DeviceBindingStore, keyring::LicenseKeyring, LicenseRuntime,
        LicenseSummaryStatus,
    };
    use license::storage::StoredLicenseBlob;
    use license::{BindingMatch, LicenseCache, LicenseFormatKind, LicenseState, NormalizedLicense};
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use std::sync::Arc;
    use time::OffsetDateTime;
    use uuid::Uuid;

    async fn setup_test_db() -> Db {
        let db_path = std::env::temp_dir().join(format!("roping-tests-{}.sqlite", Uuid::new_v4()));
        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("failed to open sqlite database");
        let _ = sqlx::query("DROP TABLE IF EXISTS _sqlx_migrations")
            .execute(&pool)
            .await;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations failed");

        let runtime = runtime_for_tests();
        let now = OffsetDateTime::now_utc().unix_timestamp();
        runtime.update_cache(LicenseCache {
            license: mock_normalized_license(now, runtime.device_hash()),
            installed_at: now,
            last_verified_at: now,
            raw_bytes: Vec::new(),
        });

        Db(pool, runtime)
    }

    fn runtime_for_tests() -> LicenseRuntime {
        let dir =
            std::env::temp_dir().join(format!("license-runtime-lib-tests-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp binding dir");
        let binding = DeviceBindingStore::load_or_init_from_dir(&dir).expect("init device binding");
        LicenseRuntime::new(
            binding,
            license::runtime::default_keyring(),
            LicenseState::default(),
            dir.clone(),
            license::runtime::service::verification_environment_for_keyring_env(
                license::runtime::keyring::KEYRING_ENV,
            )
            .expect("verification environment"),
        )
    }

    #[derive(Clone)]
    struct FixedKeyring(PublicKey);

    impl LicenseKeyring for FixedKeyring {
        fn active_key(&self) -> PublicKey {
            self.0
        }

        fn resolve_key(&self, key_id: &str) -> Option<PublicKey> {
            (key_id == "primary").then_some(self.0)
        }
    }

    fn runtime_with_test_keypair() -> (LicenseRuntime, Keypair) {
        let dir = std::env::temp_dir().join(format!("license-runtime-fixed-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp binding dir");
        let binding = DeviceBindingStore::load_or_init_from_dir(&dir).expect("init device binding");
        let keypair = fixed_test_keypair();
        let keyring: Arc<dyn LicenseKeyring + Send + Sync> = Arc::new(FixedKeyring(keypair.public));
        let runtime = LicenseRuntime::new(
            binding,
            keyring,
            LicenseState::default(),
            dir,
            licgen_core::verification::VerificationEnvironment::Development,
        );
        (runtime, keypair)
    }

    fn fixed_test_keypair() -> Keypair {
        let secret =
            SecretKey::from_bytes(&[0xAB; 32]).expect("secret key should be 32 bytes exactly");
        let public: PublicKey = (&secret).into();
        Keypair { secret, public }
    }

    fn mock_normalized_license(now: i64, device_hash: [u8; 32]) -> NormalizedLicense {
        NormalizedLicense {
            format: LicenseFormatKind::ModernLicgen,
            format_version: modern::FORMAT_VERSION,
            app_id: DEFAULT_APP_ID.into(),
            signature_valid: true,
            key_id: Some("primary".into()),
            key_version: None,
            license_id: "test-license".into(),
            plan: Some("monthly".into()),
            customer_name: Some("QA".into()),
            issued_at: now,
            not_before: now - 60,
            not_after: now + 60 * 60,
            max_clock_skew: 60,
            max_offline_days: 30,
            lease_required: false,
            revocation_epoch: None,
            allowed_fingerprints_count: 0,
            device_hash_hex: hex::encode(device_hash),
            installation_id: None,
            installation_pubkey: None,
            binding: BindingMatch::Current,
            blob_len: 128,
            blob_sha256: "deadbeef".into(),
            failure_reason: None,
        }
    }

    fn encode_modern_license_bytes(
        keypair: &Keypair,
        _device_hash: &[u8; 32],
        runtime: &LicenseRuntime,
        now: i64,
    ) -> Vec<u8> {
        use base64::Engine as _;
        use chrono::{Duration, TimeZone, Utc};
        use licgen_core::crypto::{Ed25519CryptoProvider, Ed25519Keypair, LicenseCryptoProvider};
        use licgen_core::{
            ComponentSource, DeviceFingerprintV2, FingerprintBindingBundle, FingerprintComponent,
            FingerprintComponentKind, FingerprintObservation, InstallationIdentity,
            LicensePayloadV5, OfflinePolicy, SecurityPolicy,
        };
        use serde_json::json;

        let issued_at = Utc.timestamp_opt(now, 0).single().unwrap();
        let observed = runtime.binding().fingerprint();
        let projected = DeviceFingerprintV2 {
            version: observed.version,
            hardware_hash: observed.hardware_hash.clone(),
            platform: observed.platform.as_str().to_string(),
            components: observed
                .binding
                .stable
                .iter()
                .chain(observed.binding.strict.iter())
                .map(|component| component.kind.as_str().to_string())
                .collect(),
            binding: FingerprintBindingBundle {
                stable: observed
                    .binding
                    .stable
                    .iter()
                    .map(|component| FingerprintComponent {
                        kind: match component.kind {
                            shared_core::ComponentKind::InstallationAnchor => {
                                FingerprintComponentKind::InstallationAnchor
                            }
                            shared_core::ComponentKind::MachineId => {
                                FingerprintComponentKind::MachineId
                            }
                            shared_core::ComponentKind::DiskSerial => {
                                FingerprintComponentKind::DiskSerial
                            }
                            shared_core::ComponentKind::MotherboardUuid => {
                                FingerprintComponentKind::MotherboardUuid
                            }
                            shared_core::ComponentKind::BiosUuid => {
                                FingerprintComponentKind::BiosUuid
                            }
                            shared_core::ComponentKind::CpuModel => {
                                FingerprintComponentKind::CpuModel
                            }
                            shared_core::ComponentKind::MacAddress => {
                                FingerprintComponentKind::MacAddress
                            }
                            shared_core::ComponentKind::Hostname => {
                                FingerprintComponentKind::Hostname
                            }
                            shared_core::ComponentKind::OsInstallId => {
                                FingerprintComponentKind::OsInstallId
                            }
                        },
                        hash: component.hash.clone(),
                        weight: component.weight,
                        source: match component.source {
                            shared_core::ComponentSource::System => ComponentSource::System,
                            shared_core::ComponentSource::Installer => ComponentSource::Installer,
                            shared_core::ComponentSource::Operator => ComponentSource::Operator,
                        },
                    })
                    .collect(),
                strict: observed
                    .binding
                    .strict
                    .iter()
                    .map(|component| FingerprintComponent {
                        kind: match component.kind {
                            shared_core::ComponentKind::InstallationAnchor => {
                                FingerprintComponentKind::InstallationAnchor
                            }
                            shared_core::ComponentKind::MachineId => {
                                FingerprintComponentKind::MachineId
                            }
                            shared_core::ComponentKind::DiskSerial => {
                                FingerprintComponentKind::DiskSerial
                            }
                            shared_core::ComponentKind::MotherboardUuid => {
                                FingerprintComponentKind::MotherboardUuid
                            }
                            shared_core::ComponentKind::BiosUuid => {
                                FingerprintComponentKind::BiosUuid
                            }
                            shared_core::ComponentKind::CpuModel => {
                                FingerprintComponentKind::CpuModel
                            }
                            shared_core::ComponentKind::MacAddress => {
                                FingerprintComponentKind::MacAddress
                            }
                            shared_core::ComponentKind::Hostname => {
                                FingerprintComponentKind::Hostname
                            }
                            shared_core::ComponentKind::OsInstallId => {
                                FingerprintComponentKind::OsInstallId
                            }
                        },
                        hash: component.hash.clone(),
                        weight: component.weight,
                        source: match component.source {
                            shared_core::ComponentSource::System => ComponentSource::System,
                            shared_core::ComponentSource::Installer => ComponentSource::Installer,
                            shared_core::ComponentSource::Operator => ComponentSource::Operator,
                        },
                    })
                    .collect(),
                observations: observed
                    .binding
                    .observations
                    .iter()
                    .map(|observation| FingerprintObservation {
                        kind: FingerprintComponentKind::Custom(observation.kind.as_str().to_string()),
                        value: observation.value.clone(),
                        note: None,
                    })
                    .collect(),
            },
        };
        let installation = InstallationIdentity {
            installation_id: Uuid::parse_str(&runtime.binding().installation_id()).unwrap(),
            installation_pubkey: Some(
                base64::engine::general_purpose::STANDARD
                    .encode(runtime.binding().installation_pubkey()),
            ),
            device_fingerprint: projected.clone(),
            first_seen_at: issued_at,
            last_online_check_at: None,
        };
        let payload = LicensePayloadV5 {
            license_version: modern::LICENSE_VERSION,
            license_id: Uuid::new_v4(),
            installation,
            issued_at,
            expires_at: issued_at + Duration::hours(1),
            offline_policy: OfflinePolicy {
                max_offline_days: 30,
                ..Default::default()
            },
            security_policy: SecurityPolicy {
                key_id: Some("primary".into()),
                key_version: Some("2026.04".into()),
                ..Default::default()
            },
            device_fingerprint_v2: projected,
            metadata: json!({
                "app_id": DEFAULT_APP_ID,
                "plan": "monthly",
                "customer_name_hint": "QA"
            }),
        };
        let provider = Ed25519CryptoProvider::new(
            Ed25519Keypair::from_seed_bytes("primary", &keypair.secret.to_bytes()).unwrap(),
        );
        let signature = provider.sign_license(&payload).unwrap();
        licgen_core::format::encode_signed_license(&payload, &signature).unwrap()
    }

    #[tokio::test]
    async fn bootstrap_valid_license_updates_runtime_and_guard() {
        let (runtime, keypair) = runtime_with_test_keypair();
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let bytes = encode_modern_license_bytes(&keypair, &runtime.device_hash(), &runtime, now);
        runtime
            .evaluate_license_bytes(&bytes, now)
            .expect("bootstrap verification should seed hardened snapshot");
        let blob = StoredLicenseBlob {
            raw_bytes: bytes,
            installed_at: now,
            last_verified_at: now,
        };
        assert!(runtime.apply_stored_license_for_test(&blob, now));
        let summary = runtime.summary();
        assert_eq!(summary.status, LicenseSummaryStatus::Active);
        assert!(runtime.license_state().snapshot().is_some());
        runtime.ensure_active().expect("ensure_active");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(":memory:")
                    .create_if_missing(true)
                    .journal_mode(SqliteJournalMode::Wal)
                    .foreign_keys(true),
            )
            .await
            .expect("build pool");
        let db = Db(pool, runtime.clone());
        assert!(db.require_license().is_ok());
    }

    #[tokio::test]
    async fn bootstrap_invalid_license_blocks_guard() {
        let (runtime, keypair) = runtime_with_test_keypair();
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let mut bytes =
            encode_modern_license_bytes(&keypair, &runtime.device_hash(), &runtime, now);
        // Flip a byte inside the payload segment to break the signature.
        if let Some(byte) = bytes.get_mut(20) {
            *byte ^= 0xFF;
        }
        let blob = StoredLicenseBlob {
            raw_bytes: bytes,
            installed_at: now,
            last_verified_at: now,
        };
        assert!(!runtime.apply_stored_license_for_test(&blob, now));
        let summary = runtime.summary();
        assert_eq!(summary.status, LicenseSummaryStatus::Invalid);
        assert!(runtime.license_state().snapshot().is_none());
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(":memory:")
                    .create_if_missing(true)
                    .journal_mode(SqliteJournalMode::Wal)
                    .foreign_keys(true),
            )
            .await
            .expect("build pool");
        let db = Db(pool, runtime.clone());
        assert!(db.require_license().is_err());
    }

    async fn seed_event(db: &Db) -> i64 {
        let series_id =
            sqlx::query("INSERT INTO series (name, season, status) VALUES (?1, ?2, 'active')")
                .bind("Serie QA")
                .bind("2026")
                .execute(&db.0)
                .await
                .expect("insert series failed")
                .last_insert_rowid();

        sqlx::query(
            "INSERT INTO event (series_id, name, date, status, rounds, location, entry_fee, prize_pool) VALUES (?1, 'Evento QA', '2026-02-21', 'active', 2, 'Arena', 100, 500)",
        )
        .bind(series_id)
        .execute(&db.0)
        .await
        .expect("insert event failed")
        .last_insert_rowid()
    }

    async fn roper_id_by_email(db: &Db, email: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM roper WHERE email = ?1")
            .bind(email)
            .fetch_one(&db.0)
            .await
            .expect("roper not found by email")
    }

    fn roster_entry(
        first: &str,
        last: &str,
        email: &str,
        specialty: &str,
        status: &str,
    ) -> EventRosterSyncEntry {
        EventRosterSyncEntry {
            external_id: Some(format!("{}-{}", first, last)),
            first_name: first.to_string(),
            last_name: last.to_string(),
            specialty: Some(specialty.to_string()),
            rating: Some(4.0),
            phone: None,
            normalized_phone: None,
            email: Some(email.to_string()),
            level: Some("amateur".into()),
            status: Some(status.to_string()),
            rating_override: None,
            notes: None,
            source_hash: None,
        }
    }

    #[tokio::test]
    async fn sync_event_roster_allows_team_creation() {
        let db = setup_test_db().await;
        let event_id = seed_event(&db).await;

        let payload = SyncEventRosterPayload {
            event_id,
            entries: vec![
                roster_entry(
                    "Ana",
                    "Header",
                    "ana.header@example.com",
                    "header",
                    "confirmed",
                ),
                roster_entry(
                    "Ben",
                    "Heeler",
                    "ben.heeler@example.com",
                    "heeler",
                    "confirmed",
                ),
            ],
            withdraw_absent: Some(true),
        };

        let result = sync_event_roster_internal(&db, payload)
            .await
            .expect("sync roster");
        assert!(result.created_ropers >= 2);
        let roster_records: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM event_roster WHERE event_id = ?1 AND status != 'withdrawn'",
        )
        .bind(event_id)
        .fetch_one(&db.0)
        .await
        .expect("count roster rows");
        assert_eq!(roster_records, 2);

        let header_id = roper_id_by_email(&db, "ana.header@example.com").await;
        let heeler_id = roper_id_by_email(&db, "ben.heeler@example.com").await;

        let team_id = create_team_internal(
            &db,
            NewTeam {
                event_id,
                header_id,
                heeler_id,
                rating: 8.0,
            },
        )
        .await
        .expect("team created");
        assert!(team_id > 0);
    }

    #[tokio::test]
    async fn create_team_requires_active_roster_entries() {
        let db = setup_test_db().await;
        let event_id = seed_event(&db).await;

        // Only header is on the roster initially
        sync_event_roster_internal(
            &db,
            SyncEventRosterPayload {
                event_id,
                entries: vec![roster_entry(
                    "Solo",
                    "Header",
                    "solo.header@example.com",
                    "header",
                    "confirmed",
                )],
                withdraw_absent: Some(true),
            },
        )
        .await
        .expect("sync header only");

        let heeler_id = sqlx::query(
            "INSERT INTO roper (first_name, last_name, specialty, rating, email, level) VALUES ('Hank','Heeler','heeler',3,'hank.heeler@example.com','amateur')",
        )
        .execute(&db.0)
        .await
        .expect("insert heeler row")
        .last_insert_rowid();
        let header_id = roper_id_by_email(&db, "solo.header@example.com").await;

        let err = create_team_internal(
            &db,
            NewTeam {
                event_id,
                header_id,
                heeler_id,
                rating: 7.0,
            },
        )
        .await
        .expect_err("should fail because heeler not in roster");
        assert!(err.contains("roster"), "unexpected error message: {}", err);

        // Add heeler but withdrawn
        sync_event_roster_internal(
            &db,
            SyncEventRosterPayload {
                event_id,
                entries: vec![roster_entry(
                    "Hank",
                    "Heeler",
                    "hank.heeler@example.com",
                    "heeler",
                    "withdrawn",
                )],
                withdraw_absent: Some(false),
            },
        )
        .await
        .expect("sync heeler as withdrawn");

        let err = create_team_internal(
            &db,
            NewTeam {
                event_id,
                header_id,
                heeler_id,
                rating: 7.0,
            },
        )
        .await
        .expect_err("should fail because heeler withdrawn");
        assert!(err.contains("roster"));

        // Reactivate heeler and ensure success
        sync_event_roster_internal(
            &db,
            SyncEventRosterPayload {
                event_id,
                entries: vec![roster_entry(
                    "Hank",
                    "Heeler",
                    "hank.heeler@example.com",
                    "heeler",
                    "confirmed",
                )],
                withdraw_absent: Some(false),
            },
        )
        .await
        .expect("reactivate heeler");

        create_team_internal(
            &db,
            NewTeam {
                event_id,
                header_id,
                heeler_id,
                rating: 7.5,
            },
        )
        .await
        .expect("team should succeed once roster is active");
    }

    async fn create_series(db: &Db, name: &str) -> i64 {
        sqlx::query("INSERT INTO series (name, season, status) VALUES (?1, ?2, 'active')")
            .bind(name)
            .bind("2026")
            .execute(&db.0)
            .await
            .expect("insert series failed")
            .last_insert_rowid()
    }

    async fn create_event_record(
        db: &Db,
        series_id: i64,
        name: &str,
        rounds: i64,
        status: &str,
    ) -> i64 {
        sqlx::query(
            "INSERT INTO event (series_id, name, date, status, rounds, location, entry_fee, prize_pool, max_team_rating, payoff_allocation, admin_pin) VALUES (?1, ?2, '2026-03-01', ?3, ?4, 'Arena', 125, 500, 9.5, '{\"deduction_pct\":0.1}', '1234')",
        )
        .bind(series_id)
        .bind(name)
        .bind(status)
        .bind(rounds)
        .execute(&db.0)
        .await
        .expect("insert event failed")
        .last_insert_rowid()
    }

    async fn create_roper_record(
        db: &Db,
        first_name: &str,
        last_name: &str,
        specialty: &str,
        rating: f64,
        email: &str,
    ) -> i64 {
        sqlx::query(
            "INSERT INTO roper (first_name, last_name, specialty, rating, phone, email, level, is_active) VALUES (?1, ?2, ?3, ?4, NULL, ?5, 'amateur', 1)",
        )
        .bind(first_name)
        .bind(last_name)
        .bind(specialty)
        .bind(rating)
        .bind(email)
        .execute(&db.0)
        .await
        .expect("insert roper failed")
        .last_insert_rowid()
    }

    async fn create_team_record(
        db: &Db,
        event_id: i64,
        header_id: i64,
        heeler_id: i64,
        rating: f64,
        status: &str,
    ) -> i64 {
        sqlx::query(
            "INSERT INTO team (event_id, header_id, heeler_id, rating, status) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(event_id)
        .bind(header_id)
        .bind(heeler_id)
        .bind(rating)
        .bind(status)
        .execute(&db.0)
        .await
        .expect("insert team failed")
        .last_insert_rowid()
    }

    async fn create_event_roster_record(db: &Db, event_id: i64, roper_id: i64, status: &str) {
        sqlx::query(
            "INSERT INTO event_roster (event_id, roper_id, status) VALUES (?1, ?2, ?3)",
        )
        .bind(event_id)
        .bind(roper_id)
        .bind(status)
        .execute(&db.0)
        .await
        .expect("insert event_roster failed");
    }

    async fn create_draw_record(db: &Db, event_id: i64, round: i64, position: i64, team_id: i64) {
        sqlx::query("INSERT INTO draw (event_id, round, position, team_id) VALUES (?1, ?2, ?3, ?4)")
            .bind(event_id)
            .bind(round)
            .bind(position)
            .bind(team_id)
            .execute(&db.0)
            .await
            .expect("insert draw failed");
    }

    async fn create_run_record(
        db: &Db,
        event_id: i64,
        team_id: i64,
        round: i64,
        position: i64,
        time_sec: Option<f64>,
        penalty: f64,
        no_time: bool,
        dq: bool,
        status: &str,
    ) {
        let total_sec = if status == "completed" && !no_time && !dq {
            time_sec.map(|value| value + penalty)
        } else {
            None
        };
        sqlx::query(
            "INSERT INTO run (event_id, team_id, round, position, time_sec, penalty, total_sec, no_time, dq, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .bind(event_id)
        .bind(team_id)
        .bind(round)
        .bind(position)
        .bind(time_sec)
        .bind(penalty)
        .bind(total_sec)
        .bind(if no_time { 1 } else { 0 })
        .bind(if dq { 1 } else { 0 })
        .bind(status)
        .execute(&db.0)
        .await
        .expect("insert run failed");
    }

    async fn create_payoff_rule_record(db: &Db, event_id: i64, position: i64, percentage: f64) {
        sqlx::query(
            "INSERT INTO payoff_rule (event_id, position, percentage, is_active) VALUES (?1, ?2, ?3, 1)",
        )
        .bind(event_id)
        .bind(position)
        .bind(percentage)
        .execute(&db.0)
        .await
        .expect("insert payoff rule failed");
    }

    async fn create_payoff_snapshot_record(
        db: &Db,
        event_id: i64,
        team_id: i64,
        position: i64,
        total_time: f64,
        amount: f64,
    ) {
        sqlx::query(
            "INSERT INTO payoff (event_id, team_id, position, total_time, amount) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(event_id)
        .bind(team_id)
        .bind(position)
        .bind(total_time)
        .bind(amount)
        .execute(&db.0)
        .await
        .expect("insert payoff snapshot failed");
    }

    async fn seed_backup_fixture(
        db: &Db,
        event_name: &str,
        rounds: i64,
        include_nt_dq: bool,
        include_draw: bool,
    ) -> (i64, i64) {
        let series_id = create_series(db, &format!("Serie {}", event_name)).await;
        let event_id = create_event_record(db, series_id, event_name, rounds, "active").await;

        let h1 = create_roper_record(db, "Ana", "Header", "header", 4.0, "ana@example.com").await;
        let he1 =
            create_roper_record(db, "Ben", "Heeler", "heeler", 4.5, "ben@example.com").await;
        let h2 =
            create_roper_record(db, "Caro", "Header", "header", 5.0, "caro@example.com").await;
        let he2 = create_roper_record(db, "Dani", "Heeler", "heeler", 5.5, "dani@example.com")
            .await;

        for roper_id in [h1, he1, h2, he2] {
            create_event_roster_record(db, event_id, roper_id, "confirmed").await;
        }

        let team1 = create_team_record(db, event_id, h1, he1, 8.5, "active").await;
        let team2 = create_team_record(db, event_id, h2, he2, 10.5, "active").await;

        if include_draw {
            for round in 1..=rounds {
                create_draw_record(db, event_id, round, 1, team1).await;
                create_draw_record(db, event_id, round, 2, team2).await;
            }
        }

        create_run_record(db, event_id, team1, 1, 1, Some(6.25), 0.0, false, false, "completed")
            .await;
        create_run_record(db, event_id, team2, 1, 2, Some(7.10), 0.0, false, false, "completed")
            .await;

        if rounds > 1 {
            if include_nt_dq {
                create_run_record(db, event_id, team1, 2, 1, None, 0.0, true, false, "completed")
                    .await;
                create_run_record(db, event_id, team2, 2, 2, None, 0.0, false, true, "completed")
                    .await;
            } else {
                create_run_record(
                    db,
                    event_id,
                    team1,
                    2,
                    1,
                    Some(5.95),
                    5.0,
                    false,
                    false,
                    "completed",
                )
                .await;
                create_run_record(
                    db,
                    event_id,
                    team2,
                    2,
                    2,
                    Some(6.80),
                    0.0,
                    false,
                    false,
                    "completed",
                )
                .await;
            }
        }

        if rounds > 2 {
            create_run_record(db, event_id, team1, 3, 1, None, 0.0, false, false, "pending").await;
            create_run_record(db, event_id, team2, 3, 2, None, 0.0, false, false, "pending").await;
        }

        create_payoff_rule_record(db, event_id, 1, 1.0).await;
        create_payoff_snapshot_record(db, event_id, team1, 1, 6.25, 250.0).await;
        (series_id, event_id)
    }

    fn temp_backup_path(label: &str) -> String {
        std::env::temp_dir()
            .join(format!("{}_{}.xlsx", label, Uuid::new_v4()))
            .display()
            .to_string()
    }

    fn write_minimal_backup_workbook(
        file_path: &str,
        version: i64,
        include_runs_sheet: bool,
        break_references: bool,
        duplicate_team_rows: bool,
    ) {
        let mut workbook = Workbook::new();

        {
            let manifest = workbook.add_worksheet();
            manifest.set_name("manifest").unwrap();
            write_headers(
                manifest,
                &[
                    "format",
                    "version",
                    "exported_at",
                    "app_version",
                    "event_id_original",
                    "series_id_original",
                    "event_name_original",
                    "checksum_mode",
                ],
            )
            .unwrap();
            manifest.write_string(1, 0, BACKUP_FORMAT).unwrap();
            manifest.write_number(1, 1, version as f64).unwrap();
            manifest.write_string(1, 2, "2026-03-01T00:00:00Z").unwrap();
            manifest.write_string(1, 3, "test").unwrap();
            manifest.write_number(1, 4, 1.0).unwrap();
            manifest.write_number(1, 5, 1.0).unwrap();
            manifest.write_string(1, 6, "Backup QA").unwrap();
            manifest.write_string(1, 7, "none").unwrap();
        }

        {
            let event_meta = workbook.add_worksheet();
            event_meta.set_name("event_meta").unwrap();
            write_headers(
                event_meta,
                &[
                    "name",
                    "date",
                    "status",
                    "rounds",
                    "location",
                    "entry_fee",
                    "prize_pool",
                    "max_team_rating",
                    "payoff_allocation",
                    "admin_pin",
                    "is_locked",
                ],
            )
            .unwrap();
            event_meta.write_string(1, 0, "Backup QA").unwrap();
            event_meta.write_string(1, 1, "2026-03-01").unwrap();
            event_meta.write_string(1, 2, "active").unwrap();
            event_meta.write_number(1, 3, 2.0).unwrap();
            event_meta.write_boolean(1, 10, false).unwrap();
        }

        {
            let ropers = workbook.add_worksheet();
            ropers.set_name("ropers").unwrap();
            write_headers(
                ropers,
                &[
                    "backup_roper_key",
                    "first_name",
                    "last_name",
                    "specialty",
                    "rating",
                    "phone",
                    "email",
                    "level",
                    "is_active",
                ],
            )
            .unwrap();
            ropers.write_string(1, 0, "backup_roper_0001").unwrap();
            ropers.write_string(1, 1, "Ana").unwrap();
            ropers.write_string(1, 2, "Header").unwrap();
            ropers.write_string(1, 3, "header").unwrap();
            ropers.write_number(1, 4, 4.0).unwrap();
            ropers.write_string(1, 7, "amateur").unwrap();
            ropers.write_boolean(1, 8, true).unwrap();
            ropers.write_string(2, 0, "backup_roper_0002").unwrap();
            ropers.write_string(2, 1, "Ben").unwrap();
            ropers.write_string(2, 2, "Heeler").unwrap();
            ropers.write_string(2, 3, "heeler").unwrap();
            ropers.write_number(2, 4, 4.5).unwrap();
            ropers.write_string(2, 7, "amateur").unwrap();
            ropers.write_boolean(2, 8, true).unwrap();
        }

        {
            let teams = workbook.add_worksheet();
            teams.set_name("teams").unwrap();
            write_headers(
                teams,
                &[
                    "backup_team_key",
                    "header_roper_key",
                    "heeler_roper_key",
                    "team_rating",
                    "status",
                ],
            )
            .unwrap();
            teams.write_string(1, 0, "backup_team_0001").unwrap();
            teams.write_string(1, 1, "backup_roper_0001").unwrap();
            teams
                .write_string(
                    1,
                    2,
                    if break_references {
                        "missing_roper_key"
                    } else {
                        "backup_roper_0002"
                    },
                )
                .unwrap();
            teams.write_number(1, 3, 8.5).unwrap();
            teams.write_string(1, 4, "active").unwrap();
            if duplicate_team_rows {
                teams.write_string(2, 0, "backup_team_0002").unwrap();
                teams.write_string(2, 1, "backup_roper_0001").unwrap();
                teams.write_string(2, 2, "backup_roper_0002").unwrap();
                teams.write_number(2, 3, 8.5).unwrap();
                teams.write_string(2, 4, "active").unwrap();
            }
        }

        if include_runs_sheet {
            let runs = workbook.add_worksheet();
            runs.set_name("runs").unwrap();
            write_headers(
                runs,
                &[
                    "round",
                    "position",
                    "backup_team_key",
                    "time_sec",
                    "penalty",
                    "total_sec",
                    "no_time",
                    "dq",
                    "status",
                    "captured_at",
                    "captured_by",
                ],
            )
            .unwrap();
            runs.write_number(1, 0, 1.0).unwrap();
            runs.write_number(1, 1, 1.0).unwrap();
            runs.write_string(1, 2, "backup_team_0001").unwrap();
            runs.write_number(1, 3, 6.2).unwrap();
            runs.write_number(1, 4, 0.0).unwrap();
            runs.write_number(1, 5, 6.2).unwrap();
            runs.write_boolean(1, 6, false).unwrap();
            runs.write_boolean(1, 7, false).unwrap();
            runs.write_string(1, 8, "completed").unwrap();
        }

        workbook.save(file_path).unwrap();
    }

    #[tokio::test]
    async fn backup_export_import_simple_event() {
        let db = setup_test_db().await;
        let target_series_id = create_series(&db, "Serie Restore").await;
        let (_, event_id) = seed_backup_fixture(&db, "Backup Simple", 1, false, false).await;
        let file_path = temp_backup_path("backup-simple");

        export_event_backup_impl(&db, event_id, &file_path)
            .await
            .expect("export backup");

        let inspection = inspect_event_backup_impl(&db, &file_path)
            .await
            .expect("inspect backup");
        assert_eq!(inspection.format, BACKUP_FORMAT);
        assert_eq!(inspection.version, BACKUP_VERSION);
        assert_eq!(inspection.teams_count, 2);

        let result = import_event_backup_impl(
            &db,
            ImportEventBackupPayload {
                file_path: file_path.clone(),
                target_series_id,
                restore_status_mode: Some("preserve".into()),
                dedupe_ropers: Some(false),
            },
        )
        .await
        .expect("import backup");

        let imported_event: (String, i64) = sqlx::query_as(
            "SELECT name, rounds FROM event WHERE id = ?1",
        )
        .bind(result.event_id)
        .fetch_one(&db.0)
        .await
        .expect("imported event");
        assert_eq!(imported_event.0, "Backup Simple");
        assert_eq!(imported_event.1, 1);
        assert_eq!(result.teams_created, 2);
        assert_eq!(result.runs_created, 2);
    }

    #[tokio::test]
    async fn backup_export_import_multiple_rounds_and_draw() {
        let db = setup_test_db().await;
        let target_series_id = create_series(&db, "Serie Restore Draw").await;
        let (_, event_id) = seed_backup_fixture(&db, "Backup Draw", 3, false, true).await;
        let file_path = temp_backup_path("backup-draw");

        export_event_backup_impl(&db, event_id, &file_path)
            .await
            .expect("export backup");
        let result = import_event_backup_impl(
            &db,
            ImportEventBackupPayload {
                file_path,
                target_series_id,
                restore_status_mode: Some("preserve".into()),
                dedupe_ropers: Some(false),
            },
        )
        .await
        .expect("import backup");

        let draw_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM draw WHERE event_id = ?1")
            .bind(result.event_id)
            .fetch_one(&db.0)
            .await
            .expect("count draw");
        let run_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM run WHERE event_id = ?1")
            .bind(result.event_id)
            .fetch_one(&db.0)
            .await
            .expect("count runs");
        assert_eq!(draw_count, 6);
        assert_eq!(run_count, 6);
    }

    #[tokio::test]
    async fn backup_export_import_preserves_nt_and_dq() {
        let db = setup_test_db().await;
        let target_series_id = create_series(&db, "Serie Restore NTDQ").await;
        let (_, event_id) = seed_backup_fixture(&db, "Backup NTDQ", 2, true, true).await;
        let file_path = temp_backup_path("backup-ntdq");

        export_event_backup_impl(&db, event_id, &file_path)
            .await
            .expect("export backup");
        let result = import_event_backup_impl(
            &db,
            ImportEventBackupPayload {
                file_path,
                target_series_id,
                restore_status_mode: Some("preserve".into()),
                dedupe_ropers: Some(false),
            },
        )
        .await
        .expect("import backup");

        let nt_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM run WHERE event_id = ?1 AND no_time = 1")
                .bind(result.event_id)
                .fetch_one(&db.0)
                .await
                .expect("count nt");
        let dq_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM run WHERE event_id = ?1 AND dq = 1")
                .bind(result.event_id)
                .fetch_one(&db.0)
                .await
                .expect("count dq");
        assert_eq!(nt_count, 1);
        assert_eq!(dq_count, 1);
    }

    #[tokio::test]
    async fn backup_inspect_rejects_corrupt_file() {
        let db = setup_test_db().await;
        let file_path = std::env::temp_dir()
            .join(format!("backup-corrupt-{}.xlsx", Uuid::new_v4()));
        std::fs::write(&file_path, b"not-an-xlsx").expect("write corrupt file");

        let err = inspect_event_backup_impl(&db, &file_path.display().to_string())
            .await
            .expect_err("inspect should fail");
        assert!(err.contains("BACKUP_INVALID_FORMAT"));
    }

    #[tokio::test]
    async fn backup_inspect_rejects_unsupported_version() {
        let db = setup_test_db().await;
        let file_path = temp_backup_path("backup-unsupported-version");
        write_minimal_backup_workbook(&file_path, 2, true, false, false);

        let err = inspect_event_backup_impl(&db, &file_path)
            .await
            .expect_err("inspect should fail");
        assert!(err.contains("BACKUP_UNSUPPORTED_VERSION"));
    }

    #[tokio::test]
    async fn backup_inspect_rejects_missing_columns() {
        let db = setup_test_db().await;
        let file_path = temp_backup_path("backup-missing-columns");
        let mut workbook = Workbook::new();
        {
            let manifest = workbook.add_worksheet();
            manifest.set_name("manifest").unwrap();
            write_headers(
                manifest,
                &[
                    "format",
                    "version",
                    "exported_at",
                    "app_version",
                    "event_id_original",
                    "series_id_original",
                    "event_name_original",
                    "checksum_mode",
                ],
            )
            .unwrap();
            manifest.write_string(1, 0, BACKUP_FORMAT).unwrap();
            manifest.write_number(1, 1, 1.0).unwrap();
            manifest.write_string(1, 2, "2026-03-01T00:00:00Z").unwrap();
            manifest.write_string(1, 3, "test").unwrap();
            manifest.write_number(1, 4, 1.0).unwrap();
            manifest.write_number(1, 5, 1.0).unwrap();
            manifest.write_string(1, 6, "Backup QA").unwrap();
            manifest.write_string(1, 7, "none").unwrap();
        }
        {
            let event_meta = workbook.add_worksheet();
            event_meta.set_name("event_meta").unwrap();
            write_headers(event_meta, &["name", "date", "status", "rounds"]).unwrap();
            event_meta.write_string(1, 0, "Backup QA").unwrap();
            event_meta.write_string(1, 1, "2026-03-01").unwrap();
            event_meta.write_string(1, 2, "active").unwrap();
            event_meta.write_number(1, 3, 2.0).unwrap();
        }
        {
            let ropers = workbook.add_worksheet();
            ropers.set_name("ropers").unwrap();
            write_headers(
                ropers,
                &[
                    "backup_roper_key",
                    "first_name",
                    "last_name",
                    "specialty",
                    "rating",
                    "level",
                    "is_active",
                ],
            )
            .unwrap();
            ropers.write_string(1, 0, "backup_roper_0001").unwrap();
            ropers.write_string(1, 1, "Ana").unwrap();
            ropers.write_string(1, 2, "Header").unwrap();
            ropers.write_string(1, 3, "header").unwrap();
            ropers.write_number(1, 4, 4.0).unwrap();
            ropers.write_string(1, 5, "amateur").unwrap();
            ropers.write_boolean(1, 6, true).unwrap();
            ropers.write_string(2, 0, "backup_roper_0002").unwrap();
            ropers.write_string(2, 1, "Ben").unwrap();
            ropers.write_string(2, 2, "Heeler").unwrap();
            ropers.write_string(2, 3, "heeler").unwrap();
            ropers.write_number(2, 4, 4.5).unwrap();
            ropers.write_string(2, 5, "amateur").unwrap();
            ropers.write_boolean(2, 6, true).unwrap();
        }
        {
            let teams = workbook.add_worksheet();
            teams.set_name("teams").unwrap();
            write_headers(
                teams,
                &[
                    "backup_team_key",
                    "header_roper_key",
                    "heeler_roper_key",
                    "team_rating",
                    "status",
                ],
            )
            .unwrap();
            teams.write_string(1, 0, "backup_team_0001").unwrap();
            teams.write_string(1, 1, "backup_roper_0001").unwrap();
            teams.write_string(1, 2, "backup_roper_0002").unwrap();
            teams.write_number(1, 3, 8.0).unwrap();
            teams.write_string(1, 4, "active").unwrap();
        }
        {
            let runs = workbook.add_worksheet();
            runs.set_name("runs").unwrap();
            write_headers(
                runs,
                &[
                    "round",
                    "position",
                    "backup_team_key",
                    "time_sec",
                    "total_sec",
                    "no_time",
                    "dq",
                    "status",
                ],
            )
            .unwrap();
            runs.write_number(1, 0, 1.0).unwrap();
            runs.write_number(1, 1, 1.0).unwrap();
            runs.write_string(1, 2, "backup_team_0001").unwrap();
            runs.write_number(1, 3, 6.2).unwrap();
            runs.write_number(1, 4, 6.2).unwrap();
            runs.write_boolean(1, 5, false).unwrap();
            runs.write_boolean(1, 6, false).unwrap();
            runs.write_string(1, 7, "completed").unwrap();
        }
        workbook.save(&file_path).unwrap();

        let err = inspect_event_backup_impl(&db, &file_path)
            .await
            .expect_err("inspect should fail");
        assert!(err.contains("BACKUP_MISSING_COLUMN"));
    }

    #[tokio::test]
    async fn backup_inspect_rejects_broken_references() {
        let db = setup_test_db().await;
        let file_path = temp_backup_path("backup-broken-references");
        write_minimal_backup_workbook(&file_path, 1, true, true, false);

        let err = inspect_event_backup_impl(&db, &file_path)
            .await
            .expect_err("inspect should fail");
        assert!(err.contains("BACKUP_BROKEN_REFERENCE"));
    }

    #[tokio::test]
    async fn backup_import_rolls_back_on_mid_transaction_failure() {
        let db = setup_test_db().await;
        let target_series_id = create_series(&db, "Serie Rollback").await;
        let file_path = temp_backup_path("backup-rollback");
        write_minimal_backup_workbook(&file_path, 1, true, false, true);

        let err = import_event_backup_impl(
            &db,
            ImportEventBackupPayload {
                file_path,
                target_series_id,
                restore_status_mode: Some("preserve".into()),
                dedupe_ropers: Some(false),
            },
        )
        .await
        .expect_err("import should fail");
        assert!(err.contains("BACKUP_IMPORT_FAILED"));

        let created_events: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM event WHERE name = 'Backup QA'")
                .fetch_one(&db.0)
                .await
                .expect("count events");
        assert_eq!(created_events, 0);
    }

    #[tokio::test]
    async fn series_results_only_use_closed_events_and_split_team_payouts() {
        let db = setup_test_db().await;
        let series_id = create_series(&db, "Serie Stats").await;

        let closed_event_id = create_event_record(&db, series_id, "Evento Cerrado", 2, "completed").await;
        let active_event_id = create_event_record(&db, series_id, "Evento Activo", 1, "active").await;

        let header_closed =
            create_roper_record(&db, "Ana", "Header", "header", 4.0, "stats-ana@example.com").await;
        let heeler_closed =
            create_roper_record(&db, "Ben", "Heeler", "heeler", 4.5, "stats-ben@example.com").await;
        let header_active =
            create_roper_record(&db, "Caro", "Header", "header", 5.0, "stats-caro@example.com").await;
        let heeler_active =
            create_roper_record(&db, "Dani", "Heeler", "heeler", 5.0, "stats-dani@example.com").await;

        let closed_team = create_team_record(
            &db,
            closed_event_id,
            header_closed,
            heeler_closed,
            8.5,
            "active",
        )
        .await;
        let active_team = create_team_record(
            &db,
            active_event_id,
            header_active,
            heeler_active,
            9.0,
            "active",
        )
        .await;

        create_run_record(
            &db,
            closed_event_id,
            closed_team,
            1,
            1,
            Some(6.10),
            0.0,
            false,
            false,
            "completed",
        )
        .await;
        create_run_record(
            &db,
            closed_event_id,
            closed_team,
            2,
            1,
            Some(6.00),
            0.0,
            false,
            false,
            "completed",
        )
        .await;
        create_payoff_rule_record(&db, closed_event_id, 1, 1.0).await;

        create_run_record(
            &db,
            active_event_id,
            active_team,
            1,
            1,
            Some(5.00),
            0.0,
            false,
            false,
            "completed",
        )
        .await;
        create_payoff_rule_record(&db, active_event_id, 1, 1.0).await;

        let summary = get_series_results_summary_internal(&db.0, series_id)
            .await
            .expect("series summary");
        assert_eq!(summary.closed_events, 1);
        assert_eq!(summary.unique_ropers, 2);
        assert_eq!(summary.teams_registered, 1);
        assert_eq!(summary.valid_runs, 2);
        assert!(
            (summary.total_distributed - 675.0).abs() < 0.01,
            "expected only closed-event payout to count"
        );

        let rankings = get_series_roper_rankings_internal(&db.0, series_id)
            .await
            .expect("series rankings");
        assert_eq!(rankings.len(), 2);
        assert!(rankings.iter().all(|row| row.events_played == 1));
        assert!(rankings.iter().all(|row| (row.earnings - 337.5).abs() < 0.01));
    }

    #[tokio::test]
    async fn series_roper_profile_returns_real_history_and_best_partner() {
        let db = setup_test_db().await;
        let series_id = create_series(&db, "Serie Perfil").await;

        let event_one = create_event_record(&db, series_id, "Evento Uno", 1, "completed").await;
        let event_two = create_event_record(&db, series_id, "Evento Dos", 1, "locked").await;

        let anchor_header =
            create_roper_record(&db, "Alex", "Anchor", "header", 4.0, "anchor@example.com").await;
        let partner_one =
            create_roper_record(&db, "Ben", "Partner", "heeler", 4.0, "partner-one@example.com").await;
        let partner_two =
            create_roper_record(&db, "Cody", "Partner", "heeler", 4.0, "partner-two@example.com").await;

        let team_one = create_team_record(&db, event_one, anchor_header, partner_one, 8.0, "active").await;
        let team_two = create_team_record(&db, event_two, anchor_header, partner_two, 8.0, "active").await;

        create_run_record(
            &db,
            event_one,
            team_one,
            1,
            1,
            Some(5.80),
            0.0,
            false,
            false,
            "completed",
        )
        .await;
        create_run_record(
            &db,
            event_two,
            team_two,
            1,
            1,
            Some(6.40),
            0.0,
            false,
            false,
            "completed",
        )
        .await;
        create_payoff_rule_record(&db, event_one, 1, 1.0).await;
        create_payoff_rule_record(&db, event_two, 1, 1.0).await;

        let profile = get_series_roper_profile_internal(&db.0, series_id, anchor_header)
            .await
            .expect("roper profile")
            .expect("profile should exist");
        assert_eq!(profile.history.len(), 2);
        assert!(profile.best_partner_name.is_some());
        assert!(profile.best_event_name.is_some());
        assert_eq!(profile.best_run, Some(5.8));
        assert!(profile.history.iter().any(|entry| entry.partner_name == "Ben Partner"));
        assert!(profile.history.iter().any(|entry| entry.partner_name == "Cody Partner"));
    }
}
