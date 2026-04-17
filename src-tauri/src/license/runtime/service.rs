use std::sync::{Arc, RwLock};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use sqlx::SqlitePool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::license::validator::DEFAULT_APP_ID;

use crate::license::{
    storage, validator, CmdResult, CommandError, LicenseCache, LicenseState, NormalizedLicense,
};

use super::{device_binding::DeviceBindingStore, keyring::LicenseKeyring};

#[derive(Clone)]
pub struct LicenseRuntime {
    inner: Arc<LicenseRuntimeInner>,
}

struct LicenseRuntimeInner {
    binding: DeviceBindingStore,
    keyring: Arc<dyn LicenseKeyring + Send + Sync>,
    state: LicenseState,
    info: Arc<RwLock<RuntimeInfo>>,
}

const DEFAULT_ISSUER_KEY_ID: &str = "primary";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseSummaryStatus {
    Missing,
    Active,
    Expired,
    NotYetValid,
    DeviceMismatch,
    Invalid,
}

#[derive(Debug, Clone)]
pub struct RuntimeInfo {
    pub status: LicenseSummaryStatus,
    pub license: Option<NormalizedLicense>,
    pub installed_at: Option<i64>,
    pub last_verified_at: Option<i64>,
    pub last_checked_at: Option<i64>,
    pub last_error: Option<CommandError>,
}

impl Default for RuntimeInfo {
    fn default() -> Self {
        Self {
            status: LicenseSummaryStatus::Missing,
            license: None,
            installed_at: None,
            last_verified_at: None,
            last_checked_at: None,
            last_error: None,
        }
    }
}

impl LicenseRuntime {
    pub fn new(
        binding: DeviceBindingStore,
        keyring: Arc<dyn LicenseKeyring + Send + Sync>,
        state: LicenseState,
    ) -> Self {
        Self {
            inner: Arc::new(LicenseRuntimeInner {
                binding,
                keyring,
                state,
                info: Arc::new(RwLock::new(RuntimeInfo::default())),
            }),
        }
    }

    #[allow(dead_code)]
    pub fn binding(&self) -> &DeviceBindingStore {
        &self.inner.binding
    }

    pub fn license_state(&self) -> LicenseState {
        self.inner.state.clone()
    }

    pub fn summary(&self) -> RuntimeInfo {
        self.inner
            .info
            .read()
            .expect("runtime info poisoned")
            .clone()
    }

    #[allow(dead_code)]
    pub fn active_public_key(&self) -> ed25519_dalek::PublicKey {
        self.inner.keyring.active_key()
    }

    pub fn evaluate_license_bytes(
        &self,
        bytes: &[u8],
        now: i64,
    ) -> CmdResult<validator::LicenseEvaluation> {
        self.inner.binding.refresh_observed_binding()?;
        let device_hash = self.inner.binding.device_hash();
        let legacy_device_hash = self.inner.binding.legacy_device_hash();
        let installation_pubkey = STANDARD.encode(self.inner.binding.installation_pubkey());
        let installation_id = self.inner.binding.installation_id();
        validator::evaluate_license(
            self.inner.keyring.as_ref(),
            bytes,
            &device_hash,
            legacy_device_hash.as_ref(),
            &installation_id,
            &installation_pubkey,
            now,
        )
    }

    pub fn generate_request_bytes(
        &self,
        plan: &str,
        customer_name_hint: Option<String>,
    ) -> Result<(shared_core::Request, Vec<u8>), CommandError> {
        self.inner.binding.refresh_observed_binding()?;
        let created_at_ms = current_time_millis()?;
        let request_id = Uuid::new_v4().to_string();
        let plan = parse_plan(plan)?;
        let fingerprint = self.inner.binding.fingerprint();
        let installation_id = self.inner.binding.installation_id();
        let customer_name_hint = normalize_customer_name(customer_name_hint);
        let mut request = shared_core::Request {
            request_version: 2,
            auth_version: 2,
            request_id,
            created_at_ms,
            nonce: None,
            app_id: DEFAULT_APP_ID.to_string(),
            plan,
            issuer_key_id: DEFAULT_ISSUER_KEY_ID.to_string(),
            customer_name_hint,
            features: Some(vec![shared_core::Feature::Core]),
            policy_profile: Some(shared_core::PolicyProfile::Default),
            min_app_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            installation: shared_core::Installation {
                installation_id,
                installation_pubkey: STANDARD.encode(self.inner.binding.installation_pubkey()),
                fingerprint,
            },
            request_signature: shared_core::RequestSignature {
                algorithm: shared_core::SignatureAlgorithm::Ed25519,
                value: String::new(),
            },
        };
        let signing_bytes = shared_core::build_signing_bytes(&request)
            .map_err(|err| CommandError::parse(err.to_string()))?;
        let signature = self.inner.binding.sign_request(&signing_bytes);
        request.request_signature.value = STANDARD.encode(signature);
        let bytes = shared_core::encode_wire_request(&request)
            .map_err(|err| CommandError::parse(err.to_string()))?;
        let normalized = shared_core::verify_request(&bytes)
            .map_err(|err| CommandError::parse(err.to_string()))?
            .request;
        Ok((normalized, bytes))
    }

