use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{TimeZone, Utc};
use licgen_core::crypto::license_verifier_from_pubkey;
use licgen_core::license::LicenseVerifier;
use licgen_core::{
    ClockGuardMode, ComponentSource, DeviceFingerprintV2, FingerprintBindingBundle,
    FingerprintCheckInput, FingerprintComponent, FingerprintComponentKind, FingerprintObservation,
    FingerprintOverridePolicy, LicenseError, LicensePayloadV5, LicenseVerificationHandle,
    SecurityProfile, SnapshotMode, VerificationContext, VerificationEnvironment,
};
use sqlx::SqlitePool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::license::validator::DEFAULT_APP_ID;

use crate::license::{
    storage, validator, CmdResult, CommandError, LicenseCache, LicenseState, NormalizedLicense,
};

use super::{
    device_binding::DeviceBindingStore,
    keyring::{KeyLookupError, LicenseKeyring},
};

#[derive(Clone)]
pub struct LicenseRuntime {
    inner: Arc<LicenseRuntimeInner>,
}

struct LicenseRuntimeInner {
    binding: DeviceBindingStore,
    keyring: Arc<dyn LicenseKeyring + Send + Sync>,
    state: LicenseState,
    verification_state_root: PathBuf,
    verification_environment: VerificationEnvironment,
    verification_lock: Mutex<()>,
    info: Arc<RwLock<RuntimeInfo>>,
}

const DEFAULT_ISSUER_KEY_ID: &str = "primary";

