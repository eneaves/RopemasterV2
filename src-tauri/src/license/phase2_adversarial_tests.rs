use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{TimeZone, Utc};
use ed25519_dalek::PublicKey;
use licgen_core::crypto::{Ed25519CryptoProvider, Ed25519Keypair, LicenseCryptoProvider};
use licgen_core::{
    ClockGuardMode, ComponentSource, DeviceFingerprintV2, FingerprintBindingBundle,
    FingerprintComponent, FingerprintComponentKind, FingerprintObservation, LicensePayloadV5,
    SecurityProfile, SnapshotMode, VerificationContext, VerificationEnvironment,
};
use serde_json::json;
use uuid::Uuid;

use super::runtime::{
    device_binding::DeviceBindingStore,
    fingerprint::{HardwareObserver, ObservedHardware},
    keyring::{KeyStatus, LicenseKeyring, MultiKeyring},
    LicenseRuntime,
};
use super::{storage, LicenseCache, LicenseState};

#[derive(Clone)]
struct FixedObserver(ObservedHardware);

impl HardwareObserver for FixedObserver {
    fn observe(&self) -> super::CmdResult<ObservedHardware> {
        Ok(self.0.clone())
    }
}

struct RuntimeHarness {
    root: PathBuf,
    env: VerificationEnvironment,
    runtime: LicenseRuntime,
}

impl RuntimeHarness {
    fn current_license_path(&self) -> PathBuf {
        storage::current_license_path_from_root(&self.root)
    }

    fn current_integrity_path(&self) -> PathBuf {
        storage::current_license_integrity_path_from_root(&self.root)
    }

    fn snapshot_context(&self) -> VerificationContext {
        let binding = self.runtime.binding();
        let snapshot_secret = binding
            .key_store()
            .derive_secret(b"license-snapshot", binding.installation_id().as_bytes());
        VerificationContext::new(
            SecurityProfile::for_environment(self.env),
            self.root.clone(),
            snapshot_secret,
            SnapshotMode::Bootstrap,
            ClockGuardMode::Enforced,
        )
        .expect("verification context")
    }

    fn snapshot_path_for(&self, license_bytes: &[u8]) -> PathBuf {
        let (payload, _) = licgen_core::format::decode_signed_license(license_bytes)
            .expect("decode signed license");
        self.snapshot_context().snapshot_file_path(&payload)
    }
}