    pub fn ensure_active(&self) -> Result<LicenseCache, CommandError> {
        let summary = self.summary();
        if summary.status == LicenseSummaryStatus::Active {
            match crate::license::ensure_active(&self.inner.state) {
                Ok(cache) => {
                    let now = OffsetDateTime::now_utc().unix_timestamp();
                    self.record_active(
                        cache.license.clone(),
                        cache.installed_at,
                        cache.last_verified_at,
                        now,
                    );
                    Ok(cache)
                }
                Err(err) => {
                    self.handle_cache_error(&err);
                    Err(err)
                }
            }
        } else {
            Err(Self::status_error(summary.status, summary.last_error))
        }
    }

    pub fn update_cache(&self, cache: LicenseCache) {
        self.record_active(
            cache.license.clone(),
            cache.installed_at,
            cache.last_verified_at,
            OffsetDateTime::now_utc().unix_timestamp(),
        );
    }

    pub async fn reload_from_storage(&self, pool: &SqlitePool) -> anyhow::Result<()> {
        if let Some(record) = storage::load_blob(pool).await? {
            let now = OffsetDateTime::now_utc().unix_timestamp();
            let was_active = self.apply_stored_license(&record, now);
            if was_active && record.last_verified_at != now {
                storage::update_last_verified(pool, now).await?;
            }
        } else {
            let now = OffsetDateTime::now_utc().unix_timestamp();
            self.record_missing(now);
        }
        Ok(())
    }

    pub fn device_hash_hex(&self) -> String {
        self.inner.binding.device_hash_hex()
    }

    #[allow(dead_code)]
    pub fn device_hash(&self) -> [u8; 32] {
        self.inner.binding.device_hash()
    }

    pub fn mark_license_missing(&self) {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        self.record_missing(now);
    }

    fn set_info(
        &self,
        status: LicenseSummaryStatus,
        license: Option<NormalizedLicense>,
        installed_at: Option<i64>,
        last_verified_at: Option<i64>,
        last_checked_at: Option<i64>,
        error: Option<CommandError>,
    ) {
        let mut guard = self.inner.info.write().expect("runtime info poisoned");
        guard.status = status;
        guard.license = license;
        guard.installed_at = installed_at;
        guard.last_verified_at = last_verified_at;
        guard.last_checked_at = last_checked_at;
        guard.last_error = error;
    }

    fn record_missing(&self, checked_at: i64) {
        self.inner.state.replace(None);
        self.set_info(
            LicenseSummaryStatus::Missing,
            None,
            None,
            None,
            Some(checked_at),
            None,
        );
    }

    fn record_invalid(&self, err: CommandError, checked_at: i64) {
        self.inner.state.replace(None);
        self.set_info(
            LicenseSummaryStatus::Invalid,
            None,
            None,
            None,
            Some(checked_at),
            Some(err),
        );
    }

    fn record_inactive(
        &self,
        status: LicenseSummaryStatus,
        license: NormalizedLicense,
        installed_at: i64,
        last_verified_at: i64,
        checked_at: i64,
    ) {
        self.inner.state.replace(None);
        self.set_info(
            status,
            Some(license),
            Some(installed_at),
            Some(last_verified_at),
            Some(checked_at),
            None,
        );
    }

    fn record_active(
        &self,
        license: NormalizedLicense,
        installed_at: i64,
        last_verified_at: i64,
        checked_at: i64,
    ) {
        self.inner.state.replace(Some(LicenseCache {
            license: license.clone(),
            installed_at,
            last_verified_at,
        }));
        self.set_info(
            LicenseSummaryStatus::Active,
            Some(license),
            Some(installed_at),
            Some(last_verified_at),
            Some(checked_at),
            None,
        );
    }

    fn cache_to_inactive_status(&self, status: LicenseSummaryStatus, checked_at: i64) {
        if let Some(cache) = self.inner.state.snapshot() {
            self.record_inactive(
                status,
                cache.license,
                cache.installed_at,
                cache.last_verified_at,
                checked_at,
            );
        } else {
            self.record_missing(checked_at);
        }
    }