fn map_key_lookup_error(err: KeyLookupError) -> CommandError {
    match err {
        KeyLookupError::UnknownKeyId { key_id } => CommandError::new(
            "UnknownKeyId",
            format!("La licencia usa key_id desconocido: {key_id}"),
        ),
        KeyLookupError::KeyVersionMismatch {
            key_id,
            key_version,
        } => CommandError::new(
            "KeyVersionMismatch",
            format!(
                "key_version incompatible para key_id={key_id}: recibido {:?}",
                key_version
            ),
        ),
        KeyLookupError::RetiredKey {
            key_id,
            key_version,
        } => CommandError::new(
            "RetiredKey",
            format!(
                "La licencia usa una llave retirada: key_id={key_id}, key_version={:?}",
                key_version
            ),
        ),
    }
}

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
        verification_state_root: PathBuf,
        verification_environment: VerificationEnvironment,
    ) -> Self {
        Self {
            inner: Arc::new(LicenseRuntimeInner {
                binding,
                keyring,
                state,
                verification_state_root,
                verification_environment,
                verification_lock: Mutex::new(()),
                info: Arc::new(RwLock::new(RuntimeInfo::default())),
            }),
        }
    }

    #[allow(dead_code)]
    pub fn binding(&self) -> &DeviceBindingStore {
        &self.inner.binding
    }

    pub fn invalidate_observed_binding_cache(&self) {
        self.inner.binding.invalidate_observed_binding_cache();
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
        self.verify_license_bytes(bytes, now, SnapshotMode::Bootstrap)
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
        let now = OffsetDateTime::now_utc().unix_timestamp();
        self.ensure_active_at(now)
    }

    fn ensure_active_at(&self, now: i64) -> Result<LicenseCache, CommandError> {
        let summary = self.summary();
        let cache = match self.inner.state.snapshot() {
            Some(cache) => cache,
            None => return Err(Self::status_error(summary.status, summary.last_error)),
        };
        if let Err(err) = self.validate_local_license_material(&cache.raw_bytes) {
            self.record_invalid(err.clone(), now);
            return Err(err);
        }
        match self.verify_license_bytes(&cache.raw_bytes, now, SnapshotMode::Strict) {
            Ok(evaluation) => {
                let status = Self::map_runtime_status(evaluation.status);
                if status == LicenseSummaryStatus::Active {
                    let cache = LicenseCache {
                        license: evaluation.license.clone(),
                        installed_at: cache.installed_at,
                        last_verified_at: now,
                        raw_bytes: cache.raw_bytes,
                    };
                    self.update_cache(cache.clone());
                    Ok(cache)
                } else {
                    self.record_inactive(status, evaluation.license, cache.installed_at, now, now);
                    Err(Self::status_error(status, None))
                }
            }
            Err(err) => {
                self.handle_cache_error(&err);
                Err(err)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn ensure_active_at_for_test(&self, now: i64) -> Result<LicenseCache, CommandError> {
        if let Some(cache) = self.inner.state.snapshot() {
            if !cache.raw_bytes.is_empty() {
                self.seed_local_license_material_for_test(&cache.raw_bytes);
            }
        }
        self.ensure_active_at(now)
    }

    pub fn update_cache(&self, cache: LicenseCache) {
        self.record_active(
            cache.license.clone(),
            cache.installed_at,
            cache.last_verified_at,
            OffsetDateTime::now_utc().unix_timestamp(),
            &cache.raw_bytes,
        );
    }

    pub async fn reload_from_storage(&self, pool: &SqlitePool) -> anyhow::Result<()> {
        if let Some(record) = storage::load_blob(pool).await? {
            let now = OffsetDateTime::now_utc().unix_timestamp();
            let _was_active = self.apply_stored_license(&record, now);
            if self.summary().last_verified_at == Some(now) && record.last_verified_at != now {
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

    fn verify_license_bytes(
        &self,
        bytes: &[u8],
        now: i64,
        snapshot_mode: SnapshotMode,
    ) -> CmdResult<validator::LicenseEvaluation> {
        let _verification_guard = self
            .inner
            .verification_lock
            .lock()
            .expect("verification lock poisoned");
        self.inner.binding.refresh_observed_binding()?;
        let now_dt = timestamp_to_utc(now)?;
        let (payload, _) = licgen_core::format::decode_signed_license(bytes)
            .map_err(map_hardened_error_from_decode)?;
        let key_id = payload.security_policy.key_id.clone().ok_or_else(|| {
            CommandError::new("MissingKeyId", "La licencia moderna no declara key_id.")
        })?;
        let key_version = payload.security_policy.key_version.as_deref();
        let public_key = self
            .inner
            .keyring
            .lookup_key(&key_id, key_version)
            .map_err(map_key_lookup_error)?
            .public_key;
        let verifier_provider =
            license_verifier_from_pubkey(key_id.clone(), &public_key.to_bytes())
                .map_err(map_hardened_error)?;
        let verifier = LicenseVerifier::new(&verifier_provider);
        let context = self.build_verification_context(snapshot_mode)?;
        let observed = project_shared_fingerprint(&self.inner.binding.fingerprint());
        let verified = match LicenseVerificationHandle::new(&verifier, &context).verify_signed_blob(
            bytes,
            FingerprintCheckInput {
                observed: Some(&observed),
                policy: FingerprintOverridePolicy::strict(),
            },
            now_dt,
        ) {
            Ok(verified) => verified,
            Err(err) if is_fingerprint_validation_error(&err) => {
                self.inner.binding.invalidate_observed_binding_cache();
                return Ok(build_hardened_evaluation(
                    &payload,
                    bytes,
                    &self.inner.binding,
                    now,
                ));
            }
            Err(err) => return Err(map_hardened_error(err)),
        };
        Ok(build_hardened_evaluation(
            &verified.payload,
            bytes,
            &self.inner.binding,
            now,
        ))
    }

    fn build_verification_context(
        &self,
        snapshot_mode: SnapshotMode,
    ) -> CmdResult<VerificationContext> {
        let installation_id = self.inner.binding.installation_id();
        let snapshot_secret = self
            .inner
            .binding
            .key_store()
            .derive_secret(b"license-snapshot", installation_id.as_bytes());
        VerificationContext::new(
            SecurityProfile::for_environment(self.inner.verification_environment),
            self.inner.verification_state_root.clone(),
            snapshot_secret,
            snapshot_mode,
            ClockGuardMode::Enforced,
        )
        .map_err(map_hardened_error)
    }

    fn local_license_integrity_secret(&self) -> Vec<u8> {
        self.inner.binding.key_store().derive_secret(
            storage::LOCAL_LICENSE_INTEGRITY_PURPOSE,
            self.inner.binding.installation_id().as_bytes(),
        )
    }

    fn validate_local_license_material(&self, expected_bytes: &[u8]) -> CmdResult<()> {
        let integrity_secret = self.local_license_integrity_secret();
        storage::validate_current_license_integrity_under_root(
            &self.inner.verification_state_root,
            expected_bytes,
            &integrity_secret,
        )
    }

    #[cfg(test)]
    fn seed_local_license_material_for_test(&self, bytes: &[u8]) {
        let integrity_secret = self.local_license_integrity_secret();
        storage::persist_current_license_file_under_root(&self.inner.verification_state_root, bytes)
            .expect("persist local license bytes for test");
        storage::persist_current_license_integrity_under_root(
            &self.inner.verification_state_root,
            bytes,
            &integrity_secret,
        )
        .expect("persist local license integrity for test");
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
        raw_bytes: &[u8],
    ) {
        self.inner.state.replace(Some(LicenseCache {
            license: license.clone(),
            installed_at,
            last_verified_at,
            raw_bytes: raw_bytes.to_vec(),
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
            "DeviceMismatch" => {
                self.cache_to_inactive_status(LicenseSummaryStatus::DeviceMismatch, now)
            }
            _ => self.record_invalid(err.clone(), now),
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
        if let Err(err) = self.validate_local_license_material(&record.raw_bytes) {
            tracing::warn!(
                "Detected inconsistent local license storage: {} ({})",
                err.message,
                err.code
            );
            self.record_invalid(err, now);
            return false;
        }
        match self.verify_license_bytes(&record.raw_bytes, now, SnapshotMode::Strict) {
            Ok(evaluation) => {
                let status = Self::map_runtime_status(evaluation.status);
                if status == LicenseSummaryStatus::Active {
                    self.record_active(
                        evaluation.license,
                        record.installed_at,
                        now,
                        now,
                        &record.raw_bytes,
                    );
                    true
                } else {
                    tracing::info!("License bootstrap classification: {:?}", status);
                    self.record_inactive(status, evaluation.license, record.installed_at, now, now);
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
        self.seed_local_license_material_for_test(&record.raw_bytes);
        self.apply_stored_license(record, now)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn record_invalid_for_test(&self, err: CommandError, now: i64) {
        self.record_invalid(err, now);
    }
}

fn build_hardened_evaluation(
    payload: &LicensePayloadV5,
    license_bytes: &[u8],
    binding: &DeviceBindingStore,
    now: i64,
) -> validator::LicenseEvaluation {
    let status = classify_runtime_status(payload, binding, now);
    let binding_match = classify_binding(payload, binding);
    let license = NormalizedLicense {
        format: crate::license::LicenseFormatKind::ModernLicgen,
        format_version: shared_core::licgen_envelope::FORMAT_VERSION,
        app_id: payload
            .metadata
            .get("app_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        signature_valid: true,
        key_id: payload.security_policy.key_id.clone(),
        key_version: payload.security_policy.key_version.clone(),
        license_id: payload.license_id.to_string(),
        plan: payload
            .metadata
            .get("plan")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        customer_name: payload
            .metadata
            .get("customer_name")
            .or_else(|| payload.metadata.get("customer_name_hint"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        issued_at: payload.issued_at.timestamp(),
        not_before: payload.issued_at.timestamp(),
        not_after: payload.expires_at.timestamp(),
        max_clock_skew: crate::license::modern::DEFAULT_MAX_CLOCK_SKEW_SECS,
        max_offline_days: payload.offline_policy.max_offline_days,
        lease_required: payload.offline_policy.lease_required,
        revocation_epoch: payload.security_policy.revocation_epoch,
        allowed_fingerprints_count: payload.security_policy.allowed_fingerprints.len(),
        device_hash_hex: payload.device_fingerprint_v2.hardware_hash.clone(),
        installation_id: Some(payload.installation.installation_id.to_string()),
        installation_pubkey: payload.installation.installation_pubkey.clone(),
        binding: binding_match,
        blob_len: license_bytes.len(),
        blob_sha256: sha256_hex(license_bytes),
        failure_reason: failure_reason_for_status(status),
    };

    validator::LicenseEvaluation { license, status }
}

fn classify_runtime_status(
    payload: &LicensePayloadV5,
    binding: &DeviceBindingStore,
    now: i64,
) -> validator::LicenseRuntimeStatus {
    let binding_match = classify_binding(payload, binding);
    if binding_match == crate::license::BindingMatch::Mismatch {
        return validator::LicenseRuntimeStatus::DeviceMismatch;
    }

    let not_before = payload.issued_at.timestamp();
    let not_after = payload.expires_at.timestamp();
    let max_clock_skew = crate::license::modern::DEFAULT_MAX_CLOCK_SKEW_SECS;
    if now.saturating_add(max_clock_skew) < not_before {
        return validator::LicenseRuntimeStatus::NotYetValid;
    }
    if now.saturating_sub(max_clock_skew) > not_after {
        return validator::LicenseRuntimeStatus::Expired;
    }
    validator::LicenseRuntimeStatus::Active
}

fn classify_binding(
    payload: &LicensePayloadV5,
    binding: &DeviceBindingStore,
) -> crate::license::BindingMatch {
    let installation_id_matches =
        payload.installation.installation_id.to_string() == binding.installation_id();
    let current_pubkey = STANDARD.encode(binding.installation_pubkey());
    let installation_pubkey_matches = payload
        .installation
        .installation_pubkey
        .as_deref()
        .is_none_or(|value| value == current_pubkey);
    if !installation_id_matches || !installation_pubkey_matches {
        return crate::license::BindingMatch::Mismatch;
    }

    let current_hash = binding.device_hash_hex();
    if payload.device_fingerprint_v2.hardware_hash == current_hash {
        return crate::license::BindingMatch::Current;
    }
    if binding
        .legacy_device_hash()
        .map(hex::encode)
        .as_deref()
        .is_some_and(|legacy| legacy == payload.device_fingerprint_v2.hardware_hash)
    {
        return crate::license::BindingMatch::LegacyCompat;
    }
    crate::license::BindingMatch::Mismatch
}

fn failure_reason_for_status(
    status: validator::LicenseRuntimeStatus,
) -> Option<crate::license::NormalizedFailureReason> {
    match status {
        validator::LicenseRuntimeStatus::Active => None,
        validator::LicenseRuntimeStatus::NotYetValid => {
            Some(crate::license::NormalizedFailureReason::NotYetValid)
        }
        validator::LicenseRuntimeStatus::Expired => {
            Some(crate::license::NormalizedFailureReason::Expired)
        }
        validator::LicenseRuntimeStatus::DeviceMismatch => {
            Some(crate::license::NormalizedFailureReason::DeviceMismatch)
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn timestamp_to_utc(timestamp: i64) -> CmdResult<chrono::DateTime<Utc>> {
    Utc.timestamp_opt(timestamp, 0)
        .single()
        .ok_or_else(|| CommandError::parse("timestamp fuera de rango"))
}

fn project_shared_fingerprint(fingerprint: &shared_core::Fingerprint) -> DeviceFingerprintV2 {
    DeviceFingerprintV2 {
        version: fingerprint.version,
        hardware_hash: fingerprint.hardware_hash.clone(),
        platform: fingerprint.platform.as_str().to_string(),
        components: fingerprint
            .binding
            .stable
            .iter()
            .chain(fingerprint.binding.strict.iter())
            .map(|component| component.kind.as_str().to_string())
            .collect(),
        binding: FingerprintBindingBundle {
            stable: fingerprint
                .binding
                .stable
                .iter()
                .map(project_component)
                .collect(),
            strict: fingerprint
                .binding
                .strict
                .iter()
                .map(project_component)
                .collect(),
            observations: fingerprint
                .binding
                .observations
                .iter()
                .map(project_observation)
                .collect(),
        },
    }
}

fn project_component(component: &shared_core::FingerprintComponent) -> FingerprintComponent {
    FingerprintComponent {
        kind: project_component_kind(component.kind),
        hash: component.hash.clone(),
        weight: component.weight,
        source: project_component_source(component.source),
    }
}

fn project_observation(
    observation: &shared_core::FingerprintObservation,
) -> FingerprintObservation {
    FingerprintObservation {
        kind: FingerprintComponentKind::Custom(observation.kind.as_str().to_string()),
        value: observation.value.clone(),
        note: None,
    }
}

fn project_component_kind(kind: shared_core::ComponentKind) -> FingerprintComponentKind {
    match kind {
        shared_core::ComponentKind::InstallationAnchor => {
            FingerprintComponentKind::InstallationAnchor
        }
        shared_core::ComponentKind::MachineId => FingerprintComponentKind::MachineId,
        shared_core::ComponentKind::DiskSerial => FingerprintComponentKind::DiskSerial,
        shared_core::ComponentKind::MotherboardUuid => FingerprintComponentKind::MotherboardUuid,
        shared_core::ComponentKind::BiosUuid => FingerprintComponentKind::BiosUuid,
        shared_core::ComponentKind::CpuModel => FingerprintComponentKind::CpuModel,
        shared_core::ComponentKind::MacAddress => FingerprintComponentKind::MacAddress,
        shared_core::ComponentKind::Hostname => FingerprintComponentKind::Hostname,
        shared_core::ComponentKind::OsInstallId => FingerprintComponentKind::OsInstallId,
    }
}

fn project_component_source(source: shared_core::ComponentSource) -> ComponentSource {
    match source {
        shared_core::ComponentSource::System => ComponentSource::System,
        shared_core::ComponentSource::Installer => ComponentSource::Installer,
        shared_core::ComponentSource::Operator => ComponentSource::Operator,
    }
}

fn map_hardened_error_from_decode(err: LicenseError) -> CommandError {
    match err {
        LicenseError::InvalidMagic { .. } => CommandError::new(
            "LegacyUnsupported",
            "This installation only accepts modern LICGEN licenses. Legacy blobs are no longer supported.",
        ),
        other => map_hardened_error(other),
    }
}

fn map_hardened_error(err: LicenseError) -> CommandError {
    match err {
        LicenseError::InvalidSignature { .. } => CommandError::new(
            "SignatureFailed",
            "La firma de la licencia moderna es inválida.",
        ),
        LicenseError::LicenseExpired { .. } => CommandError::new(
            "Expired",
            "La licencia ha expirado. Instala una nueva para continuar.",
        ),
        LicenseError::SnapshotMissing { .. } => {
            CommandError::new("SnapshotMissing", err.to_string())
        }
        LicenseError::SnapshotCorrupted { .. } => {
            CommandError::new("SnapshotCorrupted", err.to_string())
        }
        LicenseError::SnapshotVersionMismatch { .. } => {
            CommandError::new("SnapshotVersionMismatch", err.to_string())
        }
        LicenseError::SnapshotReplay { .. } => CommandError::new("SnapshotReplay", err.to_string()),
        LicenseError::ClockRollback { .. } => CommandError::new("ClockRollback", err.to_string()),
        LicenseError::Validation { field, message } if field.starts_with("fingerprint") => {
            CommandError::new("FingerprintMismatch", message)
        }
        LicenseError::Validation { field, message } if field == "security_policy.key_id" => {
            CommandError::new("UnknownKeyId", message)
        }
        LicenseError::UnsupportedPolicy { field, message } => {
            map_unsupported_policy_error(field, message)
        }
        LicenseError::MissingField {
            field: "security_policy.key_id",
        } => CommandError::new("MissingKeyId", "La licencia moderna no declara key_id."),
        LicenseError::IncompatibleVersion { .. } => {
            CommandError::new("LegacyUnsupported", err.to_string())
        }
        LicenseError::InvalidFormat { .. }
        | LicenseError::PayloadTooLarge { .. }
        | LicenseError::MissingField { .. }
        | LicenseError::Validation { .. }
        | LicenseError::Serialization(_)
        | LicenseError::Legacy(_)
        | LicenseError::SnapshotConfig(_) => CommandError::parse(err.to_string()),
        LicenseError::Io(_) => CommandError::io(err.to_string()),
        LicenseError::InvalidMagic { .. } => {
            CommandError::new("LegacyUnsupported", err.to_string())
        }
        LicenseError::SnapshotMismatch { .. } => {
            CommandError::new("SnapshotCorrupted", err.to_string())
        }
        LicenseError::LeaseExpired { .. } => CommandError::new("Expired", err.to_string()),
    }
}

fn map_unsupported_policy_error(field: &'static str, message: String) -> CommandError {
    match field {
        "offline_policy.lease_required" => CommandError::new("LeaseUnsupported", message),
        "security_policy.revocation_epoch" => CommandError::new("RevocationUnsupported", message),
        "offline_policy.grace_days"
        | "offline_policy.last_online_check_at"
        | "installation.last_online_check_at" => {
            CommandError::new("HybridPolicyUnsupported", message)
        }
        _ => CommandError::new("UnsupportedPolicy", message),
    }
}

fn is_fingerprint_validation_error(err: &LicenseError) -> bool {
    matches!(
        err,
        LicenseError::Validation { field, .. } if field.starts_with("fingerprint")
    )
}

pub fn verification_environment_for_keyring_env(
    keyring_env: &str,
) -> CmdResult<VerificationEnvironment> {
    match keyring_env {
        "prod" => Ok(VerificationEnvironment::Production),
        "staging" => Ok(VerificationEnvironment::Staging),
        "dev" => Ok(VerificationEnvironment::Development),
        other => Err(CommandError::parse(format!(
            "unsupported keyring env {other}"
        ))),
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
    use crate::license::runtime::fingerprint::{
        collect_fingerprint_with_observer, fingerprint_hardware_hash_bytes, HardwareObserver,
        ObservedHardware,
    };
    use crate::license::validator::DEFAULT_APP_ID;
    use chrono::{TimeZone, Utc};
    use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signature};
    use licgen_core::crypto::{Ed25519CryptoProvider, Ed25519Keypair, LicenseCryptoProvider};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier, RwLock as StdRwLock,
    };
    use std::thread;
    use std::time::Duration;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::*;

    #[derive(Clone)]
    struct FixedObserver(ObservedHardware);

    impl HardwareObserver for FixedObserver {
        fn observe(&self) -> crate::license::CmdResult<ObservedHardware> {
            Ok(self.0.clone())
        }
    }

    #[derive(Clone)]
    struct MutableCountingObserver {
        observed: Arc<StdRwLock<ObservedHardware>>,
        calls: Arc<AtomicUsize>,
    }

    impl HardwareObserver for MutableCountingObserver {
        fn observe(&self) -> crate::license::CmdResult<ObservedHardware> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self
                .observed
                .read()
                .expect("observer state poisoned")
                .clone())
        }
    }

    fn test_observer() -> std::sync::Arc<dyn HardwareObserver + Send + Sync> {
        std::sync::Arc::new(FixedObserver(ObservedHardware {
            platform: shared_core::Platform::Macos,
            machine_id: Some("machine-service".into()),
            disk_serial: Some("disk-service".into()),
            cpu_model: Some("Apple M1".into()),
            hostname: Some("service-host".into()),
            locale: Some("en_US.UTF-8".into()),
            timezone: "-0600".into(),
        }))
    }

    fn temp_binding_root() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("license-runtime-binding-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn temp_runtime() -> LicenseRuntime {
        let dir = temp_binding_root();
        let binding =
            DeviceBindingStore::load_or_init_from_dir_with_observer(&dir, test_observer())
                .expect("init binding");
        LicenseRuntime::new(
            binding,
            super::super::default_keyring(),
            LicenseState::default(),
            dir,
            VerificationEnvironment::Development,
        )
    }

    #[derive(Clone)]
    struct FixedKeyring(PublicKey);

    impl super::super::keyring::LicenseKeyring for FixedKeyring {
        fn active_key(&self) -> PublicKey {
            self.0
        }

        fn resolve_key(&self, key_id: &str) -> Option<PublicKey> {
            (key_id == "primary").then_some(self.0)
        }
    }

    fn runtime_with_fixed_key(env: VerificationEnvironment) -> (PathBuf, LicenseRuntime, [u8; 32]) {
        let dir = temp_binding_root();
        let binding =
            DeviceBindingStore::load_or_init_from_dir_with_observer(&dir, test_observer())
                .expect("init binding");
        let seed = [0x51; 32];
        let keypair = test_keypair(0x51);
        let keyring: Arc<dyn super::super::keyring::LicenseKeyring + Send + Sync> =
            Arc::new(FixedKeyring(keypair.public));
        let runtime =
            LicenseRuntime::new(binding, keyring, LicenseState::default(), dir.clone(), env);
        (dir, runtime, seed)
    }

    fn runtime_with_fixed_key_and_observer_ttl(
        env: VerificationEnvironment,
        observer: Arc<dyn HardwareObserver + Send + Sync>,
        ttl: Duration,
    ) -> (PathBuf, LicenseRuntime, [u8; 32]) {
        let dir = temp_binding_root();
        let binding = DeviceBindingStore::load_or_init_from_dir_with_observer_and_cache_ttl(
            &dir, observer, ttl,
        )
        .expect("init binding");
        let seed = [0x51; 32];
        let keypair = test_keypair(0x51);
        let keyring: Arc<dyn super::super::keyring::LicenseKeyring + Send + Sync> =
            Arc::new(FixedKeyring(keypair.public));
        let runtime =
            LicenseRuntime::new(binding, keyring, LicenseState::default(), dir.clone(), env);
        (dir, runtime, seed)
    }

    fn issue_runtime_license(
        runtime: &LicenseRuntime,
        seed: &[u8; 32],
        issued_at: i64,
        expires_at: i64,
    ) -> Vec<u8> {
        issue_runtime_license_with_mutator(runtime, seed, issued_at, expires_at, |_| {})
    }

    fn issue_runtime_license_with_mutator<F>(
        runtime: &LicenseRuntime,
        seed: &[u8; 32],
        issued_at: i64,
        expires_at: i64,
        mutate: F,
    ) -> Vec<u8>
    where
        F: FnOnce(&mut LicensePayloadV5),
    {
        let provider = Ed25519CryptoProvider::new(
            Ed25519Keypair::from_seed_bytes("primary", seed).expect("issuer keypair"),
        );
        let observed = super::project_shared_fingerprint(&runtime.binding().fingerprint());
        let installation = licgen_core::InstallationIdentity {
            installation_id: Uuid::parse_str(&runtime.binding().installation_id())
                .expect("installation uuid"),
            installation_pubkey: Some(STANDARD.encode(runtime.binding().installation_pubkey())),
            device_fingerprint: observed.clone(),
            first_seen_at: Utc.timestamp_opt(issued_at, 0).single().expect("issued_at"),
            last_online_check_at: None,
        };
        let mut payload = LicensePayloadV5 {
            license_version: licgen_core::constants::LICENSE_VERSION,
            license_id: Uuid::new_v4(),
            installation,
            issued_at: Utc.timestamp_opt(issued_at, 0).single().expect("issued_at"),
            expires_at: Utc
                .timestamp_opt(expires_at, 0)
                .single()
                .expect("expires_at"),
            offline_policy: licgen_core::OfflinePolicy {
                max_offline_days: 30,
                ..Default::default()
            },
            security_policy: licgen_core::SecurityPolicy {
                key_id: Some("primary".into()),
                ..Default::default()
            },
            device_fingerprint_v2: observed,
            metadata: json!({
                "app_id": DEFAULT_APP_ID,
                "plan": "monthly",
                "customer_name_hint": "Runtime Test",
                "min_app_version": env!("CARGO_PKG_VERSION"),
                "features": ["core"],
                "policy_profile": "default"
            }),
        };
        mutate(&mut payload);
        let signature = provider.sign_license(&payload).expect("sign license");
        licgen_core::format::encode_signed_license(&payload, &signature).expect("encode license")
    }

    fn assert_runtime_rejects_unsupported_policy<F>(mutate: F, expected_code: &str)
    where
        F: FnOnce(&mut LicensePayloadV5),
    {
        let (_dir, runtime, seed) = runtime_with_fixed_key(VerificationEnvironment::Production);
        let now = 1_900_000_000;
        let license_bytes =
            issue_runtime_license_with_mutator(&runtime, &seed, now - 60, now + 3600, mutate);

        let err = runtime
            .evaluate_license_bytes(&license_bytes, now)
            .expect_err("unsupported policy must fail verification");
        assert_eq!(err.code, expected_code);

        let record = storage::StoredLicenseBlob {
            raw_bytes: license_bytes,
            installed_at: now,
            last_verified_at: now,
        };
        let active = runtime.apply_stored_license_for_test(&record, now);
        assert!(!active, "unsupported policy must never become active");
        let summary = runtime.summary();
        assert_eq!(summary.status, LicenseSummaryStatus::Invalid);
        assert_eq!(
            summary
                .last_error
                .expect("runtime should preserve rejection reason")
                .code,
            expected_code
        );
    }

    fn snapshot_path(_dir: &PathBuf, runtime: &LicenseRuntime, license_bytes: &[u8]) -> PathBuf {
        let (payload, _) =
            licgen_core::format::decode_signed_license(license_bytes).expect("decode license");
        let context = runtime
            .build_verification_context(SnapshotMode::Bootstrap)
            .expect("verification context");
        context.snapshot_file_path(&payload)
    }

    fn read_magic(path: &std::path::Path) -> [u8; 8] {
        let bytes = std::fs::read(path).expect("read snapshot file");
        bytes[..8].try_into().expect("magic bytes")
    }

    fn test_keypair(seed: u8) -> Keypair {
        let secret = SecretKey::from_bytes(&[seed; 32]).expect("secret key");
        let public: PublicKey = (&secret).into();
        Keypair { secret, public }
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
        let runtime = temp_runtime();
        let result = runtime.ensure_active();
        assert!(result.is_err());
    }

    #[test]
    fn e2e_probe_save_request_to_disk() {
        let runtime = temp_runtime();
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
        let runtime = temp_runtime();
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
        assert!(reparsed
            .request
            .installation
            .fingerprint
            .binding
            .stable
            .iter()
            .any(|component| component.kind == shared_core::ComponentKind::DiskSerial));
    }

    #[test]
    fn request_signature_matches_installation_key() {
        let runtime = temp_runtime();
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

    #[test]
    fn ensure_active_with_placeholder_cache_fails_closed() {
        let runtime = temp_runtime();
        let license = sample_license(runtime.binding().device_hash());
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let cache = LicenseCache {
            license,
            installed_at: now,
            last_verified_at: now,
            raw_bytes: Vec::new(),
        };
        runtime.update_cache(cache.clone());
        let err = runtime.ensure_active().unwrap_err();
        assert!(matches!(
            err.code.as_str(),
            "MissingCurrentLicenseFile" | "MissingCurrentLicenseIntegrity"
        ));
    }

    #[test]
    fn bootstrap_initial_creates_snapshot_and_returns_active() {
        let (dir, runtime, seed) = runtime_with_fixed_key(VerificationEnvironment::Production);
        let now = 1_900_000_000;
        let license_bytes = issue_runtime_license(&runtime, &seed, now - 60, now + 3600);

        let evaluation = runtime
            .evaluate_license_bytes(&license_bytes, now)
            .expect("bootstrap verification");

        assert_eq!(evaluation.status, validator::LicenseRuntimeStatus::Active);
        assert!(snapshot_path(&dir, &runtime, &license_bytes).exists());
    }

    #[test]
    fn bootstrap_rejects_unsupported_lease_policy() {
        assert_runtime_rejects_unsupported_policy(
            |payload| payload.offline_policy.lease_required = true,
            "LeaseUnsupported",
        );
    }

    #[test]
    fn bootstrap_rejects_unsupported_revocation_policy() {
        assert_runtime_rejects_unsupported_policy(
            |payload| payload.security_policy.revocation_epoch = Some(7),
            "RevocationUnsupported",
        );
    }

    #[test]
    fn bootstrap_rejects_unsupported_checkin_policy_fields() {
        assert_runtime_rejects_unsupported_policy(
            |payload| {
                payload.offline_policy.grace_days = 2;
                payload.offline_policy.last_online_check_at =
                    Utc.timestamp_opt(1_899_999_900, 0).single();
            },
            "HybridPolicyUnsupported",
        );
    }

    #[test]
    fn strict_after_bootstrap_keeps_license_active() {
        let (dir, runtime, seed) = runtime_with_fixed_key(VerificationEnvironment::Production);
        let bootstrap_at = 1_900_000_000;
        let license_bytes =
            issue_runtime_license(&runtime, &seed, bootstrap_at - 60, bootstrap_at + 3600);
        runtime
            .evaluate_license_bytes(&license_bytes, bootstrap_at)
            .expect("bootstrap verification");
        assert!(snapshot_path(&dir, &runtime, &license_bytes).exists());

        let record = storage::StoredLicenseBlob {
            raw_bytes: license_bytes.clone(),
            installed_at: bootstrap_at,
            last_verified_at: bootstrap_at,
        };
        let strict_now = bootstrap_at + 600;
        let active = runtime.apply_stored_license_for_test(&record, strict_now);
        assert!(active, "strict verification must keep the license active");
        assert_eq!(runtime.summary().status, LicenseSummaryStatus::Active);
        assert_eq!(runtime.summary().last_verified_at, Some(strict_now));
    }

    #[test]
    fn tampered_current_license_is_invalid_before_strict_verify() {
        let (_dir, runtime, seed) = runtime_with_fixed_key(VerificationEnvironment::Production);
        let bootstrap_at = 1_900_000_000;
        let license_bytes =
            issue_runtime_license(&runtime, &seed, bootstrap_at - 60, bootstrap_at + 3600);
        let bootstrap_eval = runtime
            .evaluate_license_bytes(&license_bytes, bootstrap_at)
            .expect("bootstrap verification");
        runtime.update_cache(LicenseCache {
            license: bootstrap_eval.license,
            installed_at: bootstrap_at,
            last_verified_at: bootstrap_at,
            raw_bytes: license_bytes.clone(),
        });
        runtime.seed_local_license_material_for_test(&license_bytes);

        let current_path = storage::current_license_path_from_root(&runtime.inner.verification_state_root);
        crate::license::write_atomic_secure(&current_path, b"tampered-license")
            .expect("tamper current license");

        let err = runtime
            .ensure_active_at(bootstrap_at + 600)
            .expect_err("tampered current.lic must fail closed");
        assert_eq!(err.code, "LocalLicenseTampered");
        assert_eq!(runtime.summary().status, LicenseSummaryStatus::Invalid);
    }

    #[test]
    fn replaced_current_license_with_other_valid_blob_is_detected() {
        let (_dir, runtime, seed) = runtime_with_fixed_key(VerificationEnvironment::Production);
        let bootstrap_at = 1_900_000_000;
        let original =
            issue_runtime_license(&runtime, &seed, bootstrap_at - 60, bootstrap_at + 3600);
        let replacement = issue_runtime_license_with_mutator(
            &runtime,
            &seed,
            bootstrap_at - 60,
            bootstrap_at + 7200,
            |payload| {
                payload.metadata["customer_name_hint"] = json!("Replacement");
            },
        );
        let bootstrap_eval = runtime
            .evaluate_license_bytes(&original, bootstrap_at)
            .expect("bootstrap verification");
        runtime.update_cache(LicenseCache {
            license: bootstrap_eval.license,
            installed_at: bootstrap_at,
            last_verified_at: bootstrap_at,
            raw_bytes: original.clone(),
        });
        runtime.seed_local_license_material_for_test(&original);
        let current_path = storage::current_license_path_from_root(&runtime.inner.verification_state_root);
        crate::license::write_atomic_secure(&current_path, &replacement)
            .expect("replace current license");
        let secret = runtime.local_license_integrity_secret();
        storage::persist_current_license_integrity_under_root(
            &runtime.inner.verification_state_root,
            &replacement,
            &secret,
        )
        .expect("refresh integrity metadata for replaced license");

        let err = runtime
            .ensure_active_at(bootstrap_at + 600)
            .expect_err("sqlite/current mismatch must fail closed");
        assert_eq!(err.code, "LocalLicenseStateMismatch");
        assert_eq!(runtime.summary().status, LicenseSummaryStatus::Invalid);
    }

    #[test]
    fn truncated_current_license_is_detected() {
        let (_dir, runtime, seed) = runtime_with_fixed_key(VerificationEnvironment::Production);
        let bootstrap_at = 1_900_000_000;
        let license_bytes =
            issue_runtime_license(&runtime, &seed, bootstrap_at - 60, bootstrap_at + 3600);
        let bootstrap_eval = runtime
            .evaluate_license_bytes(&license_bytes, bootstrap_at)
            .expect("bootstrap verification");
        runtime.update_cache(LicenseCache {
            license: bootstrap_eval.license,
            installed_at: bootstrap_at,
            last_verified_at: bootstrap_at,
            raw_bytes: license_bytes.clone(),
        });
        runtime.seed_local_license_material_for_test(&license_bytes);
        let current_path = storage::current_license_path_from_root(&runtime.inner.verification_state_root);
        crate::license::write_atomic_secure(&current_path, b"LIC")
            .expect("truncate current license");

        let err = runtime
            .ensure_active_at(bootstrap_at + 600)
            .expect_err("truncated current.lic must fail closed");
        assert_eq!(err.code, "LocalLicenseTampered");
        assert_eq!(runtime.summary().status, LicenseSummaryStatus::Invalid);
    }

    #[test]
    fn parallel_strict_verifications_do_not_corrupt_snapshot_files() {
        let (_dir, runtime, seed) = runtime_with_fixed_key(VerificationEnvironment::Production);
        let bootstrap_at = 1_900_000_000;
        let license_bytes =
            issue_runtime_license(&runtime, &seed, bootstrap_at - 60, bootstrap_at + 3600);
        let bootstrap_eval = runtime
            .evaluate_license_bytes(&license_bytes, bootstrap_at)
            .expect("bootstrap verification");
        runtime.update_cache(LicenseCache {
            license: bootstrap_eval.license,
            installed_at: bootstrap_at,
            last_verified_at: bootstrap_at,
            raw_bytes: license_bytes.clone(),
        });

        let runtime = Arc::new(runtime);
        let barrier = Arc::new(Barrier::new(9));
        let mut workers = Vec::new();
        for _ in 0..8 {
            let runtime = Arc::clone(&runtime);
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                barrier.wait();
                runtime
                    .ensure_active_at_for_test(bootstrap_at + 600)
                    .expect("parallel strict ensure");
            }));
        }

        barrier.wait();
        for worker in workers {
            worker.join().unwrap();
        }

        let snapshot = snapshot_path(&PathBuf::new(), &runtime, &license_bytes);
        let watermark = snapshot.with_extension("hwm");
        assert_eq!(&read_magic(&snapshot), b"LICSNAP\0");
        assert_eq!(&read_magic(&watermark), b"LICSHWM\0");
        assert_eq!(runtime.summary().status, LicenseSummaryStatus::Active);
    }

    #[test]
    fn ensure_active_reuses_observed_binding_within_ttl() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observer_state = Arc::new(StdRwLock::new(ObservedHardware {
            platform: shared_core::Platform::Macos,
            machine_id: Some("machine-cache".into()),
            disk_serial: Some("disk-cache".into()),
            cpu_model: Some("Apple M1".into()),
            hostname: Some("service-host".into()),
            locale: Some("en_US.UTF-8".into()),
            timezone: "-0600".into(),
        }));
        let observer: Arc<dyn HardwareObserver + Send + Sync> = Arc::new(MutableCountingObserver {
            observed: Arc::clone(&observer_state),
            calls: Arc::clone(&calls),
        });
        let (_dir, runtime, seed) = runtime_with_fixed_key_and_observer_ttl(
            VerificationEnvironment::Production,
            observer,
            Duration::from_secs(10),
        );
        let bootstrap_at = 1_900_000_000;
        let license_bytes =
            issue_runtime_license(&runtime, &seed, bootstrap_at - 60, bootstrap_at + 3600);
        let bootstrap_eval = runtime
            .evaluate_license_bytes(&license_bytes, bootstrap_at)
            .expect("bootstrap verification");
        runtime.update_cache(LicenseCache {
            license: bootstrap_eval.license,
            installed_at: bootstrap_at,
            last_verified_at: bootstrap_at,
            raw_bytes: license_bytes,
        });

        let baseline_calls = calls.load(Ordering::SeqCst);
        assert_eq!(
            baseline_calls, 2,
            "bootstrap path should collect once for installation bootstrap and once for the initial refresh",
        );
        runtime
            .ensure_active_at_for_test(bootstrap_at + 60)
            .expect("first strict verify");
        runtime
            .ensure_active_at_for_test(bootstrap_at + 61)
            .expect("second strict verify");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            baseline_calls,
            "two strict verifies inside the TTL should reuse the observed binding cache",
        );
    }

    #[test]
    fn ensure_active_refreshes_observed_binding_after_ttl() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observer: Arc<dyn HardwareObserver + Send + Sync> = Arc::new(MutableCountingObserver {
            observed: Arc::new(StdRwLock::new(ObservedHardware {
                platform: shared_core::Platform::Macos,
                machine_id: Some("machine-ttl".into()),
                disk_serial: Some("disk-ttl".into()),
                cpu_model: Some("Apple M1".into()),
                hostname: Some("service-host".into()),
                locale: Some("en_US.UTF-8".into()),
                timezone: "-0600".into(),
            })),
            calls: Arc::clone(&calls),
        });
        let (_dir, runtime, seed) = runtime_with_fixed_key_and_observer_ttl(
            VerificationEnvironment::Production,
            observer,
            Duration::from_millis(25),
        );
        let bootstrap_at = 1_900_000_000;
        let license_bytes =
            issue_runtime_license(&runtime, &seed, bootstrap_at - 60, bootstrap_at + 3600);
        let bootstrap_eval = runtime
            .evaluate_license_bytes(&license_bytes, bootstrap_at)
            .expect("bootstrap verification");
        runtime.update_cache(LicenseCache {
            license: bootstrap_eval.license,
            installed_at: bootstrap_at,
            last_verified_at: bootstrap_at,
            raw_bytes: license_bytes,
        });

        let baseline_calls = calls.load(Ordering::SeqCst);
        assert_eq!(
            baseline_calls, 2,
            "bootstrap path should collect once for installation bootstrap and once for the initial refresh",
        );
        std::thread::sleep(Duration::from_millis(40));
        runtime
            .ensure_active_at_for_test(bootstrap_at + 60)
            .expect("strict verify after ttl");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            baseline_calls + 1,
            "strict verify after the TTL should recollect hardware",
        );
    }

    #[test]
    fn fingerprint_mismatch_invalidates_observed_binding_cache() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observer_state = Arc::new(StdRwLock::new(ObservedHardware {
            platform: shared_core::Platform::Macos,
            machine_id: Some("machine-stable".into()),
            disk_serial: Some("disk-stable".into()),
            cpu_model: Some("Apple M1".into()),
            hostname: Some("service-host".into()),
            locale: Some("en_US.UTF-8".into()),
            timezone: "-0600".into(),
        }));
        let observer: Arc<dyn HardwareObserver + Send + Sync> = Arc::new(MutableCountingObserver {
            observed: Arc::clone(&observer_state),
            calls: Arc::clone(&calls),
        });
        let (_dir, runtime, seed) = runtime_with_fixed_key_and_observer_ttl(
            VerificationEnvironment::Production,
            observer,
            Duration::from_millis(25),
        );
        let bootstrap_at = 1_900_000_000;
        let license_bytes =
            issue_runtime_license(&runtime, &seed, bootstrap_at - 60, bootstrap_at + 3600);
        let record = storage::StoredLicenseBlob {
            raw_bytes: license_bytes.clone(),
            installed_at: bootstrap_at,
            last_verified_at: bootstrap_at,
        };
        let bootstrap_eval = runtime
            .evaluate_license_bytes(&license_bytes, bootstrap_at)
            .expect("bootstrap verification");
        runtime.update_cache(LicenseCache {
            license: bootstrap_eval.license,
            installed_at: bootstrap_at,
            last_verified_at: bootstrap_at,
            raw_bytes: license_bytes,
        });

        let baseline_calls = calls.load(Ordering::SeqCst);
        assert_eq!(
            baseline_calls, 2,
            "bootstrap path should collect once for installation bootstrap and once for the initial refresh",
        );

        *observer_state.write().expect("observer state poisoned") = ObservedHardware {
            platform: shared_core::Platform::Macos,
            machine_id: Some("machine-mismatch".into()),
            disk_serial: Some("disk-mismatch".into()),
            cpu_model: Some("Apple M1".into()),
            hostname: Some("service-host".into()),
            locale: Some("en_US.UTF-8".into()),
            timezone: "-0600".into(),
        };
        std::thread::sleep(Duration::from_millis(40));
        let err = runtime
            .ensure_active_at_for_test(bootstrap_at + 60)
            .expect_err("hardware mismatch must fail");
        assert_eq!(err.code, "DeviceMismatch");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            baseline_calls + 1,
            "mismatch should recollect once",
        );

        *observer_state.write().expect("observer state poisoned") = ObservedHardware {
            platform: shared_core::Platform::Macos,
            machine_id: Some("machine-stable".into()),
            disk_serial: Some("disk-stable".into()),
            cpu_model: Some("Apple M1".into()),
            hostname: Some("service-host".into()),
            locale: Some("en_US.UTF-8".into()),
            timezone: "-0600".into(),
        };
        let active = runtime.apply_stored_license_for_test(&record, bootstrap_at + 61);
        assert!(
            active,
            "cache invalidation should force a fresh observation for the stored license blob"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            baseline_calls + 2,
            "after a mismatch the next verify must recollect instead of reusing stale cached hardware",
        );
    }

    #[test]
    fn strict_without_snapshot_fails_without_weak_fallback() {
        let (_dir, runtime, seed) = runtime_with_fixed_key(VerificationEnvironment::Production);
        let now = 1_900_000_000;
        let license_bytes = issue_runtime_license(&runtime, &seed, now - 60, now + 3600);
        let record = storage::StoredLicenseBlob {
            raw_bytes: license_bytes,
            installed_at: now,
            last_verified_at: now,
        };

        let active = runtime.apply_stored_license_for_test(&record, now);
        assert!(!active);
        let summary = runtime.summary();
        assert_eq!(summary.status, LicenseSummaryStatus::Invalid);
        assert_eq!(
            summary.last_error.as_ref().map(|err| err.code.as_str()),
            Some("SnapshotMissing")
        );
    }

    #[test]
    fn corrupt_snapshot_fails_strict_verification() {
        let (dir, runtime, seed) = runtime_with_fixed_key(VerificationEnvironment::Production);
        let now = 1_900_000_000;
        let license_bytes = issue_runtime_license(&runtime, &seed, now - 60, now + 3600);
        runtime
            .evaluate_license_bytes(&license_bytes, now)
            .expect("bootstrap verification");
        let path = snapshot_path(&dir, &runtime, &license_bytes);
        std::fs::write(&path, b"tampered").expect("tamper snapshot");

        let record = storage::StoredLicenseBlob {
            raw_bytes: license_bytes,
            installed_at: now,
            last_verified_at: now,
        };
        let active = runtime.apply_stored_license_for_test(&record, now + 600);
        assert!(!active);
        let summary = runtime.summary();
        assert_eq!(summary.status, LicenseSummaryStatus::Invalid);
        assert_eq!(
            summary.last_error.as_ref().map(|err| err.code.as_str()),
            Some("SnapshotCorrupted")
        );
    }

    #[test]
    fn deleted_snapshot_after_bootstrap_fails_closed_without_fallback() {
        let (dir, runtime, seed) = runtime_with_fixed_key(VerificationEnvironment::Production);
        let now = 1_900_000_000;
        let license_bytes = issue_runtime_license(&runtime, &seed, now - 60, now + 3600);
        runtime
            .evaluate_license_bytes(&license_bytes, now)
            .expect("bootstrap verification");
        let path = snapshot_path(&dir, &runtime, &license_bytes);
        std::fs::remove_file(&path).expect("delete snapshot");

        let record = storage::StoredLicenseBlob {
            raw_bytes: license_bytes,
            installed_at: now,
            last_verified_at: now,
        };
        let active = runtime.apply_stored_license_for_test(&record, now + 600);
        assert!(!active);
        let summary = runtime.summary();
        assert_eq!(summary.status, LicenseSummaryStatus::Invalid);
        assert_eq!(
            summary.last_error.as_ref().map(|err| err.code.as_str()),
            Some("SnapshotCorrupted")
        );
    }

    #[test]
    fn strict_detects_clock_rollback() {
        let (_dir, runtime, seed) = runtime_with_fixed_key(VerificationEnvironment::Production);
        let bootstrap_at = 1_900_000_000;
        let license_bytes =
            issue_runtime_license(&runtime, &seed, bootstrap_at - 60, bootstrap_at + 7200);
        runtime
            .evaluate_license_bytes(&license_bytes, bootstrap_at)
            .expect("bootstrap verification");

        let cache = LicenseCache {
            license: runtime
                .evaluate_license_bytes(&license_bytes, bootstrap_at)
                .expect("bootstrap eval")
                .license,
            installed_at: bootstrap_at,
            last_verified_at: bootstrap_at,
            raw_bytes: license_bytes.clone(),
        };
        runtime.update_cache(cache);
        runtime
            .ensure_active_at_for_test(bootstrap_at + 3600)
            .expect("strict verification later");
        let err = runtime
            .ensure_active_at_for_test(bootstrap_at + 3000)
            .expect_err("rollback must fail");
        assert_eq!(err.code, "ClockRollback");
    }

    #[test]
    fn expired_license_does_not_revive_after_clock_rollback() {
        let (_dir, runtime, seed) = runtime_with_fixed_key(VerificationEnvironment::Production);
        let bootstrap_at = 1_900_000_000;
        let expires_at = bootstrap_at + 120;
        let license_bytes = issue_runtime_license(&runtime, &seed, bootstrap_at - 60, expires_at);
        let bootstrap_eval = runtime
            .evaluate_license_bytes(&license_bytes, bootstrap_at)
            .expect("bootstrap verification");
        runtime.update_cache(LicenseCache {
            license: bootstrap_eval.license,
            installed_at: bootstrap_at,
            last_verified_at: bootstrap_at,
            raw_bytes: license_bytes.clone(),
        });

        let expired_err = runtime
            .ensure_active_at_for_test(expires_at + 600)
            .expect_err("expired license must stop being active");
        assert_eq!(expired_err.code, "Expired");

        let rollback_err = runtime
            .ensure_active_at_for_test(expires_at - 30)
            .expect_err("rolled back clock must not revive expired license");
        assert!(matches!(
            rollback_err.code.as_str(),
            "ClockRollback" | "Expired"
        ));
    }

    #[test]
    fn generates_request_after_schema_v3_migration() {
        let dir =
            std::env::temp_dir().join(format!("license-runtime-migrate-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let observer = test_observer();
        let installation_id = "6600700f-e758-47a6-b7d7-6c9a81e61e60";
        let keypair = test_keypair(0x44);
        let fingerprint = collect_fingerprint_with_observer(installation_id, observer.as_ref())
            .expect("fingerprint");
        let hardware_hash =
            fingerprint_hardware_hash_bytes(&fingerprint).expect("fingerprint hash");
        let installation_path = dir.join("installation.json");
        std::fs::write(
            &installation_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema": 3,
                "installation_id": installation_id,
                "keypair_b64": STANDARD.encode(keypair.to_bytes()),
                "fingerprint": fingerprint,
                "hardware_hash_hex": hex::encode(hardware_hash),
                "created_at": 1_700_000_000i64,
                "migrated_from_legacy": false,
                "legacy_device_hash_hex": null
            }))
            .unwrap(),
        )
        .unwrap();

        let binding = DeviceBindingStore::load_or_init_from_dir_with_observer(&dir, observer)
            .expect("migrate");
        let runtime = LicenseRuntime::new(
            binding,
            super::super::default_keyring(),
            LicenseState::default(),
            dir.clone(),
            VerificationEnvironment::Development,
        );

        let (request, bytes) = runtime
            .generate_request_bytes("monthly", Some("Cliente Legacy".into()))
            .expect("request after migration");

        let installation_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&installation_path).unwrap()).unwrap();
        println!("EVIDENCE_MIGRATION_DIR={}", dir.display());
        println!(
            "EVIDENCE_MIGRATION_INSTALLATION_JSON={}",
            installation_path.display()
        );
        println!(
            "EVIDENCE_MIGRATION_INSTALLATION_KEY={}",
            dir.join("installation.key").display()
        );
        println!(
            "EVIDENCE_MIGRATION_INSTALLATION_PUBKEY={}",
            STANDARD.encode(keypair.public.to_bytes())
        );
        assert!(dir.join("installation.key").exists());
        assert!(installation_json.get("keypair_b64").is_none());
        assert_eq!(request.installation.installation_id, installation_id);

        let reparsed = shared_core::verify_request(&bytes).expect("parse bytes");
        assert_eq!(
            reparsed.request.installation.installation_pubkey,
            STANDARD.encode(keypair.public.to_bytes())
        );

        let signing = shared_core::build_signing_bytes(&request).expect("signing payload bytes");
        let sig_buf: [u8; 64] = STANDARD
            .decode(&request.request_signature.value)
            .expect("signature b64")
            .try_into()
            .expect("signature length");
        let signature = Signature::from_bytes(&sig_buf).unwrap();
        keypair
            .public
            .verify_strict(&signing, &signature)
            .expect("signature valid after migration");
    }
}
