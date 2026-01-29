use anyhow::Result;
use rand::seq::SliceRandom;
use rand::thread_rng;
use sqlx::QueryBuilder;
use sqlx::Row;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    FromRow, Sqlite, SqlitePool, Transaction,
};
use std::path::PathBuf;
use rust_xlsxwriter::*;
use tauri::{Manager, State};

/* ------------------- STATE ------------------- */
#[derive(Clone)]
struct Db(SqlitePool);

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
        "#
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

/* ------------------- HEALTH ------------------- */
#[tauri::command]
async fn health_check(db: State<'_, Db>) -> Result<String, String> {
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
    log_audit(&db.0, "create_series", "series", Some(id), Some(payload.name)).await?;
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
                (COALESCE(e.entry_fee, 0.0) * (SELECT COUNT(*) FROM team t WHERE t.event_id = e.id AND t.status = 'active'))
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
                (COALESCE(e.entry_fee, 0.0) * (SELECT COUNT(*) FROM team t WHERE t.event_id = e.id AND t.status = 'active'))
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
    .bind(&payload.entry_fee)
    .bind(&payload.prize_pool)
    .bind(&payload.max_team_rating)
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
    
    log_audit(&db.0, "update_event_status", "event", Some(id), Some(normalized_status)).await?;
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
        builder.push("payoff_allocation = ").push_bind(pa).push(", ");
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
    let pool = &db.0;
    // Verificar existencia y estado
    let status_opt: Option<String> =
        sqlx::query_scalar("SELECT status FROM event WHERE id = ?1 AND is_deleted = 0")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;

    let Some(_status) = status_opt else {
        return Err("Evento no encontrado.".into());
    };

    // if status == "locked" {
    //     return Err("El evento está bloqueado; no se puede eliminar.".into());
    // }

    // Soft-delete: marcar is_deleted = 1. No cambiamos status a 'archived' porque el CHECK constraint no lo permite.
    let res = sqlx::query("UPDATE event SET is_deleted = 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?1")
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
    log_audit(pool, "duplicate_event", "event", Some(new_id), Some(format!("Copied from {}", id))).await?;
    Ok(new_id)
}

#[tauri::command]
async fn lock_event(db: State<'_, Db>, event_id: i64) -> Result<(), String> {
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
        log_audit(&db.0, "create_payoff_rule", "payoff_rule", Some(new_id), None).await?;
        Ok(new_id)
    }
}

#[derive(serde::Serialize)]
struct PayoutBreakdown {
    total_pot: f64,
    deductions: f64,
    net_pot: f64,
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
        "#
    )
    .bind(event_id)
    .fetch_one(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    // 2. Count Active Teams
    let team_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM team WHERE event_id = ?1 AND status = 'active'")
            .bind(event_id)
            .fetch_one(&db.0)
            .await
            .map_err(|e| e.to_string())?;

    // 3. Calculate Pot
    let entry_fee = event.entry_fee.unwrap_or(0.0);
    let prize_pool = event.prize_pool.unwrap_or(0.0);
    let total_pot = (team_count as f64 * entry_fee) + prize_pool;

    // Deductions (Placeholder: 0% for now, or make it configurable later)
    let deduction_pct = 0.0;
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
            "UPDATE run SET status = 'skipped' WHERE event_id = ?1 AND team_id = ?2 AND round > ?3"
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
    log_audit(&db.0, "save_run", "run", Some(run_id), Some(format!("Event {} Round {}", payload.event_id, payload.round))).await?;
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

#[tauri::command]
async fn create_team(db: State<'_, Db>, t: NewTeam) -> Result<i64, String> {
    // log intent
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

    // Verifica que existan los ropers
    let exist: (i64, i64) = sqlx::query_as(
        r#"
        SELECT 
          (SELECT COUNT(1) FROM roper WHERE id = ?1) AS h,
          (SELECT COUNT(1) FROM roper WHERE id = ?2) AS he
        "#,
    )
    .bind(t.header_id)
    .bind(t.heeler_id)
    .fetch_one(&db.0)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "create_team: error checking ropers");
        e.to_string()
    })?;

    if exist.0 == 0 || exist.1 == 0 {
        tracing::error!(
            header_exists = exist.0,
            heeler_exists = exist.1,
            "create_team failed: missing roper"
        );
        return Err("Header o Heeler no existen en la tabla roper.".into());
    }

    // Inserta respetando UNIQUE(event_id, header_id, heeler_id)
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
    .await;

    match res {
        Ok(r) => {
            let last_id = r.last_insert_rowid();
            tracing::info!(
                event_id = t.event_id,
                header_id = t.header_id,
                heeler_id = t.heeler_id,
                rating = t.rating,
                last_row = last_id,
                "create_team: success"
            );
            log_audit(&db.0, "create_team", "team", Some(last_id), Some(format!("Event {}", t.event_id))).await?;
            Ok(last_id)
        }
        Err(e) => {
            tracing::error!(error = %e, "create_team failed: insert error");
            if e.to_string().contains("UNIQUE") {
                Err("Ya existe un equipo con ese header/heeler en este evento.".into())
            } else {
                Err(e.to_string())
            }
        }
    }
}