    fn map_runtime_status(status: validator::LicenseRuntimeStatus) -> LicenseSummaryStatus {
        match status {
            validator::LicenseRuntimeStatus::Active => LicenseSummaryStatus::Active,
            validator::LicenseRuntimeStatus::Expired => LicenseSummaryStatus::Expired,
            validator::LicenseRuntimeStatus::NotYetValid => LicenseSummaryStatus::NotYetValid,
            validator::LicenseRuntimeStatus::DeviceMismatch => LicenseSummaryStatus::DeviceMismatch,
        }
    }

    fn handle_cache_error(&self, err: &CommandError) {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        match err.code.as_str() {
            "LicenseRequired" => self.record_missing(now),
            "Expired" => self.cache_to_inactive_status(LicenseSummaryStatus::Expired, now),
            "NotYetValid" => self.cache_to_inactive_status(LicenseSummaryStatus::NotYetValid, now),
            _ => {}
        }
    }

    fn status_error(
        status: LicenseSummaryStatus,
        last_error: Option<CommandError>,
    ) -> CommandError {
        match status {
            LicenseSummaryStatus::Missing => CommandError::new(
                "LicenseRequired",
                "Instala una licencia válida para continuar.",
            ),
            LicenseSummaryStatus::Expired => CommandError::new(
                "Expired",
                "La licencia ha expirado. Instala una nueva para continuar.",
            ),
            LicenseSummaryStatus::NotYetValid => CommandError::new(
                "NotYetValid",
                "La licencia aún no es válida en este dispositivo.",
            ),
            LicenseSummaryStatus::DeviceMismatch => CommandError::new(
                "DeviceMismatch",
                "La licencia pertenece a otro dispositivo.",
            ),
            LicenseSummaryStatus::Invalid => last_error.unwrap_or_else(|| {
                CommandError::new(
                    "Invalid",
                    "La licencia almacenada es inválida; reinstálala desde el generador.",
                )
            }),
            LicenseSummaryStatus::Active => CommandError::new(
                "LicenseRequired",
                "Instala una licencia válida para continuar.",
            ),
        }
    }

