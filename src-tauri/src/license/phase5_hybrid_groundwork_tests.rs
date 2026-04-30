/// Fase 5/6: políticas híbridas fail-closed.
///
/// Verifica que:
/// 1. Licencias offline normales siguen funcionando.
/// 2. lease/check-in/revocación no soportados se rechazan explícitamente.
/// 3. allowed_fingerprints sigue enforced client-side.
/// 4. No hay regresiones en firma ni key_version.
use chrono::{Duration, TimeZone, Utc};
use ed25519_dalek::{Keypair, SecretKey, Signer};
use serde_json::json;
use uuid::Uuid;

use super::{
    modern,
    runtime::keyring::MultiKeyring,
    validator::{evaluate_license, LicenseRuntimeStatus},
};

fn test_keypair() -> Keypair {
    let secret = SecretKey::from_bytes(&[0x42; 32]).expect("32 bytes");
    let public: ed25519_dalek::PublicKey = (&secret).into();
    Keypair { secret, public }
}

fn build_license_blob(
    keypair: &Keypair,
    installation_id: &str,
    device_hash_hex: &str,
    offline_policy: serde_json::Value,
    security_policy: serde_json::Value,
    installation_last_online_check_at: serde_json::Value,
) -> Vec<u8> {
    let issued_at = Utc.timestamp_opt(1_700_000_000, 0).single().unwrap();
    let payload = json!({
        "license_version": modern::LICENSE_VERSION,
        "license_id": Uuid::new_v4(),
        "installation": {
            "installation_id": installation_id,
            "installation_pubkey": null,
            "device_fingerprint": {
                "version": 2,
                "hardware_hash": device_hash_hex,
                "platform": "macos",
                "components": [],
                "binding": { "stable": [], "strict": [], "observations": [] }
            },
            "first_seen_at": issued_at,
            "last_online_check_at": installation_last_online_check_at
        },
        "issued_at": issued_at,
        "expires_at": issued_at + Duration::days(365),
        "offline_policy": offline_policy,
        "security_policy": security_policy,
        "device_fingerprint_v2": {
            "version": 2,
            "hardware_hash": device_hash_hex,
            "platform": "macos",
            "components": [],
            "binding": { "stable": [], "strict": [], "observations": [] }
        },
        "metadata": {
            "app_id": "roping_manager",
            "plan": "monthly",
            "customer_name_hint": "Phase5 Test"
        }
    });
    let payload_bytes = serde_json::to_vec(&payload).unwrap();
    let signature = keypair.sign(&payload_bytes).to_bytes();
    let mut out = Vec::new();
    out.extend_from_slice(modern::LICENSE_MAGIC);
    out.extend_from_slice(&modern::FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(payload_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload_bytes);
    out.extend_from_slice(&(signature.len() as u16).to_le_bytes());
    out.extend_from_slice(&signature);
    out
}

fn pure_offline_policy() -> serde_json::Value {
    json!({
        "lease_required": false,
        "max_offline_days": 30,
        "grace_days": 0,
        "last_online_check_at": null
    })
}

fn security_policy_base(key_id: &str) -> serde_json::Value {
    json!({
        "policy_version": 1,
        "revocation_epoch": null,
        "key_id": key_id,
        "key_version": null,
        "allowed_fingerprints": []
    })
}

#[test]
fn backward_compat_offline_policy_without_hybrid_fields() {
    let keypair = test_keypair();
    let device_hash = [0xAAu8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let installation_id = Uuid::new_v4().to_string();

    let minimal_offline = json!({ "max_offline_days": 30 });
    let minimal_security = json!({ "policy_version": 1, "key_id": "primary" });

    let bytes = build_license_blob(
        &keypair,
        &installation_id,
        &device_hash_hex,
        minimal_offline,
        minimal_security,
        serde_json::Value::Null,
    );

    let keyring = MultiKeyring::new().with_key("primary", keypair.public, None);
    let result = evaluate_license(
        &keyring,
        &bytes,
        &device_hash,
        None,
        &installation_id,
        "",
        1_700_000_100,
    )
    .expect("minimal offline payload should be accepted");

    assert_eq!(result.status, LicenseRuntimeStatus::Active);
    assert!(!result.license.lease_required);
    assert_eq!(result.license.revocation_epoch, None);
    assert_eq!(result.license.allowed_fingerprints_count, 0);
    assert_eq!(result.license.max_offline_days, 30);
}

#[test]
fn lease_required_true_is_rejected_offline() {
    let keypair = test_keypair();
    let device_hash = [0xBBu8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let installation_id = Uuid::new_v4().to_string();

    let lease_policy = json!({
        "lease_required": true,
        "max_offline_days": 14,
        "grace_days": 0,
        "last_online_check_at": null
    });

    let bytes = build_license_blob(
        &keypair,
        &installation_id,
        &device_hash_hex,
        lease_policy,
        security_policy_base("primary"),
        serde_json::Value::Null,
    );

    let keyring = MultiKeyring::new().with_key("primary", keypair.public, None);
    let err = evaluate_license(
        &keyring,
        &bytes,
        &device_hash,
        None,
        &installation_id,
        "",
        1_700_000_100,
    )
    .expect_err("lease_required must fail closed");

    assert_eq!(err.code, "LeaseUnsupported");
}

#[test]
fn revocation_epoch_present_is_rejected() {
    let keypair = test_keypair();
    let device_hash = [0x11u8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let installation_id = Uuid::new_v4().to_string();

    let security = json!({
        "policy_version": 1,
        "revocation_epoch": 1_750_000_000u64,
        "key_id": "primary",
        "key_version": null,
        "allowed_fingerprints": []
    });

    let bytes = build_license_blob(
        &keypair,
        &installation_id,
        &device_hash_hex,
        pure_offline_policy(),
        security,
        serde_json::Value::Null,
    );

    let keyring = MultiKeyring::new().with_key("primary", keypair.public, None);
    let err = evaluate_license(
        &keyring,
        &bytes,
        &device_hash,
        None,
        &installation_id,
        "",
        1_700_000_100,
    )
    .expect_err("revocation_epoch must fail closed");

    assert_eq!(err.code, "RevocationUnsupported");
}

#[test]
fn grace_days_and_last_online_check_at_are_rejected() {
    let keypair = test_keypair();
    let device_hash = [0x22u8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let installation_id = Uuid::new_v4().to_string();

    let offline_with_checkin = json!({
        "lease_required": false,
        "max_offline_days": 7,
        "grace_days": 2,
        "last_online_check_at": "2024-01-15T10:00:00Z"
    });

    let bytes = build_license_blob(
        &keypair,
        &installation_id,
        &device_hash_hex,
        offline_with_checkin,
        security_policy_base("primary"),
        serde_json::Value::Null,
    );

    let keyring = MultiKeyring::new().with_key("primary", keypair.public, None);
    let err = evaluate_license(
        &keyring,
        &bytes,
        &device_hash,
        None,
        &installation_id,
        "",
        1_700_000_100,
    )
    .expect_err("grace/check-in fields must fail closed");

    assert_eq!(err.code, "HybridPolicyUnsupported");
}

#[test]
fn installation_last_online_check_at_is_rejected() {
    let keypair = test_keypair();
    let device_hash = [0x23u8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let installation_id = Uuid::new_v4().to_string();

    let bytes = build_license_blob(
        &keypair,
        &installation_id,
        &device_hash_hex,
        pure_offline_policy(),
        security_policy_base("primary"),
        json!("2024-01-15T10:00:00Z"),
    );

    let keyring = MultiKeyring::new().with_key("primary", keypair.public, None);
    let err = evaluate_license(
        &keyring,
        &bytes,
        &device_hash,
        None,
        &installation_id,
        "",
        1_700_000_100,
    )
    .expect_err("installation check-in fields must fail closed");

    assert_eq!(err.code, "HybridPolicyUnsupported");
}

#[test]
fn allowed_fingerprints_matching_device_is_active() {
    let keypair = test_keypair();
    let device_hash = [0xCCu8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let installation_id = Uuid::new_v4().to_string();

    let security = json!({
        "policy_version": 1,
        "revocation_epoch": null,
        "key_id": "primary",
        "key_version": null,
        "allowed_fingerprints": [device_hash_hex]
    });

    let bytes = build_license_blob(
        &keypair,
        &installation_id,
        &device_hash_hex,
        pure_offline_policy(),
        security,
        serde_json::Value::Null,
    );

    let keyring = MultiKeyring::new().with_key("primary", keypair.public, None);
    let result = evaluate_license(
        &keyring,
        &bytes,
        &device_hash,
        None,
        &installation_id,
        "",
        1_700_000_100,
    )
    .expect("device in allowed_fingerprints must be accepted");

    assert_eq!(result.status, LicenseRuntimeStatus::Active);
    assert_eq!(result.license.allowed_fingerprints_count, 1);
}

#[test]
fn allowed_fingerprints_non_matching_device_is_rejected() {
    let keypair = test_keypair();
    let license_device_hash = [0xDDu8; 32];
    let current_device_hash = [0xEEu8; 32];
    let license_device_hash_hex = hex::encode(license_device_hash);
    let installation_id = Uuid::new_v4().to_string();

    let security = json!({
        "policy_version": 1,
        "revocation_epoch": null,
        "key_id": "primary",
        "key_version": null,
        "allowed_fingerprints": [license_device_hash_hex]
    });

    let bytes = build_license_blob(
        &keypair,
        &installation_id,
        &license_device_hash_hex,
        pure_offline_policy(),
        security,
        serde_json::Value::Null,
    );

    let keyring = MultiKeyring::new().with_key("primary", keypair.public, None);
    let err = evaluate_license(
        &keyring,
        &bytes,
        &current_device_hash,
        None,
        &installation_id,
        "",
        1_700_000_100,
    )
    .expect_err("device outside allowed_fingerprints must be rejected");

    assert_eq!(err.code, "DeviceMismatch");
}

#[test]
fn regression_altered_signature_still_rejected() {
    let keypair = test_keypair();
    let device_hash = [0x33u8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let installation_id = Uuid::new_v4().to_string();

    let mut bytes = build_license_blob(
        &keypair,
        &installation_id,
        &device_hash_hex,
        pure_offline_policy(),
        security_policy_base("primary"),
        serde_json::Value::Null,
    );
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;

    let keyring = MultiKeyring::new().with_key("primary", keypair.public, None);
    let err = evaluate_license(
        &keyring,
        &bytes,
        &device_hash,
        None,
        &installation_id,
        "",
        1_700_000_100,
    )
    .expect_err("altered signature must fail");
    assert_eq!(err.code, "SignatureFailed");
}

#[test]
fn regression_key_version_mismatch() {
    let keypair = test_keypair();
    let device_hash = [0x66u8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let installation_id = Uuid::new_v4().to_string();

    let security = json!({
        "policy_version": 1,
        "revocation_epoch": null,
        "key_id": "primary",
        "key_version": "2026.04",
        "allowed_fingerprints": []
    });

    let bytes = build_license_blob(
        &keypair,
        &installation_id,
        &device_hash_hex,
        pure_offline_policy(),
        security,
        serde_json::Value::Null,
    );

    let keyring =
        MultiKeyring::new().with_key("primary", keypair.public, Some("2025.01".to_string()));
    let err = evaluate_license(
        &keyring,
        &bytes,
        &device_hash,
        None,
        &installation_id,
        "",
        1_700_000_100,
    )
    .expect_err("key_version mismatch must still fail");
    assert_eq!(err.code, "KeyVersionMismatch");
}
