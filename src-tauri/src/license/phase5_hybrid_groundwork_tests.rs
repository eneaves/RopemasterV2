/// Fase 5: tests de groundwork híbrido.
///
/// Verifica que:
/// 1. Licencias offline existentes (sin campos híbridos) siguen funcionando.
/// 2. Campos híbridos (lease_required, revocation_epoch, etc.) se parsean
///    sin romper el flujo de evaluación.
/// 3. `allowed_fingerprints` está enforced client-side desde Fase 5.
/// 4. `NormalizedLicense` expone los campos híbridos correctamente.
/// 5. No hay regresiones en el contrato key_id / key_version / binding.
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

/// Construye un blob LICGEN firmado con offline_policy y security_policy inyectables.
fn build_license_blob(
    keypair: &Keypair,
    _key_id: &str,
    installation_id: &str,
    device_hash_hex: &str,
    offline_policy: serde_json::Value,
    security_policy: serde_json::Value,
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
            "last_online_check_at": null
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
        "grace_days": 5,
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

// ─── 1. Backward compatibility ───────────────────────────────────────────────

/// Un payload que solo tiene `max_offline_days` (sin lease_required ni grace_days)
/// debe parsear correctamente gracias a `#[serde(default)]` en los campos nuevos.
#[test]
fn backward_compat_offline_policy_without_hybrid_fields() {
    let keypair = test_keypair();
    let device_hash = [0xAAu8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let installation_id = Uuid::new_v4().to_string();

    // offline_policy mínimo (como en licencias offline previas)
    let minimal_offline = json!({ "max_offline_days": 30 });
    // security_policy mínimo (sin revocation_epoch ni allowed_fingerprints)
    let minimal_security = json!({ "policy_version": 1, "key_id": "primary" });

    let bytes = build_license_blob(
        &keypair,
        "primary",
        &installation_id,
        &device_hash_hex,
        minimal_offline,
        minimal_security,
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
    .expect("minimal payload should be accepted");

    assert_eq!(result.status, LicenseRuntimeStatus::Active);
    // Campos híbridos defaulean a sus valores neutros
    assert!(!result.license.lease_required);
    assert_eq!(result.license.revocation_epoch, None);
    assert_eq!(result.license.allowed_fingerprints_count, 0);
    assert_eq!(result.license.max_offline_days, 30);
}

// ─── 2. lease_required groundwork ────────────────────────────────────────────

/// Una licencia con `lease_required=true` se acepta en modo offline (hoy, groundwork)
/// pero `NormalizedLicense.lease_required` es `true` para exposición al UI.
#[test]
fn lease_required_true_is_accepted_offline_but_exposed() {
    let keypair = test_keypair();
    let device_hash = [0xBBu8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let installation_id = Uuid::new_v4().to_string();

    let lease_policy = json!({
        "lease_required": true,
        "max_offline_days": 14,
        "grace_days": 3,
        "last_online_check_at": null
    });

    let bytes = build_license_blob(
        &keypair,
        "primary",
        &installation_id,
        &device_hash_hex,
        lease_policy,
        security_policy_base("primary"),
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
    .expect("lease_required license must be accepted in offline mode (groundwork)");

    // Groundwork: sigue Active sin servidor
    assert_eq!(result.status, LicenseRuntimeStatus::Active);
    // Expuesto para que el UI pueda advertir
    assert!(result.license.lease_required);
    assert_eq!(result.license.max_offline_days, 14);
}

// ─── 3. allowed_fingerprints enforced (Fase 5) ───────────────────────────────

/// allowed_fingerprints no vacío con device hash en la lista → Active.
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
        "primary",
        &installation_id,
        &device_hash_hex,
        pure_offline_policy(),
        security,
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

/// allowed_fingerprints no vacío con device hash fuera de la lista → DeviceMismatch.
/// Este es el enforcement real que faltaba en el cliente antes de Fase 5.
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
        "primary",
        &installation_id,
        &license_device_hash_hex,
        pure_offline_policy(),
        security,
    );

    let keyring = MultiKeyring::new().with_key("primary", keypair.public, None);
    let err = evaluate_license(
        &keyring,
        &bytes,
        &current_device_hash, // dispositivo diferente al de la lista
        None,
        &installation_id,
        "",
        1_700_000_100,
    )
    .expect_err("device NOT in allowed_fingerprints must be rejected");

    assert_eq!(err.code, "DeviceMismatch");
}

/// allowed_fingerprints vacío = sin restricción explícita (comportamiento original offline).
#[test]
fn allowed_fingerprints_empty_means_no_restriction() {
    let keypair = test_keypair();
    let device_hash = [0xFFu8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let installation_id = Uuid::new_v4().to_string();

    let security = json!({
        "policy_version": 1,
        "revocation_epoch": null,
        "key_id": "primary",
        "key_version": null,
        "allowed_fingerprints": []
    });

    let bytes = build_license_blob(
        &keypair,
        "primary",
        &installation_id,
        &device_hash_hex,
        pure_offline_policy(),
        security,
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
    .expect("empty allowed_fingerprints allows any device");

    assert_eq!(result.status, LicenseRuntimeStatus::Active);
    assert_eq!(result.license.allowed_fingerprints_count, 0);
}

/// allowed_fingerprints con múltiples entradas acepta cualquiera que coincida.
#[test]
fn allowed_fingerprints_multi_entry_accepts_any_matching() {
    let keypair = test_keypair();
    let device_hash = [0x77u8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let other_hash_hex = hex::encode([0x88u8; 32]);
    let installation_id = Uuid::new_v4().to_string();

    let security = json!({
        "policy_version": 1,
        "revocation_epoch": null,
        "key_id": "primary",
        "key_version": null,
        "allowed_fingerprints": [other_hash_hex, device_hash_hex]
    });

    let bytes = build_license_blob(
        &keypair,
        "primary",
        &installation_id,
        &device_hash_hex,
        pure_offline_policy(),
        security,
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
    .expect("device hash in multi-entry list must be accepted");
    assert_eq!(result.status, LicenseRuntimeStatus::Active);
    assert_eq!(result.license.allowed_fingerprints_count, 2);
}

// ─── 4. revocation_epoch groundwork ──────────────────────────────────────────

/// revocation_epoch presente → parseado, expuesto en NormalizedLicense, no bloquea hoy.
#[test]
fn revocation_epoch_parsed_and_exposed() {
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
        "primary",
        &installation_id,
        &device_hash_hex,
        pure_offline_policy(),
        security,
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
    .expect("revocation_epoch is groundwork, must not block today");

    assert_eq!(result.status, LicenseRuntimeStatus::Active);
    assert_eq!(result.license.revocation_epoch, Some(1_750_000_000));
}

// ─── 5. grace_days y last_online_check_at ────────────────────────────────────

#[test]
fn grace_days_and_last_online_check_at_parse_without_error() {
    let keypair = test_keypair();
    let device_hash = [0x22u8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let installation_id = Uuid::new_v4().to_string();

    let offline_with_all = json!({
        "lease_required": true,
        "max_offline_days": 7,
        "grace_days": 2,
        "last_online_check_at": "2024-01-15T10:00:00Z"
    });

    let bytes = build_license_blob(
        &keypair,
        "primary",
        &installation_id,
        &device_hash_hex,
        offline_with_all,
        security_policy_base("primary"),
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
    .expect("all hybrid offline fields must parse without error");

    assert_eq!(result.status, LicenseRuntimeStatus::Active);
    assert!(result.license.lease_required);
    assert_eq!(result.license.max_offline_days, 7);
}

// ─── 6. Regressions (Fase 1-4) ───────────────────────────────────────────────

/// Firma alterada sigue siendo rechazada con SignatureFailed.
#[test]
fn regression_altered_signature_still_rejected() {
    let keypair = test_keypair();
    let device_hash = [0x33u8; 32];
    let device_hash_hex = hex::encode(device_hash);
    let installation_id = Uuid::new_v4().to_string();

    let mut bytes = build_license_blob(
        &keypair,
        "primary",
        &installation_id,
        &device_hash_hex,
        pure_offline_policy(),
        security_policy_base("primary"),
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

/// Hardware hash diferente sigue produciendo DeviceMismatch.
#[test]
fn regression_hardware_hash_mismatch() {
    let keypair = test_keypair();
    let license_hash = [0x44u8; 32];
    let current_hash = [0x55u8; 32];
    let installation_id = Uuid::new_v4().to_string();

    let bytes = build_license_blob(
        &keypair,
        "primary",
        &installation_id,
        &hex::encode(license_hash),
        pure_offline_policy(),
        security_policy_base("primary"),
    );

    let keyring = MultiKeyring::new().with_key("primary", keypair.public, None);
    let result = evaluate_license(
        &keyring,
        &bytes,
        &current_hash,
        None,
        &installation_id,
        "",
        1_700_000_100,
    )
    .unwrap();
    assert_eq!(result.status, LicenseRuntimeStatus::DeviceMismatch);
}

/// key_version mismatch sigue siendo rechazado.
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
        "primary",
        &installation_id,
        &device_hash_hex,
        pure_offline_policy(),
        security,
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