    fn apply_stored_license(&self, record: &storage::StoredLicenseBlob, now: i64) -> bool {
        match self.evaluate_license_bytes(&record.raw_bytes, now) {
            Ok(evaluation) => {
                let status = Self::map_runtime_status(evaluation.status);
                if status == LicenseSummaryStatus::Active {
                    self.record_active(evaluation.license, record.installed_at, now, now);
                    true
                } else {
                    tracing::info!("License bootstrap classification: {:?}", status);
                    self.record_inactive(
                        status,
                        evaluation.license,
                        record.installed_at,
                        record.last_verified_at,
                        now,
                    );
                    false
                }
            }
            Err(err) => {
                tracing::warn!(
                    "Failed to bootstrap license: {} ({})",
                    err.message,
                    err.code
                );
                self.record_invalid(err, now);
                false
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn apply_stored_license_for_test(
        &self,
        record: &storage::StoredLicenseBlob,
        now: i64,
    ) -> bool {
        self.apply_stored_license(record, now)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn record_invalid_for_test(&self, err: CommandError, now: i64) {
        self.record_invalid(err, now);
    }
}

fn parse_plan(plan: &str) -> Result<shared_core::Plan, CommandError> {
    match plan {
        "monthly" => Ok(shared_core::Plan::Monthly),
        "yearly" => Ok(shared_core::Plan::Yearly),
        "per_event" => Ok(shared_core::Plan::PerEvent),
        other => Err(CommandError::parse(format!("invalid plan {other}"))),
    }
}

fn normalize_customer_name(customer_name_hint: Option<String>) -> Option<String> {
    customer_name_hint.and_then(|value| {
        let normalized = value
            .split_whitespace()
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    })
}

fn current_time_millis() -> Result<u64, CommandError> {
    let millis = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
    u64::try_from(millis).map_err(|_| CommandError::parse("created_at_ms out of range"))
}

#[cfg(test)]
mod tests {
    use crate::license::validator::DEFAULT_APP_ID;
    use ed25519_dalek::Signature;
    use time::OffsetDateTime;

    use super::*;

    fn temp_binding() -> DeviceBindingStore {
        let dir =
            std::env::temp_dir().join(format!("license-runtime-binding-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        DeviceBindingStore::load_or_init_from_dir(dir).expect("init binding")
    }

    fn sample_license(device_hash: [u8; 32]) -> NormalizedLicense {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        NormalizedLicense {
            format: crate::license::LicenseFormatKind::ModernLicgen,
            format_version: 1,
            app_id: DEFAULT_APP_ID.into(),
            signature_valid: true,
            key_id: Some("1".into()),
            key_version: None,
            license_id: "LIC-RUNTIME-TEST".into(),
            plan: Some("monthly".into()),
            customer_name: Some("Test".into()),
            issued_at: 1_700_000_000,
            not_before: now - 60,
            not_after: now + 60,
            max_clock_skew: 30,
            max_offline_days: 30,
            lease_required: false,
            revocation_epoch: None,
            allowed_fingerprints_count: 0,
            device_hash_hex: hex::encode(device_hash),
            installation_id: None,
            installation_pubkey: None,
            binding: crate::license::BindingMatch::Current,
            blob_len: 128,
            blob_sha256: "deadbeef".into(),
            failure_reason: None,
        }
    }

    #[test]
    fn ensure_active_without_cache_fails() {
        let runtime = LicenseRuntime::new(
            temp_binding(),
            super::super::default_keyring(),
            LicenseState::default(),
        );
        let result = runtime.ensure_active();
        assert!(result.is_err());
    }

    #[test]
    fn ensure_active_with_cache_succeeds() {
        let runtime = LicenseRuntime::new(
            temp_binding(),
            super::super::default_keyring(),
            LicenseState::default(),
        );
        let license = sample_license(runtime.binding().device_hash());
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let cache = LicenseCache {
            license,
            installed_at: now,
            last_verified_at: now,
        };
        runtime.update_cache(cache.clone());
        let result = runtime.ensure_active().expect("active license");
        assert_eq!(result.license.license_id, cache.license.license_id);
    }

    #[test]
    fn e2e_probe_save_request_to_disk() {
        let runtime = LicenseRuntime::new(
            temp_binding(),
            super::super::default_keyring(),
            LicenseState::default(),
        );
        let (_request, bytes) = runtime
            .generate_request_bytes("monthly", Some("E2E Integration Test".into()))
            .expect("request bytes");
        let out = std::path::PathBuf::from("/tmp/e2e_integration_request.req");
        std::fs::write(&out, &bytes).expect("write request to disk");
        println!("E2E_REQUEST_PATH={}", out.display());
        println!("E2E_REQUEST_LEN={}", bytes.len());
        println!(
            "E2E_REQUEST_MAGIC={:?}",
            std::str::from_utf8(&bytes[..6]).unwrap_or("<binary>")
        );
        println!("E2E_APP_ID={}", _request.app_id);
        println!("E2E_PLAN={}", _request.plan.as_str());
        println!(
            "E2E_INSTALLATION_ID={}",
            _request.installation.installation_id
        );
        println!(
            "E2E_HARDWARE_HASH={}",
            _request.installation.fingerprint.hardware_hash
        );
        println!("E2E_ISSUER_KEY_ID={}", _request.issuer_key_id);
        println!("E2E_REQUEST_VERSION={}", _request.request_version);
        println!("E2E_AUTH_VERSION={}", _request.auth_version);
        assert_eq!(
            bytes[..6],
            *b"LICREQ",
            "magic must be LICREQ for modern wire format"
        );
    }

    #[test]
    fn generates_request_roundtrip() {
        let runtime = LicenseRuntime::new(
            temp_binding(),
            super::super::default_keyring(),
            LicenseState::default(),
        );
        let (request, bytes) = runtime
            .generate_request_bytes("monthly", Some("Cliente Demo".into()))
            .expect("request bytes");
        assert_eq!(request.plan.as_str(), "monthly");
        assert_eq!(request.app_id, DEFAULT_APP_ID);
        let reparsed = shared_core::verify_request(&bytes).expect("parse bytes");
        assert_eq!(
            reparsed.request.installation.fingerprint.hardware_hash,
            request.installation.fingerprint.hardware_hash
        );
    }

    #[test]
    fn request_signature_matches_installation_key() {
        let runtime = LicenseRuntime::new(
            temp_binding(),
            super::super::default_keyring(),
            LicenseState::default(),
        );
        let (request, _) = runtime
            .generate_request_bytes("monthly", Some("Cliente Demo".into()))
            .expect("request bytes");
        assert!(!request.installation.installation_id.is_empty());
        let signing = shared_core::build_signing_bytes(&request).expect("signing payload bytes");
        let pubkey_bytes = STANDARD
            .decode(&request.installation.installation_pubkey)
            .expect("pubkey b64");
        let pubkey = ed25519_dalek::PublicKey::from_bytes(&pubkey_bytes).unwrap();
        let sig_buf: [u8; 64] = STANDARD
            .decode(&request.request_signature.value)
            .expect("signature b64")
            .try_into()
            .expect("signature length");
        let signature = Signature::from_bytes(&sig_buf).unwrap();
        pubkey
            .verify_strict(&signing, &signature)
            .expect("signature valid");
    }
}
