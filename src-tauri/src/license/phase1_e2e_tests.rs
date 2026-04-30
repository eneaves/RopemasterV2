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

#[derive(Clone)]
struct FixedObserver(ObservedHardware);

impl HardwareObserver for FixedObserver {
    fn observe(&self) -> super::CmdResult<ObservedHardware> {
        Ok(self.0.clone())
    }
}

fn observer() -> Arc<dyn HardwareObserver + Send + Sync> {
    Arc::new(FixedObserver(ObservedHardware {
        platform: shared_core::Platform::Macos,
        machine_id: Some("machine-phase1".into()),
        disk_serial: Some("disk-phase1".into()),
        cpu_model: Some("Apple M1".into()),
        hostname: Some("phase1-host".into()),
        locale: Some("en_US.UTF-8".into()),
        timezone: "-0600".into(),
    }))
}

fn runtime_with_issuer_key() -> (LicenseRuntime, Arc<dyn LicenseSigner>, KeyMetadataSnapshot) {
    let dir = std::env::temp_dir().join(format!("phase1-modern-runtime-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create temp binding dir");
    let binding = DeviceBindingStore::load_or_init_from_dir_with_observer(&dir, observer())
        .expect("init device binding");

    let seed = [0x42; 32];
    let signer_keypair = Ed25519Keypair::from_seed_bytes("primary", &seed).expect("issuer keypair");
    let verifier_provider = Ed25519CryptoProvider::new(signer_keypair);
    let public_key = PublicKey::from_bytes(&verifier_provider.verifying_key_bytes())
        .expect("convert issuer public key for app runtime");
    let keyring: Arc<dyn LicenseKeyring + Send + Sync> = Arc::new(FixedKeyring {
        key_id: "primary",
        public_key,
    });

    let runtime = LicenseRuntime::new(
        binding,
        keyring,
        LicenseState::default(),
        dir.clone(),
        licgen_core::verification::VerificationEnvironment::Development,
    );
    let signer: Arc<dyn LicenseSigner> =
        Arc::new(InMemorySigner::from_seed(&seed, "primary").expect("in-memory signer"));
    let metadata = KeyMetadataSnapshot::new(
        Some("primary".into()),
        signer.key_version().map(str::to_string),
        ED25519_SIGNATURE_ALG,
    );
    (runtime, signer, metadata)
}

#[test]
fn request_wire_contract_stays_stable() {
    let bytes = shared_core::test_vectors::valid_request_bytes();
    let parsed = shared_core::verify_request(&bytes).expect("parse shared_core request");
    let reencoded = shared_core::encode_wire_request(&parsed.request).expect("reencode request");
    assert_eq!(reencoded, bytes);
}

#[test]
fn phase1_req_issue_install_verify_active() {
    let (runtime, signer, key_metadata) = runtime_with_issuer_key();
    let (_request, request_bytes) = runtime
        .generate_request_bytes("monthly", Some("Cliente Phase 1".into()))
        .expect("generate request");

    let load = load_request(LoadRequestInput {
        bytes: request_bytes,
        legacy_adapter: &NullLegacyAdapter,
        crypto_provider: None,
        environment: licgen_core::verification::VerificationEnvironment::Development,
        auth_policy: licgen_core::request::RequestAuthPolicy::StrictV2,
    })
    .expect("load modern request");

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
    let issued = issue_license(IssueLicenseInput {
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
    .expect("issue modern license");

    assert!(issued.signed_license.starts_with(b"LICGEN"));

    let verify_now = issued.payload.issued_at.timestamp() + 1;
    let evaluation = runtime
        .evaluate_license_bytes(&issued.signed_license, verify_now)
        .expect("app evaluates modern license");
    assert_eq!(
        evaluation.status,
        super::validator::LicenseRuntimeStatus::Active
    );
    assert_eq!(evaluation.license.format, LicenseFormatKind::ModernLicgen);
    assert_eq!(evaluation.license.key_id.as_deref(), Some("primary"));
    assert_eq!(evaluation.license.binding, BindingMatch::Current);

    let blob = super::storage::StoredLicenseBlob {
        raw_bytes: issued.signed_license,
        installed_at: verify_now,
        last_verified_at: verify_now,
    };
    assert!(runtime.apply_stored_license_for_test(&blob, verify_now));
    let summary = runtime.summary();
    assert_eq!(summary.status, super::runtime::LicenseSummaryStatus::Active);
    assert_eq!(
        summary.license.expect("runtime summary license").format,
        LicenseFormatKind::ModernLicgen
    );
    assert_eq!(
        runtime
            .ensure_active()
            .expect("active cache after install")
            .license
            .license_id,
        issued.payload.license_id.to_string()
    );
    assert_eq!(issuance_audit.records.len(), 2);
}
