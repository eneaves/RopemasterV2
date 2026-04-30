use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use time::{macros::format_description, OffsetDateTime};

use super::{
    runtime::{LicenseRuntime, LicenseSummaryStatus},
    storage::{self},
    validator::LicenseRuntimeStatus,
    write_atomic, CmdResult, CommandError, LicenseCache, NormalizedLicense,
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
    pub exported_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_path: Option<String>,
    pub archived_internally: bool,
    pub created_at: i64,
    pub plan: String,
    pub device_hash_hex: String,
    pub request_id_hex: String,
    pub installation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce_hex: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum LicenseUiState {
    Active,
    Expired,
    NotYetValid,
    DeviceMismatch,
    Missing,
    Invalid,
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
    pub is_placeholder: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LicenseInputPayload {
    Path { path: String },
    Bytes { bytes: Vec<u8> },
}

#[tauri::command]
pub async fn get_device_hash(runtime: State<'_, LicenseRuntime>) -> CmdResult<String> {
    Ok(runtime.device_hash_hex())
}

#[tauri::command]
pub async fn generate_license_request(
    app: AppHandle,
    runtime: State<'_, LicenseRuntime>,
    plan: Plan,
    customer_name_hint: Option<String>,
    destination_path: Option<String>,
) -> CmdResult<LicenseRequestSummaryDto> {
    let (request, bytes) = runtime
        .generate_request_bytes(plan.as_str(), customer_name_hint)
        .map_err(|err| err)?;
    let created_at = OffsetDateTime::from_unix_timestamp((request.created_at_ms / 1000) as i64)
        .map_err(|err| CommandError::parse(err.to_string()))?;

    let request_dir = storage::requests_dir(&app)?;
    let hash_hex = request.installation.fingerprint.hardware_hash.clone();
    let request_id_hex = request.request_id.replace('-', "");
    let hash_prefix = &hash_hex[..12];
    let timestamp_fmt = format_description!("[year][month][day]-[hour][minute][second]");
    let timestamp = created_at
        .format(&timestamp_fmt)
        .map_err(|err| CommandError::io(err.to_string()))?;
    let filename = format!("{timestamp}-{}-{hash_prefix}.req", plan.as_str());
    let targets = build_request_targets(&request_dir, &filename, destination_path.as_deref());
    write_request_targets(&targets, &bytes)?;
    let exported_path = targets.exported_path.to_string_lossy().to_string();
    let archived_path = targets
        .archived_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());

    Ok(LicenseRequestSummaryDto {
        exported_path,
        archived_path,
        archived_internally: targets.archived_internally,
        created_at: created_at.unix_timestamp(),
        plan: plan.as_str().to_string(),
        device_hash_hex: hash_hex,
        nonce_hex: request.nonce.clone(),
        request_id_hex,
        installation_id: request.installation.installation_id.clone(),
    })
}

#[tauri::command]
pub async fn install_license(
    app: AppHandle,
    runtime: State<'_, LicenseRuntime>,
    db: State<'_, Db>,
    input: LicenseInputPayload,
) -> CmdResult<LicenseStatusDto> {
    runtime.invalidate_observed_binding_cache();
    let bytes = match input {
        LicenseInputPayload::Path { path } => {
            std::fs::read(Path::new(&path)).map_err(|err| CommandError::io(err.to_string()))?
        }
        LicenseInputPayload::Bytes { bytes } => bytes,
    };

    let now = OffsetDateTime::now_utc().unix_timestamp();
    let evaluation = runtime.evaluate_license_bytes(&bytes, now)?;
    ensure_installable(evaluation.status)?;
    let license = evaluation.license;
    let integrity_secret = runtime.binding().key_store().derive_secret(
        storage::LOCAL_LICENSE_INTEGRITY_PURPOSE,
        runtime.binding().installation_id().as_bytes(),
    );

    storage::upsert_blob(&db.0, &bytes, now)
        .await
        .map_err(map_sqlx_error)?;
    storage::persist_license_files(&app, &bytes, &license)?;
    storage::persist_current_license_integrity(&app, &bytes, &integrity_secret)?;

    let cache = LicenseCache {
        license: license.clone(),
        installed_at: now,
        last_verified_at: now,
        raw_bytes: bytes.clone(),
    };
    runtime.invalidate_observed_binding_cache();
    runtime.update_cache(cache.clone());

    Ok(build_status_from_payload(
        LicenseUiState::Active,
        &cache.license,
        cache.installed_at,
        cache.last_verified_at,
        now,
    ))
}

#[tauri::command]
pub async fn license_status(
    runtime: State<'_, LicenseRuntime>,
) -> CmdResult<Option<LicenseStatusDto>> {
    let summary = runtime.summary();
    let now = summary
        .last_checked_at
        .unwrap_or_else(|| OffsetDateTime::now_utc().unix_timestamp());

    if summary.license.is_none() && summary.status == LicenseSummaryStatus::Missing {
        return Ok(None);
    }

    let ui_state = match summary.status {
        LicenseSummaryStatus::Active => LicenseUiState::Active,
        LicenseSummaryStatus::Expired => LicenseUiState::Expired,
        LicenseSummaryStatus::NotYetValid => LicenseUiState::NotYetValid,
        LicenseSummaryStatus::DeviceMismatch => LicenseUiState::DeviceMismatch,
        LicenseSummaryStatus::Invalid => LicenseUiState::Invalid,
        LicenseSummaryStatus::Missing => LicenseUiState::Missing,
    };

    let dto = if let Some(license) = summary.license {
        build_status_from_payload(
            ui_state,
            &license,
            summary.installed_at.unwrap_or(0),
            summary.last_verified_at.unwrap_or(0),
            now,
        )
    } else {
        build_placeholder_status(ui_state, runtime.device_hash_hex(), now)
    };

    Ok(Some(dto))
}

#[tauri::command]
pub async fn remove_license(
    app: AppHandle,
    runtime: State<'_, LicenseRuntime>,
    db: State<'_, Db>,
) -> CmdResult<()> {
    storage::delete_blob(&db.0).await.map_err(map_sqlx_error)?;
    runtime.mark_license_missing();

    if let Ok(path) = storage::current_license_path(&app) {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
    if let Ok(path) = storage::current_license_integrity_path(&app) {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }

    Ok(())
}

fn build_status_from_payload(
    ui_state: LicenseUiState,
    payload: &NormalizedLicense,
    installed_at: i64,
    last_verified_at: i64,
    last_checked_at: i64,
) -> LicenseStatusDto {
    LicenseStatusDto {
        status: ui_state,
        plan: payload.plan.clone().filter(|s| !s.is_empty()),
        customer_name: payload.customer_name.clone(),
        license_id: payload.license_id.clone(),
        not_before: payload.not_before,
        not_after: payload.not_after,
        max_clock_skew: payload.max_clock_skew,
        device_hash_hex: payload.device_hash_hex.clone(),
        installed_at,
        last_verified_at,
        last_checked_at,
        is_placeholder: false,
    }
}

fn build_placeholder_status(
    ui_state: LicenseUiState,
    device_hash_hex: String,
    last_checked_at: i64,
) -> LicenseStatusDto {
    LicenseStatusDto {
        status: ui_state,
        plan: None,
        customer_name: None,
        license_id: String::from("—"),
        not_before: 0,
        not_after: 0,
        max_clock_skew: 0,
        device_hash_hex,
        installed_at: 0,
        last_verified_at: 0,
        last_checked_at,
        is_placeholder: true,
    }
}

fn map_sqlx_error(err: sqlx::Error) -> CommandError {
    CommandError::io(err.to_string())
}

fn write_request_file(path: &Path, bytes: &[u8]) -> CmdResult<()> {
    write_atomic(path, bytes).map_err(|err| CommandError::io(err.to_string()))
}

#[derive(Debug, Clone)]
struct RequestTargets {
    exported_path: PathBuf,
    archived_path: Option<PathBuf>,
    archived_internally: bool,
}

fn build_request_targets(
    request_dir: &Path,
    filename: &str,
    destination_path: Option<&str>,
) -> RequestTargets {
    let internal_archive_path = request_dir.join(filename);
    let exported_path = destination_path
        .map(PathBuf::from)
        .unwrap_or_else(|| internal_archive_path.clone());
    let archived_path = if exported_path == internal_archive_path {
        None
    } else {
        Some(internal_archive_path)
    };
    let archived_internally = archived_path.is_some();

    RequestTargets {
        exported_path,
        archived_path,
        archived_internally,
    }
}

fn write_request_targets(targets: &RequestTargets, bytes: &[u8]) -> CmdResult<()> {
    write_request_file(&targets.exported_path, bytes)?;
    if let Some(path) = &targets.archived_path {
        write_request_file(path, bytes)?;
    }
    Ok(())
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

fn ensure_installable(status: LicenseRuntimeStatus) -> CmdResult<()> {
    if status == LicenseRuntimeStatus::Active {
        Ok(())
    } else {
        Err(runtime_status_error(status).unwrap_or_else(|| {
            CommandError::new(
                "InvalidLicenseState",
                "La licencia no se puede instalar en este dispositivo.",
            )
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    fn temp_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("license-request-test-{}", Uuid::new_v4()));
        dir.join("nested").join("request.req")
    }

    #[test]
    fn write_request_file_persists_bytes() {
        let path = temp_path();
        let bytes = vec![1u8, 2, 3, 4];
        write_request_file(&path, &bytes).expect("write request");
        let stored = fs::read(&path).expect("read back");
        assert_eq!(stored, bytes);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn build_request_targets_respects_user_selected_path() {
        let request_dir = std::env::temp_dir().join(format!("license-request-{}", Uuid::new_v4()));
        let selected_path = request_dir.join("Desktop").join("customer-visible.req");
        let targets = build_request_targets(
            &request_dir,
            "archive.req",
            Some(selected_path.to_string_lossy().as_ref()),
        );

        assert_eq!(targets.exported_path, selected_path);
        assert_eq!(targets.archived_path, Some(request_dir.join("archive.req")));
        assert!(targets.archived_internally);
    }

    #[test]
    fn write_request_targets_persists_internal_and_user_copy() {
        let root = std::env::temp_dir().join(format!("license-request-targets-{}", Uuid::new_v4()));
        let targets = RequestTargets {
            archived_path: Some(root.join("internal").join("archive.req")),
            exported_path: root.join("visible").join("customer.req"),
            archived_internally: true,
        };
        let bytes = b"LICREQ-test".to_vec();

        write_request_targets(&targets, &bytes).expect("write dual targets");

        assert_eq!(
            fs::read(targets.archived_path.as_ref().expect("archive path")).expect("read archive"),
            bytes
        );
        assert_eq!(
            fs::read(&targets.exported_path).expect("read exported"),
            bytes
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn build_request_targets_without_user_destination_exports_only_once() {
        let request_dir = std::env::temp_dir().join(format!("license-request-{}", Uuid::new_v4()));
        let targets = build_request_targets(&request_dir, "archive.req", None);

        assert_eq!(targets.exported_path, request_dir.join("archive.req"));
        assert_eq!(targets.archived_path, None);
        assert!(!targets.archived_internally);
    }
}