fn observer(machine_id: &str, disk_serial: &str) -> Arc<dyn HardwareObserver + Send + Sync> {
    Arc::new(FixedObserver(ObservedHardware {
        platform: shared_core::Platform::Macos,
        machine_id: Some(machine_id.into()),
        disk_serial: Some(disk_serial.into()),
        cpu_model: Some("Apple M3".into()),
        hostname: Some("adversarial-host".into()),
        locale: Some("en_US.UTF-8".into()),
        timezone: "-0600".into(),
    }))
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "phase2-adversarial-{label}-{}",
        Uuid::new_v4()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn runtime_with_keyring(
    label: &str,
    env: VerificationEnvironment,
    keyring: Arc<dyn LicenseKeyring + Send + Sync>,
    observer: Arc<dyn HardwareObserver + Send + Sync>,
) -> RuntimeHarness {
    let root = temp_dir(label);
    let binding = DeviceBindingStore::load_or_init_from_dir_with_observer(&root, observer)
        .expect("init binding");
    let runtime = LicenseRuntime::new(binding, keyring, LicenseState::default(), root.clone(), env);
    RuntimeHarness { root, env, runtime }
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

fn issue_runtime_license(
    runtime: &LicenseRuntime,
    seed_byte: u8,
    key_id: &str,
    key_version: Option<&str>,
    issued_at: i64,
    expires_at: i64,
) -> Vec<u8> {
    issue_runtime_license_with_mutator(
        runtime,
        seed_byte,
        key_id,
        key_version,
        issued_at,
        expires_at,
        |_| {},
    )
}

fn issue_runtime_license_with_mutator<F>(
    runtime: &LicenseRuntime,
    seed_byte: u8,
    key_id: &str,
    key_version: Option<&str>,
    issued_at: i64,
    expires_at: i64,
    mutate: F,
) -> Vec<u8>
where
    F: FnOnce(&mut LicensePayloadV5),
{
    let seed = [seed_byte; 32];
    let provider = Ed25519CryptoProvider::new(
        Ed25519Keypair::from_seed_bytes(key_id, &seed).expect("issuer keypair"),
    );
    let observed = project_shared_fingerprint(&runtime.binding().fingerprint());
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
        expires_at: Utc.timestamp_opt(expires_at, 0).single().expect("expires_at"),
        offline_policy: licgen_core::OfflinePolicy {
            max_offline_days: 30,
            ..Default::default()
        },
        security_policy: licgen_core::SecurityPolicy {
            key_id: Some(key_id.to_string()),
            key_version: key_version.map(str::to_string),
            ..Default::default()
        },
        device_fingerprint_v2: observed,
        metadata: json!({
            "app_id": crate::license::validator::DEFAULT_APP_ID,
            "plan": "monthly",
            "customer_name_hint": "Adversarial Runtime",
            "min_app_version": env!("CARGO_PKG_VERSION"),
            "features": ["core"],
            "policy_profile": "default"
        }),
    };
    mutate(&mut payload);
    let signature = provider.sign_license(&payload).expect("sign license");
    licgen_core::format::encode_signed_license(&payload, &signature).expect("encode license")
}

fn cache_bootstrapped_license(runtime: &LicenseRuntime, license_bytes: &[u8], bootstrap_at: i64) {
    let evaluation = runtime
        .evaluate_license_bytes(license_bytes, bootstrap_at)
        .expect("bootstrap verification");
    runtime.update_cache(LicenseCache {
        license: evaluation.license,
        installed_at: bootstrap_at,
        last_verified_at: bootstrap_at,
        raw_bytes: license_bytes.to_vec(),
    });
}

fn versioned_keyring(seed_byte: u8, key_version: Option<&str>, status: KeyStatus) -> Arc<dyn LicenseKeyring + Send + Sync> {
    let public_key = PublicKey::from_bytes(
        &Ed25519Keypair::from_seed_bytes("primary", &[seed_byte; 32])
            .expect("keypair")
            .verifying_key_bytes(),
    )
    .expect("public key");
    Arc::new(
        MultiKeyring::new()
            .with_key_status(
                "primary",
                public_key,
                key_version.map(str::to_string),
                status,
            )
            .with_active_key_versioned("primary", key_version.map(str::to_string)),
    )
}

#[test]
fn adversarial_snapshot_replay_restoring_old_snapshot_fails_closed() {
    let harness = runtime_with_keyring(
        "snapshot-replay",
        VerificationEnvironment::Production,
        versioned_keyring(0x44, Some("2026-10"), KeyStatus::Active),
        observer("machine-replay", "disk-replay"),
    );
    let bootstrap_at = 1_900_000_000;
    let license_bytes = issue_runtime_license(
        &harness.runtime,
        0x44,
        "primary",
        Some("2026-10"),
        bootstrap_at - 60,
        bootstrap_at + 7200,
    );

    cache_bootstrapped_license(&harness.runtime, &license_bytes, bootstrap_at);
    harness
        .runtime
        .ensure_active_at_for_test(bootstrap_at + 600)
        .expect("strict verification should seed durable state");
    let snapshot_path = harness.snapshot_path_for(&license_bytes);
    let original_snapshot = fs::read(&snapshot_path).expect("snapshot bytes");

    harness
        .runtime
        .ensure_active_at_for_test(bootstrap_at + 1200)
        .expect("second strict verification should advance watermark");

    fs::write(&snapshot_path, original_snapshot).expect("restore stale snapshot");

    let err = harness
        .runtime
        .ensure_active_at_for_test(bootstrap_at + 1800)
        .expect_err("snapshot replay must fail closed");
    assert_eq!(err.code, "SnapshotReplay");
}

#[test]
fn adversarial_snapshot_watermark_tampering_fails_closed() {
    let harness = runtime_with_keyring(
        "watermark-tamper",
        VerificationEnvironment::Production,
        versioned_keyring(0x44, Some("2026-10"), KeyStatus::Active),
        observer("machine-hwm", "disk-hwm"),
    );
    let bootstrap_at = 1_900_000_000;
    let license_bytes = issue_runtime_license(
        &harness.runtime,
        0x44,
        "primary",
        Some("2026-10"),
        bootstrap_at - 60,
        bootstrap_at + 7200,
    );

    cache_bootstrapped_license(&harness.runtime, &license_bytes, bootstrap_at);
    harness
        .runtime
        .ensure_active_at_for_test(bootstrap_at + 600)
        .expect("strict verification");

    let watermark_path = harness.snapshot_path_for(&license_bytes).with_extension("hwm");
    let mut bytes = fs::read(&watermark_path).expect("watermark bytes");
    *bytes.last_mut().expect("non-empty watermark") ^= 0xFF;
    fs::write(&watermark_path, &bytes).expect("tamper watermark");

    let err = harness
        .runtime
        .ensure_active_at_for_test(bootstrap_at + 900)
        .expect_err("tampered watermark must fail closed");
    assert_eq!(err.code, "SnapshotCorrupted");
}

#[test]
fn adversarial_snapshot_watermark_deletion_fails_closed() {
    let harness = runtime_with_keyring(
        "watermark-delete",
        VerificationEnvironment::Production,
        versioned_keyring(0x44, Some("2026-10"), KeyStatus::Active),
        observer("machine-hwm-delete", "disk-hwm-delete"),
    );
    let bootstrap_at = 1_900_000_000;
    let license_bytes = issue_runtime_license(
        &harness.runtime,
        0x44,
        "primary",
        Some("2026-10"),
        bootstrap_at - 60,
        bootstrap_at + 7200,
    );

    cache_bootstrapped_license(&harness.runtime, &license_bytes, bootstrap_at);
    harness
        .runtime
        .ensure_active_at_for_test(bootstrap_at + 600)
        .expect("strict verification");

    let watermark_path = harness.snapshot_path_for(&license_bytes).with_extension("hwm");
    fs::remove_file(&watermark_path).expect("delete watermark");

    let err = harness
        .runtime
        .ensure_active_at_for_test(bootstrap_at + 900)
        .expect_err("missing watermark must fail closed");
    assert_eq!(err.code, "SnapshotCorrupted");
}

#[test]
fn adversarial_integrity_metadata_tampering_invalidates_runtime() {
    let harness = runtime_with_keyring(
        "integrity-tamper",
        VerificationEnvironment::Production,
        versioned_keyring(0x44, Some("2026-10"), KeyStatus::Active),
        observer("machine-integrity", "disk-integrity"),
    );
    let bootstrap_at = 1_900_000_000;
    let license_bytes = issue_runtime_license(
        &harness.runtime,
        0x44,
        "primary",
        Some("2026-10"),
        bootstrap_at - 60,
        bootstrap_at + 7200,
    );

    cache_bootstrapped_license(&harness.runtime, &license_bytes, bootstrap_at);
    harness
        .runtime
        .ensure_active_at_for_test(bootstrap_at + 600)
        .expect("strict verification");

    fs::write(
        harness.current_integrity_path(),
        br#"{"version":1,"sha256":"bad","size_bytes":999,"tag_sha256":"bad"}"#,
    )
    .expect("tamper integrity metadata");

    let err = harness
        .runtime
        .ensure_active()
        .expect_err("tampered integrity metadata must fail closed");
    assert_eq!(err.code, "LocalLicenseTampered");
}

#[test]
fn adversarial_missing_integrity_metadata_fails_closed() {
    let harness = runtime_with_keyring(
        "integrity-delete",
        VerificationEnvironment::Production,
        versioned_keyring(0x44, Some("2026-10"), KeyStatus::Active),
        observer("machine-integrity-delete", "disk-integrity-delete"),
    );
    let bootstrap_at = 1_900_000_000;
    let license_bytes = issue_runtime_license(
        &harness.runtime,
        0x44,
        "primary",
        Some("2026-10"),
        bootstrap_at - 60,
        bootstrap_at + 7200,
    );

    cache_bootstrapped_license(&harness.runtime, &license_bytes, bootstrap_at);
    harness
        .runtime
        .ensure_active_at_for_test(bootstrap_at + 600)
        .expect("strict verification");
    fs::remove_file(harness.current_integrity_path()).expect("delete integrity metadata");

    let err = harness
        .runtime
        .ensure_active()
        .expect_err("missing integrity metadata must fail closed");
    assert_eq!(err.code, "MissingCurrentLicenseIntegrity");
}

#[test]
fn adversarial_replayed_old_integrity_metadata_is_detected() {
    let harness = runtime_with_keyring(
        "integrity-replay",
        VerificationEnvironment::Production,
        versioned_keyring(0x44, Some("2026-10"), KeyStatus::Active),
        observer("machine-integrity-replay", "disk-integrity-replay"),
    );
    let bootstrap_at = 1_900_000_000;
    let original = issue_runtime_license(
        &harness.runtime,
        0x44,
        "primary",
        Some("2026-10"),
        bootstrap_at - 60,
        bootstrap_at + 7200,
    );
    let replacement = issue_runtime_license_with_mutator(
        &harness.runtime,
        0x44,
        "primary",
        Some("2026-10"),
        bootstrap_at - 60,
        bootstrap_at + 9600,
        |payload| {
            payload.metadata["customer_name_hint"] = json!("Replacement");
        },
    );

    cache_bootstrapped_license(&harness.runtime, &original, bootstrap_at);
    harness
        .runtime
        .ensure_active_at_for_test(bootstrap_at + 600)
        .expect("strict verification");
    let original_integrity = fs::read(harness.current_integrity_path()).expect("integrity bytes");

    crate::license::write_atomic_secure(&harness.current_license_path(), &replacement)
        .expect("replace current.lic");
    fs::write(harness.current_integrity_path(), original_integrity)
        .expect("replay old integrity metadata");

    let err = harness
        .runtime
        .ensure_active()
        .expect_err("replayed integrity metadata must fail closed");
    assert_eq!(err.code, "LocalLicenseTampered");
}

#[test]
fn adversarial_cross_environment_signature_mismatch_is_rejected() {
    let harness = runtime_with_keyring(
        "cross-env",
        VerificationEnvironment::Production,
        versioned_keyring(0x44, Some("2026-10"), KeyStatus::Active),
        observer("machine-cross-env", "disk-cross-env"),
    );
    let now = 1_900_000_000;
    let staging_signed = issue_runtime_license(
        &harness.runtime,
        0x45,
        "primary",
        Some("2026-10"),
        now - 60,
        now + 3600,
    );

    let err = harness
        .runtime
        .evaluate_license_bytes(&staging_signed, now)
        .expect_err("license signed with another trust anchor must fail");
    assert_eq!(err.code, "SignatureFailed");
}

#[test]
fn adversarial_downgrade_missing_required_key_version_is_rejected() {
    let harness = runtime_with_keyring(
        "downgrade-missing-key-version",
        VerificationEnvironment::Production,
        versioned_keyring(0x44, Some("2026-10"), KeyStatus::Active),
        observer("machine-downgrade", "disk-downgrade"),
    );
    let now = 1_900_000_000;
    let downgraded = issue_runtime_license(
        &harness.runtime,
        0x44,
        "primary",
        None,
        now - 60,
        now + 3600,
    );

    let err = harness
        .runtime
        .evaluate_license_bytes(&downgraded, now)
        .expect_err("license without required key_version must fail closed");
    assert_eq!(err.code, "KeyVersionMismatch");
}

#[test]
fn adversarial_retired_key_is_rejected_even_if_signature_is_valid() {
    let harness = runtime_with_keyring(
        "retired-key",
        VerificationEnvironment::Production,
        versioned_keyring(0x44, Some("2026-01"), KeyStatus::Retired),
        observer("machine-retired", "disk-retired"),
    );
    let now = 1_900_000_000;
    let retired = issue_runtime_license(
        &harness.runtime,
        0x44,
        "primary",
        Some("2026-01"),
        now - 60,
        now + 3600,
    );

    let err = harness
        .runtime
        .evaluate_license_bytes(&retired, now)
        .expect_err("retired key must be rejected even with valid signature");
    assert_eq!(err.code, "RetiredKey");
}

#[test]
fn adversarial_unknown_key_id_is_rejected_by_runtime() {
    let harness = runtime_with_keyring(
        "unknown-key-id",
        VerificationEnvironment::Production,
        versioned_keyring(0x44, Some("2026-10"), KeyStatus::Active),
        observer("machine-unknown-key", "disk-unknown-key"),
    );
    let now = 1_900_000_000;
    let unknown = issue_runtime_license(
        &harness.runtime,
        0x44,
        "rotated",
        Some("2026-10"),
        now - 60,
        now + 3600,
    );

    let err = harness
        .runtime
        .evaluate_license_bytes(&unknown, now)
        .expect_err("unknown key_id must fail closed");
    assert_eq!(err.code, "UnknownKeyId");
}
