use std::fs;
use std::sync::Arc;

use ed25519_dalek::PublicKey;
use licgen_core::audit::issuance::{IssuanceAuditRecord, IssuanceAuditSink};
use licgen_core::crypto::{
    Ed25519CryptoProvider, Ed25519Keypair, InMemorySigner, LicenseSigner, ED25519_SIGNATURE_ALG,
};
use licgen_core::{AuditTrail, KeyMetadataSnapshot, NullLegacyAdapter};
use licgen_workflows::{
    issue_license, load_request, verify_request, IssueLicenseInput, LoadRequestInput,
    VerifyRequestInput,
};
use time::OffsetDateTime;
use uuid::Uuid;

use super::runtime::{
    device_binding::DeviceBindingStore,
    fingerprint::{HardwareObserver, ObservedHardware},
    keyring::LicenseKeyring,
    LicenseRuntime,
};
use super::{BindingMatch, LicenseFormatKind, LicenseState};

#[derive(Default)]
struct VecIssuanceAudit {
    records: Vec<IssuanceAuditRecord>,
}

impl IssuanceAuditSink for VecIssuanceAudit {
    fn append(&mut self, record: &IssuanceAuditRecord) -> std::io::Result<()> {
        self.records.push(record.clone());
        Ok(())
    }
}

#[derive(Clone)]
struct FixedObserver(ObservedHardware);

impl HardwareObserver for FixedObserver {
    fn observe(&self) -> super::CmdResult<ObservedHardware> {
        Ok(self.0.clone())
    }
}

#[derive(Clone)]
struct FixedKeyring {
    key_id: &'static str,
    public_key: PublicKey,
}

impl LicenseKeyring for FixedKeyring {
    fn active_key(&self) -> PublicKey {
        self.public_key
    }

    fn resolve_key(&self, key_id: &str) -> Option<PublicKey> {
        (key_id == self.key_id).then_some(self.public_key)
    }
}

fn observer(
    machine_id: &str,
    cpu: &str,
    hostname: &str,
) -> Arc<dyn HardwareObserver + Send + Sync> {
    Arc::new(FixedObserver(ObservedHardware {
        platform: shared_core::Platform::Macos,
        machine_id: Some(machine_id.into()),
        cpu_model: Some(cpu.into()),
        hostname: Some(hostname.into()),
        locale: Some("en_US.UTF-8".into()),
        timezone: "-0600".into(),
    }))
}

fn runtime_with_observer(
    dir: &std::path::Path,
    observer: Arc<dyn HardwareObserver + Send + Sync>,
) -> (LicenseRuntime, Arc<dyn LicenseSigner>, KeyMetadataSnapshot) {
    let binding = DeviceBindingStore::load_or_init_from_dir_with_observer(dir, observer)
        .expect("init device binding");

    let seed = [0x42; 32];
    let verifier_provider =
        Ed25519CryptoProvider::new(Ed25519Keypair::from_seed_bytes("primary", &seed).unwrap());
    let public_key =
        PublicKey::from_bytes(&verifier_provider.verifying_key_bytes()).expect("public key");
    let keyring: Arc<dyn LicenseKeyring + Send + Sync> = Arc::new(FixedKeyring {
        key_id: "primary",
        public_key,
    });
    let runtime = LicenseRuntime::new(binding, keyring, LicenseState::default());
    let signer: Arc<dyn LicenseSigner> =
        Arc::new(InMemorySigner::from_seed(&seed, "primary").expect("signer"));
    let metadata = KeyMetadataSnapshot::new(
        Some("primary".into()),
        signer.key_version().map(str::to_string),
        ED25519_SIGNATURE_ALG,
    );
    (runtime, signer, metadata)
}

