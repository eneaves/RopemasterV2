use std::path::{Path, PathBuf};

use license_core::{
    request::{LicenseRequest, REQUEST_VER},
    LicensePayload, DEFAULT_APP_ID,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use time::{macros::format_description, OffsetDateTime};

use super::{
    device,
    storage::{self},
    validator::{self, LicenseRuntimeStatus},
    write_atomic, CmdResult, CommandError, LicenseCache, LicenseState,
};
use crate::Db;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    Monthly,
    Yearly,
    PerEvent,
}

impl Plan {
    pub fn as_str(self) -> &'static str {
        match self {
            Plan::Monthly => "monthly",
            Plan::Yearly => "yearly",
            Plan::PerEvent => "per_event",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LicenseRequestSummaryDto {
    pub path: String,
    pub archive_path: String,
    pub created_at: i64,
    pub plan: String,
    pub device_hash_hex: String,
    pub nonce_hex: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum LicenseUiState {
    Active,
    Expired,
    NotYetValid,
    InvalidDevice,
}

#[derive(Debug, Serialize, Clone)]
pub struct LicenseStatusDto {
    pub status: LicenseUiState,
    pub plan: Option<String>,
    pub customer_name: Option<String>,
    pub license_id: String,
    pub not_before: i64,
    pub not_after: i64,
    pub max_clock_skew: i64,
    pub device_hash_hex: String,
    pub installed_at: i64,
    pub last_verified_at: i64,
    pub last_checked_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LicenseInputPayload {
    Path { path: String },
    Bytes { bytes: Vec<u8> },
}

#[tauri::command]
pub async fn get_device_hash(app: AppHandle) -> CmdResult<String> {
    device::get_or_init_device_hash_hex(&app)
}

#[tauri::command]
pub async fn generate_license_request(
    app: AppHandle,
    plan: Plan,
    customer_name_hint: Option<String>,
    destination_path: Option<String>,
) -> CmdResult<LicenseRequestSummaryDto> {
    let device_hash = device::get_or_init_device_hash(&app)?;
    let created_at = OffsetDateTime::now_utc();
    let nonce: [u8; 16] = rand::thread_rng().gen();

    let request = LicenseRequest {
        ver: REQUEST_VER,
        app_id: DEFAULT_APP_ID.to_string(),
        plan: plan.as_str().to_string(),
        device_hash,
        created_at: created_at.unix_timestamp() as u64,
        nonce,
        customer_name_hint,
    };

    let bytes = license_core::request::request_to_bytes(&request)
        .map_err(|err| CommandError::parse(err.to_string()))?;

    let request_dir = requests_dir(&app)?;
    let hash_hex = hex::encode(device_hash);
    let hash_prefix = &hash_hex[..12];
    let timestamp_fmt = format_description!("[year][month][day]-[hour][minute][second]");
    let timestamp = created_at
        .format(&timestamp_fmt)
        .map_err(|err| CommandError::io(err.to_string()))?;
    let filename = format!("{timestamp}-{}-{hash_prefix}.req", plan.as_str());
    let archive_path = request_dir.join(&filename);
    write_atomic(&archive_path, &bytes).map_err(|err| CommandError::io(err.to_string()))?;

    let user_path = if let Some(custom) = destination_path {
        let user_path = PathBuf::from(custom);
        write_atomic(&user_path, &bytes).map_err(|err| CommandError::io(err.to_string()))?;
        user_path
    } else {
        archive_path.clone()
    };

    Ok(LicenseRequestSummaryDto {
        path: user_path.to_string_lossy().to_string(),
        archive_path: archive_path.to_string_lossy().to_string(),
        created_at: created_at.unix_timestamp(),
        plan: plan.as_str().to_string(),
        device_hash_hex: hash_hex,
        nonce_hex: hex::encode(nonce),
    })
}

#[tauri::command]
pub async fn install_license(
    app: AppHandle,
    state: State<'_, LicenseState>,
    db: State<'_, Db>,
    input: LicenseInputPayload,
) -> CmdResult<LicenseStatusDto> {
    let bytes = match input {
        LicenseInputPayload::Path { path } => {
            std::fs::read(Path::new(&path)).map_err(|err| CommandError::io(err.to_string()))?
        }
        LicenseInputPayload::Bytes { bytes } => bytes,
    };

    let device_hash = device::get_or_init_device_hash(&app)?;
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let key = super::public_key();
    let evaluation = validator::evaluate_license(&key, &bytes, &device_hash, now)?;
    if let Some(err) = runtime_status_error(evaluation.status) {
        return Err(err);
    }
    let payload = evaluation.payload;

    storage::upsert_blob(&db.0, &bytes, now)
        .await
        .map_err(map_sqlx_error)?;
    persist_license_files(&app, &bytes, &payload)?;

    let cache = LicenseCache {
        payload: payload.clone(),
        installed_at: now,
        last_verified_at: now,
    };
    state.replace(Some(cache.clone()));

    Ok(build_status_dto(&cache, &device_hash, now)?)
}

#[tauri::command]
pub async fn license_status(
    app: AppHandle,
    state: State<'_, LicenseState>,
) -> CmdResult<Option<LicenseStatusDto>> {
    if let Some(cache) = state.snapshot() {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let device_hash = device::get_or_init_device_hash(&app)?;
        Ok(Some(build_status_dto(&cache, &device_hash, now)?))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn remove_license(
    app: AppHandle,
    state: State<'_, LicenseState>,
    db: State<'_, Db>,
) -> CmdResult<()> {
    storage::delete_blob(&db.0).await.map_err(map_sqlx_error)?;
    state.replace(None);

    if let Ok(path) = current_license_path(&app) {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }

    Ok(())
}

fn build_status_dto(
    cache: &LicenseCache,
    device_hash: &[u8; 32],
    now: i64,
) -> CmdResult<LicenseStatusDto> {
    let payload = &cache.payload;
    let runtime = validator::runtime_state(payload, device_hash, now)?;
    let (status, not_before, not_after, skew) = (
        match runtime {
            LicenseRuntimeStatus::Active => LicenseUiState::Active,
            LicenseRuntimeStatus::Expired => LicenseUiState::Expired,
            LicenseRuntimeStatus::NotYetValid => LicenseUiState::NotYetValid,
            LicenseRuntimeStatus::DeviceMismatch => LicenseUiState::InvalidDevice,
        },
        payload.not_before as i64,
        payload.not_after as i64,
        payload.max_clock_skew as i64,
    );

    Ok(LicenseStatusDto {
        status,
        plan: Some(payload.plan.clone()).filter(|s| !s.is_empty()),
        customer_name: payload.customer_name.clone(),
        license_id: payload.license_id.clone(),
        not_before,
        not_after,
        max_clock_skew: skew,
        device_hash_hex: hex::encode(payload.allowed_device_hash),
        installed_at: cache.installed_at,
        last_verified_at: cache.last_verified_at,
        last_checked_at: now,
    })
}

fn persist_license_files(app: &AppHandle, bytes: &[u8], payload: &LicensePayload) -> CmdResult<()> {
    let current_path = current_license_path(app)?;
    write_atomic(&current_path, bytes).map_err(|err| CommandError::io(err.to_string()))?;

    let history_dir = installed_dir(app)?.join("history");
    let history_name = format!(
        "{}-{}.lic",
        payload.issued_at,
        sanitize_filename(&payload.license_id)
    );
    let history_path = history_dir.join(history_name);
    write_atomic(&history_path, bytes).map_err(|err| CommandError::io(err.to_string()))?;
    Ok(())
}

fn license_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|err| CommandError::io(err.to_string()))?
        .join("licenses");
    std::fs::create_dir_all(&dir).map_err(|err| CommandError::io(err.to_string()))?;
    Ok(dir)
}

fn requests_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    let path = license_dir(app)?.join("requests");
    std::fs::create_dir_all(&path).map_err(|err| CommandError::io(err.to_string()))?;
    Ok(path)
}

fn installed_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    let path = license_dir(app)?.join("installed");
    std::fs::create_dir_all(&path).map_err(|err| CommandError::io(err.to_string()))?;
    Ok(path)
}

fn current_license_path(app: &AppHandle) -> CmdResult<PathBuf> {
    Ok(installed_dir(app)?.join("current.lic"))
}

fn sanitize_filename(input: &str) -> String {
    input
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn map_sqlx_error(err: sqlx::Error) -> CommandError {
    CommandError::io(err.to_string())
}

fn runtime_status_error(status: LicenseRuntimeStatus) -> Option<CommandError> {
    match status {
        LicenseRuntimeStatus::Active => None,
        LicenseRuntimeStatus::NotYetValid => Some(CommandError::new(
            "NotYetValid",
            "La licencia aún no es válida para este dispositivo",
        )),
        LicenseRuntimeStatus::Expired => Some(CommandError::new(
            "Expired",
            "La licencia ha expirado; instala una nueva.",
        )),
        LicenseRuntimeStatus::DeviceMismatch => Some(CommandError::new(
            "DeviceMismatch",
            "La licencia pertenece a otro dispositivo.",
        )),
    }
}
