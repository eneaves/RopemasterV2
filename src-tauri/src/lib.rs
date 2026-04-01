use anyhow::Result;
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
use std::path::PathBuf;
use tauri::{Emitter, Manager, State};

mod timer_capture;
use once_cell::sync::Lazy;
use std::sync::{Arc, Mutex};
use timer_capture::PolarisTimerCapture;

mod license;

/* ------------------- STATE ------------------- */
#[derive(Clone)]
struct Db(SqlitePool, license::LicenseState);

impl Db {
    fn require_license(&self) -> Result<(), String> {
        license::ensure_active(&self.1)
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
                        ELSE CAST(SUM(CASE WHEN r.status = 'completed' THEN 1 ELSE 0 END) AS REAL) / COUNT(r.id) * 100.0
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

#[tauri::command]
async fn get_payout_breakdown(db: State<'_, Db>, event_id: i64) -> Result<PayoutBreakdown, String> {
    db.require_license()?;
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
    .fetch_one(&db.0)
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
    .fetch_one(&db.0)
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
    .fetch_all(&db.0)
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

    // Si es NT o DQ, sacar al equipo de las rondas siguientes (status='skipped')
    if payload.no_time || payload.dq {
        sqlx::query(
            "UPDATE run SET status = 'skipped' WHERE event_id = ?1 AND team_id = ?2 AND round > ?3",
        )
        .bind(payload.event_id)
        .bind(payload.team_id)
        .bind(payload.round)
        .execute(&db.0)
        .await
        .map_err(|e| e.to_string())?;
    } else {
        // Si se corrige y es tiempo válido, restaurar rondas futuras a 'pending' si estaban 'skipped'
        sqlx::query(
            "UPDATE run SET status = 'pending' WHERE event_id = ?1 AND team_id = ?2 AND round > ?3 AND status = 'skipped'"
        )
        .bind(payload.event_id)
        .bind(payload.team_id)
        .bind(payload.round)
        .execute(&db.0)
        .await
        .map_err(|e| e.to_string())?;
    }

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
            if opts.reseed.unwrap_or(true) {
                teams.shuffle(&mut thread_rng());
            }
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

#[tauri::command]
async fn get_standings(db: State<'_, Db>, event_id: i64) -> Result<Vec<StandingRow>, String> {
    db.require_license()?;
    // Agregados por equipo para el evento
    let mut rows: Vec<StandingAgg> = sqlx::query_as::<_, StandingAgg>(
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
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    // Si no hay runs, regresamos vacío
    if rows.is_empty() {
        return Ok(vec![]);
    }

    // Ordenar: completed_runs desc (pero 0 al final), luego total_time asc (nulos al final),
    // luego best_time asc (nulos al final), y por último team_id asc.
    rows.sort_by(|a, b| {
        use std::cmp::Ordering;
        // completed desc
        let cr = b.completed_runs.cmp(&a.completed_runs);
        if cr != Ordering::Equal {
            return cr;
        }

        // total_time asc (None al final)
        match (&a.total_time, &b.total_time) {
            (Some(ta), Some(tb)) => {
                let ot = ta.partial_cmp(tb).unwrap_or(Ordering::Equal);
                if ot != Ordering::Equal {
                    return ot;
                }
            }
            (Some(_), None) => return Ordering::Less,
            (None, Some(_)) => return Ordering::Greater,
            (None, None) => {}
        }

        // best_time asc (None al final)
        match (&a.best_time, &b.best_time) {
            (Some(ta), Some(tb)) => {
                let ob = ta.partial_cmp(tb).unwrap_or(Ordering::Equal);
                if ob != Ordering::Equal {
                    return ob;
                }
            }
            (Some(_), None) => return Ordering::Less,
            (None, Some(_)) => return Ordering::Greater,
            (None, None) => {}
        }

        // último desempate: team_id
        a.team_id.cmp(&b.team_id)
    });

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
            ELSE CAST(SUM(CASE WHEN r.status = 'completed' THEN 1 ELSE 0 END) AS REAL) / COUNT(r.id) * 100.0
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
            let license_state = license::LicenseState::default();
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
                license::bootstrap(handle, &pool, &license_state).await?;
                app.manage(Db(pool.clone(), license_state.clone()));
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
            // draw
            get_draw,
            update_event_status,
            export_event_to_excel,
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
    use license::{LicenseCache, LicenseState};
    use license_core::{LicensePayload, DEFAULT_APP_ID, PAYLOAD_VERSION_CURRENT};
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use std::collections::BTreeMap;
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

        let state = LicenseState::default();
        let now = OffsetDateTime::now_utc().unix_timestamp();
        state.replace(Some(LicenseCache {
            payload: mock_license_payload(now),
            installed_at: now,
            last_verified_at: now,
        }));

        Db(pool, state)
    }

    fn mock_license_payload(now: i64) -> LicensePayload {
        LicensePayload {
            ver: PAYLOAD_VERSION_CURRENT,
            key_id: 1,
            serial: 1,
            license_id: "test-license".into(),
            issued_at: now as u64,
            not_before: (now - 60) as u64,
            not_after: (now + 60 * 60) as u64,
            max_clock_skew: 60,
            allowed_device_hash: [0; 32],
            plan: "monthly".into(),
            features: BTreeMap::new(),
            policy: BTreeMap::new(),
            customer_name: Some("QA".into()),
            app_id: DEFAULT_APP_ID.into(),
        }
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
}