fn issue_modern_license(
    runtime: &LicenseRuntime,
    signer: Arc<dyn LicenseSigner>,
    key_metadata: KeyMetadataSnapshot,
) -> Vec<u8> {
    let (_request, request_bytes) = runtime
        .generate_request_bytes("monthly", Some("Cliente Phase 2".into()))
        .expect("generate request");

    let load = load_request(LoadRequestInput {
        bytes: request_bytes,
        legacy_adapter: &NullLegacyAdapter,
        crypto_provider: None,
        environment: licgen_core::verification::VerificationEnvironment::Development,
        auth_policy: licgen_core::request::RequestAuthPolicy::StrictV2,
    })
    .expect("load request");

    let verified = verify_request(VerifyRequestInput {
        request: load.request,
        shared_request: load.shared_request,
        format: load.format,
        environment: licgen_core::verification::VerificationEnvironment::Development,
        expected_key_id: Some("primary".into()),
        crypto_provider: None,
        auth_policy: licgen_core::request::RequestAuthPolicy::StrictV2,
    })
    .expect("verify request");

    let mut audit_trail = AuditTrail::default();
    let mut issuance_audit = VecIssuanceAudit::default();
    issue_license(IssueLicenseInput {
        request: verified.request,
        shared_request: verified.shared_request,
        signer,
        audit_trail: &mut audit_trail,
        issuance_audit: &mut issuance_audit,
        environment: licgen_core::verification::VerificationEnvironment::Development,
        allow_unsafe_plan: false,
        key_metadata,
        audit_output_path: None,
        audit_source: None,
        audit_operator: None,
    })
    .expect("issue license")
    .signed_license
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("phase2-binding-{label}-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn persisted_hardware_hash(dir: &std::path::Path) -> String {
    let file = dir.join("installation.json");
    let bytes = fs::read(file).expect("read installation file");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("installation json");
    value
        .get("hardware_hash_hex")
        .and_then(|value| value.as_str())
        .expect("hardware_hash_hex")
        .to_string()
}

fn now_ts() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

#[test]
fn request_uses_current_observed_binding() {
    let dir = temp_dir("req-current");
    let (runtime, _signer, _meta) =
        runtime_with_observer(&dir, observer("machine-a", "cpu-a", "host-a"));

    let (request, _bytes) = runtime
        .generate_request_bytes("monthly", Some("Observed Binding".into()))
        .expect("request");

    assert_eq!(
        request.installation.fingerprint.hardware_hash,
        runtime.device_hash_hex()
    );
}

/// Phase 2 verified clone detection and live observation.
/// Phase 3 changes one behavior: installation.json is no longer rewritten when
/// hardware changes. The in-memory hash still updates so evaluation reflects
/// current hardware, but the persisted file stays anchored to the original binding.
///
/// This test verifies the Phase 2 + Phase 3 combined contract:
/// - installation ID is preserved across reloads
/// - in-memory hash updates to reflect current hardware
/// - `installation.json` retains the anchored hash (no silent rewrite)
/// - requests after a hardware change encode the current observed hash
#[test]
fn persisted_binding_reflects_anchor_not_current_hardware() {
    let dir = temp_dir("no-rewrite");
    let (runtime_a, _signer_a, _meta_a) =
        runtime_with_observer(&dir, observer("machine-a", "cpu-a", "host-a"));
    let installation_id = runtime_a.binding().installation_id();
    let first_hash = runtime_a.device_hash_hex();

    let (runtime_b, _signer_b, _meta_b) =
        runtime_with_observer(&dir, observer("machine-b", "cpu-a", "host-a"));
    let second_hash = runtime_b.device_hash_hex();

    // Installation ID is preserved
    assert_eq!(installation_id, runtime_b.binding().installation_id());
    // In-memory hash reflects new hardware
    assert_ne!(first_hash, second_hash);
    // Phase 3 policy: persisted file must NOT have been rewritten
    assert_eq!(
        persisted_hardware_hash(&dir),
        first_hash,
        "installation.json must retain the original anchored hash"
    );
    // Hardware drift is detectable
    assert!(runtime_b.binding().has_hardware_drift());
    // Requests encode the current (new) hardware hash
    let (request, _) = runtime_b
        .generate_request_bytes("monthly", Some("Observed Binding".into()))
        .expect("request after hardware change");
    assert_eq!(request.installation.fingerprint.hardware_hash, second_hash);
}

#[test]
fn legitimate_installation_continues_to_validate() {
    let dir = temp_dir("legit");
    let (runtime, signer, metadata) =
        runtime_with_observer(&dir, observer("machine-a", "cpu-a", "host-a"));
    let license = issue_modern_license(&runtime, signer, metadata);
    let evaluation = runtime
        .evaluate_license_bytes(&license, now_ts())
        .expect("evaluate modern license");

    assert_eq!(
        evaluation.status,
        super::validator::LicenseRuntimeStatus::Active
    );
    assert_eq!(evaluation.license.format, LicenseFormatKind::ModernLicgen);
    assert_eq!(evaluation.license.binding, BindingMatch::Current);
}

#[test]
fn cloned_installation_json_and_license_do_not_validate_on_other_environment() {
    let dir_a = temp_dir("origin");
    let (runtime_a, signer_a, metadata_a) =
        runtime_with_observer(&dir_a, observer("machine-a", "cpu-a", "host-a"));
    let license = issue_modern_license(&runtime_a, signer_a, metadata_a);
    let origin_hash = runtime_a.device_hash_hex();

    let dir_b = temp_dir("clone");
    fs::copy(
        dir_a.join("installation.json"),
        dir_b.join("installation.json"),
    )
    .expect("clone installation state");
    let (runtime_b, _signer_b, _metadata_b) =
        runtime_with_observer(&dir_b, observer("machine-b", "cpu-a", "host-a"));
    let clone_hash = runtime_b.device_hash_hex();

    assert_ne!(origin_hash, clone_hash);
    let evaluation = runtime_b
        .evaluate_license_bytes(&license, now_ts())
        .expect("evaluation must classify binding mismatch");

    assert_eq!(
        evaluation.status,
        super::validator::LicenseRuntimeStatus::DeviceMismatch
    );
    assert_eq!(evaluation.license.binding, BindingMatch::Mismatch);
}