#[tauri::command]
async fn hard_delete_teams_for_event(db: State<'_, Db>, event_id: i64) -> Result<(), String> {
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
            log_audit(&db.0, "hard_delete_teams", "team", None, Some(format!("Event {}", event_id))).await?;
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
async fn get_series_logs(db: State<'_, Db>, series_id: i64, limit: i64) -> Result<Vec<AuditLogItem>, String> {
    sqlx::query_as::<_, AuditLogItem>(
        r#"
        SELECT id, action, entity_type, entity_id, user_id, metadata, created_at
        FROM audit_log
        WHERE (entity_type = 'series' AND entity_id = ?1)
           OR (entity_type = 'event' AND entity_id IN (SELECT id FROM event WHERE series_id = ?1))
        ORDER BY created_at DESC
        LIMIT ?2
        "#
    )
    .bind(series_id)
    .bind(limit)
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())
}

/* ------------------- ROPERS ------------------- */

#[tauri::command]
async fn list_ropers(db: State<'_, Db>) -> Result<Vec<RoperRow>, String> {
    // Solo retornamos ropers activos (is_active = 1) como parte de la política de soft-delete.
    sqlx::query_as::<_, RoperRow>(
        r#"
        SELECT id, first_name, last_name, specialty, CAST(rating AS INTEGER) AS rating, phone, email, level, created_at, updated_at
        FROM roper
        WHERE is_active = 1
        ORDER BY last_name, first_name
        "#,
    )
    .fetch_all(&db.0)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_roper(db: State<'_, Db>, r: NewRoper) -> Result<i64, String> {
    // Validar specialty
    if r.specialty != "header" && r.specialty != "heeler" && r.specialty != "both" {
        return Err("Specialty inválida: usa 'header', 'heeler' o 'both'.".into());
    }
    if r.rating < 0 {
        return Err("Rating inválido: debe ser >= 0.".into());
    }

    // validar nivel
    let level = r.level.unwrap_or_else(|| "amateur".to_string());
    let level_l = level.to_lowercase();
    if level_l != "pro" && level_l != "amateur" && level_l != "principiante" {
        return Err("Nivel inválido: use 'pro', 'amateur' o 'principiante'.".into());
    }

    let res = sqlx::query(
        r#"
        INSERT INTO roper (first_name, last_name, specialty, rating, phone, email, level)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
    )
    .bind(&r.first_name)
    .bind(&r.last_name)
    .bind(&r.specialty)
    .bind(r.rating)
    .bind(&r.phone)
    .bind(&r.email)
    .bind(level_l)
    .execute(&db.0)
    .await
    .map_err(|e| e.to_string())?;

    let id = res.last_insert_rowid();
    log_audit(&db.0, "create_roper", "roper", Some(id), Some(format!("{} {}", r.first_name, r.last_name))).await?;
    Ok(id)
}

#[tauri::command]
async fn update_roper(db: State<'_, Db>, r: UpdateRoper) -> Result<(), String> {
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
    if let Some(phone) = r.phone {
        builder.push("phone = ").push_bind(phone).push(", ");
        has_any = true;
    }
    if let Some(email) = r.email {
        builder.push("email = ").push_bind(email).push(", ");
        has_any = true;
    }
    if let Some(level) = r.level {
        let lvl = level.to_lowercase();
        if lvl != "pro" && lvl != "amateur" && lvl != "principiante" {
            return Err("Nivel inválido: use 'pro', 'amateur' o 'principiante'.".into());
        }
        builder.push("level = ").push_bind(lvl).push(", ");
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
    // Política: soft-delete para ropers. Marcamos `is_active = 0`.
    let res = sqlx::query("UPDATE roper SET is_active = 0, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?1")
        .bind(id)
        .execute(&db.0)
        .await
        .map_err(|e| e.to_string())?;

    if res.rows_affected() == 0 {
        return Err("Roper no encontrado.".into());
    }

    log_audit(&db.0, "delete_roper", "roper", Some(id), None).await?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct UpdateTeam {
    id: i64,
    rating: Option<f64>,
    status: Option<String>, // 'active' | 'inactive'
}

#[tauri::command]
async fn update_team(db: State<'_, Db>, t: UpdateTeam) -> Result<(), String> {
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
            return Err("El evento está finalizado o archivado. No se pueden modificar rondas.".into());
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
        return Err(format!("La ronda {} ya ha comenzado (tiene tiempos capturados). No se puede regenerar.", opts.round));
    }

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

    // 3) si reseed o no hay draw previo, barajar
    let reseed = opts.reseed.unwrap_or(true);
    if reseed {
        teams.shuffle(&mut thread_rng());
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

    log_audit(&db.0, "generate_draw", "draw", None, Some(format!("Event {} Round {}", opts.event_id, opts.round))).await?;
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
    ensure_event_unlocked(&db.0, opts.event_id).await?;

    // Get active teams with composition for smart shuffling (filtering eliminated)
    let mut teams: Vec<(i64, i64, i64)> = sqlx::query_as(
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

    let mut tx: Transaction<'_, Sqlite> = db.0.begin().await.map_err(|e| e.to_string())?;

    // For each round
    for r in 1..=opts.rounds {
        // Shuffle if requested (with smart spacing logic)
        if opts.shuffle {
            // 1. Random shuffle first
            teams.shuffle(&mut thread_rng());

            // 2. Smart sort to avoid consecutive ropers
            let mut ordered: Vec<(i64, i64, i64)> = Vec::with_capacity(teams.len());
            let mut pool = teams.clone();
            
            while !pool.is_empty() {
                let mut best_idx = 0;
                let mut best_score = -1;
                
                // Scan candidates
                for (i, candidate) in pool.iter().enumerate() {
                    let mut spacing = 0;
                    let mut conflicts = false;
                    
                    // Check backwards in 'ordered' to find distance to last conflict
                    // We check only last 5 for performance/relevance
                    for prev in ordered.iter().rev().take(10) {
                         spacing += 1;
                         // Check if any roper matches
                         if prev.1 == candidate.1 || prev.1 == candidate.2 || 
                            prev.2 == candidate.1 || prev.2 == candidate.2 {
                             conflicts = true;
                             break;
                         }
                    }
                    
                    // Score: if no conflict in last 10, score is 100 max used.
                    // If conflict at distance 'spacing', score is 'spacing'.
                    // We want to maximize spacing.
                    let score = if conflicts { spacing } else { 100 };
                    
                    if score > best_score {
                        best_score = score;
                        best_idx = i;
                        // Optimization: if we found a candidate with no recent conflict, pick it immediately
                        if score >= 10 { break; } 
                    }
                }
                
                ordered.push(pool.remove(best_idx));
            }
            teams = ordered;
        }

        for (idx, team_tuple) in teams.iter().enumerate() {
            let team_id = team_tuple.0;
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
            .bind(team_id)
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
            .bind(team_id)
            .bind(r)
            .bind(position)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    tx.commit().await.map_err(|e| e.to_string())?;

    log_audit(&db.0, "generate_draw_batch", "draw", None, Some(format!("Event {} Rounds {}", opts.event_id, opts.rounds))).await?;
    Ok(teams.len() as i64 * opts.rounds)
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
async fn get_recent_activity(db: State<'_, Db>, limit: i64, offset: Option<i64>) -> Result<Vec<AuditLogItem>, String> {
    let off = offset.unwrap_or(0);
    sqlx::query_as::<_, AuditLogItem>(
        r#"
        SELECT id, action, entity_type, entity_id, user_id, metadata, created_at
        FROM audit_log
        ORDER BY created_at DESC
        LIMIT ?1 OFFSET ?2
        "#
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
    let pool = &db.0;

    let total_series: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM series WHERE is_deleted = 0")
        .fetch_one(pool).await.map_err(|e| e.to_string())?;
    
    let active_series: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM series WHERE is_deleted = 0 AND status = 'active'")
        .fetch_one(pool).await.map_err(|e| e.to_string())?;

    let total_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event WHERE is_deleted = 0")
        .fetch_one(pool).await.map_err(|e| e.to_string())?;

    let active_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event WHERE is_deleted = 0 AND status = 'active'")
        .fetch_one(pool).await.map_err(|e| e.to_string())?;

    let completed_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event WHERE is_deleted = 0 AND (status = 'completed' OR status = 'locked')")
        .fetch_one(pool).await.map_err(|e| e.to_string())?;

    let upcoming_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event WHERE is_deleted = 0 AND status = 'upcoming'")
        .fetch_one(pool).await.map_err(|e| e.to_string())?;

    let locked_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event WHERE is_deleted = 0 AND status = 'locked'")
        .fetch_one(pool).await.map_err(|e| e.to_string())?;

    let total_teams: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM team WHERE status = 'active'")
        .fetch_one(pool).await.map_err(|e| e.to_string())?;

    // Calculate Total Pot: Sum of (entry_fee * team_count) + prize_pool for all active/completed events
    let pot_opt: Option<f64> = sqlx::query_scalar(
        r#"
        SELECT SUM(
            COALESCE(e.prize_pool, 0) + 
            (COALESCE(e.entry_fee, 0) * (SELECT COUNT(*) FROM team t WHERE t.event_id = e.id AND t.status = 'active'))
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
        "#
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
async fn export_event_to_excel(db: State<'_, Db>, event_id: i64, options: ExportOptions) -> Result<(), String> {
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
            "#
        )
            .bind(event_id)
            .fetch_one(&db.0)
            .await
            .map_err(|e| e.to_string())?;
        
        worksheet.write_string(0, 0, "Event Name").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 1, &event.name).map_err(|e| e.to_string())?;
        worksheet.write_string(1, 0, "Date").map_err(|e| e.to_string())?;
        worksheet.write_string(1, 1, &event.date).map_err(|e| e.to_string())?;
        worksheet.write_string(2, 0, "Status").map_err(|e| e.to_string())?;
        worksheet.write_string(2, 1, event.status.as_deref().unwrap_or("")).map_err(|e| e.to_string())?;
        worksheet.write_string(3, 0, "Location").map_err(|e| e.to_string())?;
        worksheet.write_string(3, 1, event.location.as_deref().unwrap_or("")).map_err(|e| e.to_string())?;
    }

    // 2. Teams
    if options.teams {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Teams").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 0, "ID").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 1, "Header").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 2, "Heeler").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 3, "Rating").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 4, "Status").map_err(|e| e.to_string())?;

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
            "#
        )
        .bind(event_id)
        .fetch_all(&db.0)
        .await
        .map_err(|e| e.to_string())?;

        for (i, (id, header, heeler, rating, status)) in teams_expanded.iter().enumerate() {
            let row = (i + 1) as u32;
            worksheet.write_number(row, 0, *id as f64).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 1, header).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 2, heeler).map_err(|e| e.to_string())?;
            worksheet.write_number(row, 3, *rating).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 4, status).map_err(|e| e.to_string())?;
        }
    }

    // 3. Run Order
    if options.run_order {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Run Order").map_err(|e| e.to_string())?;
        let runs = get_runs_expanded(db.clone(), event_id, None).await?;
        worksheet.write_string(0, 0, "Round").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 1, "Position").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 2, "Header").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 3, "Heeler").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 4, "Time").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 5, "Penalty").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 6, "Total").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 7, "Status").map_err(|e| e.to_string())?;

        for (i, run) in runs.iter().enumerate() {
            let row = (i + 1) as u32;
            worksheet.write_number(row, 0, run.round as f64).map_err(|e| e.to_string())?;
            worksheet.write_number(row, 1, run.position as f64).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 2, &run.header_name).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 3, &run.heeler_name).map_err(|e| e.to_string())?;
            if let Some(t) = run.time_sec { worksheet.write_number(row, 4, t).map_err(|e| e.to_string())?; }
            worksheet.write_number(row, 5, run.penalty).map_err(|e| e.to_string())?;
            if let Some(t) = run.total_sec { worksheet.write_number(row, 6, t).map_err(|e| e.to_string())?; }
            worksheet.write_string(row, 7, &run.status).map_err(|e| e.to_string())?;
        }
    }

    // 4. Standings
    if options.standings {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Standings").map_err(|e| e.to_string())?;
        let standings = get_standings(db.clone(), event_id).await?;
        worksheet.write_string(0, 0, "Rank").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 1, "Header").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 2, "Heeler").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 3, "Total Time").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 4, "Caught").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 5, "Avg Time").map_err(|e| e.to_string())?;

        for (i, s) in standings.iter().enumerate() {
            let row = (i + 1) as u32;
            worksheet.write_number(row, 0, s.rank as f64).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 1, &s.header_name).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 2, &s.heeler_name).map_err(|e| e.to_string())?;
            if let Some(t) = s.total_time { worksheet.write_number(row, 3, t).map_err(|e| e.to_string())?; }
            worksheet.write_number(row, 4, s.completed_runs as f64).map_err(|e| e.to_string())?;
            if let Some(t) = s.avg_time { worksheet.write_number(row, 5, t).map_err(|e| e.to_string())?; }
        }
    }

    // 5. Payoffs
    if options.payoffs {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Payoffs").map_err(|e| e.to_string())?;
        let breakdown = get_payout_breakdown(db.clone(), event_id).await?;
        
        worksheet.write_string(0, 0, "Total Pot").map_err(|e| e.to_string())?;
        worksheet.write_number(0, 1, breakdown.total_pot).map_err(|e| e.to_string())?;
        worksheet.write_string(1, 0, "Deductions").map_err(|e| e.to_string())?;
        worksheet.write_number(1, 1, breakdown.deductions).map_err(|e| e.to_string())?;
        worksheet.write_string(2, 0, "Net Pot").map_err(|e| e.to_string())?;
        worksheet.write_number(2, 1, breakdown.net_pot).map_err(|e| e.to_string())?;

        worksheet.write_string(4, 0, "Place").map_err(|e| e.to_string())?;
        worksheet.write_string(4, 1, "Percentage").map_err(|e| e.to_string())?;
        worksheet.write_string(4, 2, "Amount").map_err(|e| e.to_string())?;
        worksheet.write_string(4, 3, "Per Person").map_err(|e| e.to_string())?;

        for (i, p) in breakdown.payouts.iter().enumerate() {
            let row = (i + 5) as u32;
            worksheet.write_number(row, 0, p.place as f64).map_err(|e| e.to_string())?;
            worksheet.write_number(row, 1, p.percentage).map_err(|e| e.to_string())?;
            worksheet.write_number(row, 2, p.amount).map_err(|e| e.to_string())?;
            worksheet.write_number(row, 3, p.amount / 2.0).map_err(|e| e.to_string())?;
        }
    }

    // 6. Event Logs
    if options.event_logs {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Event Logs").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 0, "Date").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 1, "Action").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 2, "User").map_err(|e| e.to_string())?;
        worksheet.write_string(0, 3, "Details").map_err(|e| e.to_string())?;

        let logs: Vec<(String, String, Option<i64>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT created_at, action, user_id, metadata
            FROM audit_log
            WHERE entity_type = 'event' AND entity_id = ?1
            ORDER BY created_at DESC
            "#
        )
        .bind(event_id)
        .fetch_all(&db.0)
        .await
        .map_err(|e| e.to_string())?;

        for (i, (date, action, user_id, metadata)) in logs.iter().enumerate() {
            let row = (i + 1) as u32;
            worksheet.write_string(row, 0, date).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 1, action).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 2, &user_id.map(|u| u.to_string()).unwrap_or_default()).map_err(|e| e.to_string())?;
            worksheet.write_string(row, 3, metadata.as_deref().unwrap_or("")).map_err(|e| e.to_string())?;
        }
    }

    workbook.save(&options.file_path).map_err(|e| e.to_string())?;
    log_audit(&db.0, "export_event", "event", Some(event_id), Some("Exported to Excel".into())).await?;
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
            let db_path = resolve_db_path(&handle)?;
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
                app.manage(Db(pool));
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
            get_dashboard_stats
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri");
}
