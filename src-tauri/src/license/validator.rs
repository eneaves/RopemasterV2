/// The app_id this installation targets. Re-exported from shared_core — single source of truth.
pub use shared_core::DEFAULT_APP_ID;

use super::NormalizedLicense;

#[cfg(test)]
use sha2::{Digest, Sha256};

#[cfg(test)]
use super::{
    modern,
    runtime::keyring::{KeyLookupError, LicenseKeyring},
    BindingMatch, CommandError, LicenseFormatKind, NormalizedFailureReason,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseRuntimeStatus {
    Active,
    NotYetValid,
    Expired,
    DeviceMismatch,
}

#[derive(Debug, Clone)]
pub struct LicenseEvaluation {
    pub license: NormalizedLicense,
    pub status: LicenseRuntimeStatus,
}

#[cfg(test)]
pub fn evaluate_license(
    keyring: &dyn LicenseKeyring,
    license_bytes: &[u8],
    device_hash: &[u8; 32],
    legacy_device_hash: Option<&[u8; 32]>,
    installation_id: &str,
    installation_pubkey_b64: &str,
    now: i64,
) -> Result<LicenseEvaluation, CommandError> {
    // Phase 3: only the modern LICGEN format is accepted.
    // The legacy CBOR path has been retired — no new licenses are issued in that format
    // and no installations carry legacy licenses.
    if !license_bytes.starts_with(modern::LICENSE_MAGIC) {
        return Err(CommandError::new(
            "LegacyUnsupported",
            "This installation only accepts modern LICGEN licenses. \
             Legacy CBOR licenses (format < LICGEN) are no longer supported. \
             Please obtain a new license from the generator.",
        ));
    }

    evaluate_modern_license(
        keyring,
        license_bytes,
        device_hash,
        legacy_device_hash,
        installation_id,
        installation_pubkey_b64,
        now,
    )
}

#[cfg(test)]
fn evaluate_modern_license(
    keyring: &dyn LicenseKeyring,
    license_bytes: &[u8],
    device_hash: &[u8; 32],
    legacy_device_hash: Option<&[u8; 32]>,
    installation_id: &str,
    installation_pubkey_b64: &str,
    now: i64,
) -> Result<LicenseEvaluation, CommandError> {
    let parsed = modern::parse_license(license_bytes).map_err(map_modern_error)?;
    if parsed.payload.installation.device_fingerprint.hardware_hash
        != parsed.payload.device_fingerprint_v2.hardware_hash
    {
        return Err(CommandError::new(
            "Parse",
            "La licencia moderna contiene fingerprints inconsistentes.",
        ));
    }

    let key_id = parsed
        .payload
        .security_policy
        .key_id
        .clone()
        .ok_or_else(|| {
            CommandError::new("MissingKeyId", "La licencia moderna no declara key_id.")
        })?;
    let key_version = parsed.payload.security_policy.key_version.as_deref();
    let public_key = keyring
        .lookup_key(&key_id, key_version)
        .map_err(map_key_lookup_error)?
        .public_key;
    parsed
        .verify_signature(&public_key)
        .map_err(map_modern_error)?;
    reject_unsupported_hybrid_policies(&parsed.payload)?;

    let app_id = parsed
        .metadata_string("app_id")
        .unwrap_or_default()
        .trim()
        .to_string();
    if app_id != DEFAULT_APP_ID {
        return Err(CommandError::new(
            "AppIdMismatch",
            format!("license targets app_id={app_id}"),
        ));
    }

    // Fase 5: enforcement de allowed_fingerprints en client-side.
    // Si la lista no está vacía, el hardware_hash del dispositivo actual DEBE estar
    // en ella. Lista vacía = sin restricción (comportamiento original offline).
    // Este enforcement espeja el lado del generador (LicenseVerificationHandle::enforce_fingerprint).
    let allowed = &parsed.payload.security_policy.allowed_fingerprints;
    if !allowed.is_empty() {
        let current_hex = hex::encode(device_hash);
        if !allowed.contains(&current_hex) {
            return Err(CommandError::new(
                "DeviceMismatch",
                "El dispositivo no está en la lista de fingerprints autorizados para esta licencia.",
            ));
        }
    }

    let binding = classify_modern_binding(
        &parsed,
        &hex::encode(device_hash),
        legacy_device_hash.map(hex::encode),
        installation_id,
        installation_pubkey_b64,
    );
    let status = classify_runtime(
        parsed.payload.issued_at.timestamp(),
        parsed.payload.expires_at.timestamp(),
        modern::DEFAULT_MAX_CLOCK_SKEW_SECS,
        binding,
        now,
    );

    let license = NormalizedLicense {
        format: LicenseFormatKind::ModernLicgen,
        format_version: parsed.format_version,
        app_id,
        signature_valid: true,
        key_id: Some(key_id),
        key_version: parsed.payload.security_policy.key_version.clone(),
        license_id: parsed.payload.license_id.to_string(),
        plan: parsed.metadata_string("plan").map(str::to_string),
        customer_name: parsed
            .metadata_string("customer_name_hint")
            .map(str::to_string),
        issued_at: parsed.payload.issued_at.timestamp(),
        not_before: parsed.payload.issued_at.timestamp(),
        not_after: parsed.payload.expires_at.timestamp(),
        max_clock_skew: modern::DEFAULT_MAX_CLOCK_SKEW_SECS,
        max_offline_days: parsed.payload.offline_policy.max_offline_days,
        lease_required: parsed.payload.offline_policy.lease_required,
        revocation_epoch: parsed.payload.security_policy.revocation_epoch,
        allowed_fingerprints_count: parsed.payload.security_policy.allowed_fingerprints.len(),
        device_hash_hex: parsed.payload.device_fingerprint_v2.hardware_hash.clone(),
        installation_id: Some(parsed.payload.installation.installation_id.to_string()),
        installation_pubkey: parsed.payload.installation.installation_pubkey.clone(),
        binding,
        blob_len: license_bytes.len(),
        blob_sha256: sha256_hex(license_bytes),
        failure_reason: failure_reason_for_status(status),
    };

    Ok(LicenseEvaluation { license, status })
}

#[cfg(test)]
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

#[cfg(test)]
fn reject_unsupported_hybrid_policies(
    payload: &modern::ModernLicensePayload,
) -> Result<(), CommandError> {
    if payload.offline_policy.lease_required {
        return Err(CommandError::new(
            "LeaseUnsupported",
            "offline_policy.lease_required requiere lease/check-in online real y hoy no está soportado.",
        ));
    }
    if payload.offline_policy.grace_days > 0 {
        return Err(CommandError::new(
            "HybridPolicyUnsupported",
            "offline_policy.grace_days requiere enforcement real de lease/check-in y hoy no está soportado.",
        ));
    }
    if payload.offline_policy.last_online_check_at.is_some() {
        return Err(CommandError::new(
            "HybridPolicyUnsupported",
            "offline_policy.last_online_check_at requiere check-in online real y hoy no está soportado.",
        ));
    }
    if payload.installation.last_online_check_at.is_some() {
        return Err(CommandError::new(
            "HybridPolicyUnsupported",
            "installation.last_online_check_at requiere check-in online real y hoy no está soportado.",
        ));
    }
    if payload.security_policy.revocation_epoch.is_some() {
        return Err(CommandError::new(
            "RevocationUnsupported",
            "security_policy.revocation_epoch requiere revocación online real y hoy no está soportado.",
        ));
    }
    Ok(())
}

#[cfg(test)]
fn classify_modern_binding(
    parsed: &modern::ParsedModernLicense,
    current_device_hash_hex: &str,
    legacy_device_hash_hex: Option<String>,
    installation_id: &str,
    installation_pubkey_b64: &str,
) -> BindingMatch {
    let matches_installation_id =
        parsed.payload.installation.installation_id.to_string() == installation_id;
    let matches_installation_pubkey = parsed
        .payload
        .installation
        .installation_pubkey
        .as_deref()
        .is_none_or(|value| value == installation_pubkey_b64);
    if !matches_installation_id || !matches_installation_pubkey {
        return BindingMatch::Mismatch;
    }

    if parsed.payload.device_fingerprint_v2.hardware_hash == current_device_hash_hex {
        return BindingMatch::Current;
    }
    if legacy_device_hash_hex
        .as_deref()
        .is_some_and(|value| value == parsed.payload.device_fingerprint_v2.hardware_hash)
    {
        return BindingMatch::LegacyCompat;
    }
    BindingMatch::Mismatch
}

#[cfg(test)]
fn classify_runtime(
    not_before: i64,
    not_after: i64,
    max_clock_skew: i64,
    binding: BindingMatch,
    now: i64,
) -> LicenseRuntimeStatus {
    if binding == BindingMatch::Mismatch {
        return LicenseRuntimeStatus::DeviceMismatch;
    }

    if now.saturating_add(max_clock_skew) < not_before {
        return LicenseRuntimeStatus::NotYetValid;
    }
    if now.saturating_sub(max_clock_skew) > not_after {
        return LicenseRuntimeStatus::Expired;
    }
    LicenseRuntimeStatus::Active
}

#[cfg(test)]
fn failure_reason_for_status(status: LicenseRuntimeStatus) -> Option<NormalizedFailureReason> {
    match status {
        LicenseRuntimeStatus::Active => None,
        LicenseRuntimeStatus::NotYetValid => Some(NormalizedFailureReason::NotYetValid),
        LicenseRuntimeStatus::Expired => Some(NormalizedFailureReason::Expired),
        LicenseRuntimeStatus::DeviceMismatch => Some(NormalizedFailureReason::DeviceMismatch),
    }
}

#[cfg(test)]
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
fn map_modern_error(err: modern::ModernLicenseError) -> CommandError {
    match err {
        modern::ModernLicenseError::InvalidSignature => CommandError::new(
            "SignatureFailed",
            "La firma de la licencia moderna es inválida.",
        ),
        modern::ModernLicenseError::UnsupportedFormatVersion { .. }
        | modern::ModernLicenseError::UnsupportedLicenseVersion { .. } => {
            CommandError::new("LegacyUnsupported", err.to_string())
        }
        modern::ModernLicenseError::InvalidFormat(_) => CommandError::parse(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signer};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

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

    fn test_keypair() -> Keypair {
        let secret =
            SecretKey::from_bytes(&[0x11; 32]).expect("secret key should be exactly 32 bytes");
        let public: ed25519_dalek::PublicKey = (&secret).into();
        Keypair { secret, public }
    }

    fn encode_modern_license_bytes(
        keypair: &Keypair,
        key_id: &str,
        app_id: &str,
        installation_id: &str,
        installation_pubkey_b64: &str,
        device_hash_hex: &str,
    ) -> Vec<u8> {
        encode_modern_license_bytes_with_key_version(
            keypair,
            key_id,
            "2026.04",
            app_id,
            installation_id,
            installation_pubkey_b64,
            device_hash_hex,
        )
    }

    fn encode_modern_license_bytes_with_key_version(
        keypair: &Keypair,
        key_id: &str,
        key_version: &str,
        app_id: &str,
        installation_id: &str,
        installation_pubkey_b64: &str,
        device_hash_hex: &str,
    ) -> Vec<u8> {
        let issued_at = Utc.timestamp_opt(1_700_000_000, 0).single().unwrap();
        let payload = json!({
            "license_version": modern::LICENSE_VERSION,
            "license_id": Uuid::new_v4(),
            "installation": {
                "installation_id": installation_id,
                "installation_pubkey": installation_pubkey_b64,
                "device_fingerprint": {
                    "version": 2,
                    "hardware_hash": device_hash_hex,
                    "platform": "macos",
                    "components": [],
                    "binding": {
                        "stable": [],
                        "strict": [],
                        "observations": []
                    }
                },
                "first_seen_at": issued_at,
                "last_online_check_at": null
            },
            "issued_at": issued_at,
            "expires_at": issued_at + Duration::days(30),
            "offline_policy": {
                "lease_required": false,
                "max_offline_days": 30,
                "grace_days": 0,
                "last_online_check_at": null
            },
            "security_policy": {
                "policy_version": 1,
                "revocation_epoch": null,
                "key_id": key_id,
                "key_version": key_version,
                "allowed_fingerprints": []
            },
            "device_fingerprint_v2": {
                "version": 2,
                "hardware_hash": device_hash_hex,
                "platform": "macos",
                "components": [],
                "binding": {
                    "stable": [],
                    "strict": [],
                    "observations": []
                }
            },
            "metadata": {
                "app_id": app_id,
                "plan": "monthly",
                "customer_name_hint": "Test Modern"
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

    #[test]
    fn valid_modern_license_classifies_as_active() {
        let keypair = test_keypair();
        let device_hash = [9u8; 32];
        let device_hash_hex = hex::encode(device_hash);
        let installation_id = Uuid::new_v4().to_string();
        let installation_pubkey = "base64-installation-pubkey";
        let bytes = encode_modern_license_bytes(
            &keypair,
            "primary",
            DEFAULT_APP_ID,
            &installation_id,
            installation_pubkey,
            &device_hash_hex,
        );
        let keyring = FixedKeyring {
            key_id: "primary",
            public_key: keypair.public,
        };

        let result = evaluate_license(
            &keyring,
            &bytes,
            &device_hash,
            None,
            &installation_id,
            installation_pubkey,
            1_700_000_100,
        )
        .unwrap();
        assert_eq!(result.status, LicenseRuntimeStatus::Active);
        assert_eq!(result.license.format, LicenseFormatKind::ModernLicgen);
        assert_eq!(result.license.key_id.as_deref(), Some("primary"));
    }

    #[test]
    fn altered_modern_license_is_rejected() {
        let keypair = test_keypair();
        let device_hash = [3u8; 32];
        let installation_id = Uuid::new_v4().to_string();
        let installation_pubkey = "base64-installation-pubkey";
        let mut bytes = encode_modern_license_bytes(
            &keypair,
            "primary",
            DEFAULT_APP_ID,
            &installation_id,
            installation_pubkey,
            &hex::encode(device_hash),
        );
        let idx = bytes.len() - 1;
        bytes[idx] ^= 0x01;
        let keyring = FixedKeyring {
            key_id: "primary",
            public_key: keypair.public,
        };

        let err = evaluate_license(
            &keyring,
            &bytes,
            &device_hash,
            None,
            &installation_id,
            installation_pubkey,
            1_700_000_100,
        )
        .unwrap_err();
        assert_eq!(err.code, "SignatureFailed");
    }

    #[test]
    fn unknown_modern_key_id_is_rejected() {
        let keypair = test_keypair();
        let device_hash = [5u8; 32];
        let installation_id = Uuid::new_v4().to_string();
        let installation_pubkey = "base64-installation-pubkey";
        let bytes = encode_modern_license_bytes(
            &keypair,
            "rotated",
            DEFAULT_APP_ID,
            &installation_id,
            installation_pubkey,
            &hex::encode(device_hash),
        );
        let keyring = FixedKeyring {
            key_id: "primary",
            public_key: keypair.public,
        };

        let err = evaluate_license(
            &keyring,
            &bytes,
            &device_hash,
            None,
            &installation_id,
            installation_pubkey,
            1_700_000_100,
        )
        .unwrap_err();
        assert_eq!(err.code, "UnknownKeyId");
    }

    #[test]
    fn wrong_modern_app_id_is_rejected() {
        let keypair = test_keypair();
        let device_hash = [8u8; 32];
        let installation_id = Uuid::new_v4().to_string();
        let installation_pubkey = "base64-installation-pubkey";
        let bytes = encode_modern_license_bytes(
            &keypair,
            "primary",
            "other_app",
            &installation_id,
            installation_pubkey,
            &hex::encode(device_hash),
        );
        let keyring = FixedKeyring {
            key_id: "primary",
            public_key: keypair.public,
        };

        let err = evaluate_license(
            &keyring,
            &bytes,
            &device_hash,
            None,
            &installation_id,
            installation_pubkey,
            1_700_000_100,
        )
        .unwrap_err();
        assert_eq!(err.code, "AppIdMismatch");
    }

    /// Phase 3: legacy CBOR bytes must be explicitly rejected with LegacyUnsupported.
    #[test]
    fn legacy_cbor_bytes_rejected_with_explicit_error() {
        let key_id = "primary";
        let secret = SecretKey::from_bytes(&[0x11; 32]).unwrap();
        let public: ed25519_dalek::PublicKey = (&secret).into();
        let keypair = Keypair {
            secret: SecretKey::from_bytes(&[0x11; 32]).unwrap(),
            public,
        };
        let device_hash = [7u8; 32];
        // Construct arbitrary bytes that do NOT start with LICGEN magic
        // (simulate old CBOR license blob)
        let cbor_like = {
            let mut b = Vec::new();
            b.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // BE length prefix (CBOR-style)
            b.extend_from_slice(&[0xA1, 0x63, 0x61, 0x62]); // random CBOR bytes
            b
        };
        let keyring = FixedKeyring {
            key_id,
            public_key: keypair.public,
        };
        let err = evaluate_license(
            &keyring,
            &cbor_like,
            &device_hash,
            None,
            "unused",
            "unused",
            1_700_000_100,
        )
        .unwrap_err();
        assert_eq!(err.code, "LegacyUnsupported");
    }

    /// Phase 3: modern license with wrong device hash produces DeviceMismatch (not Active).
    #[test]
    fn modern_license_wrong_device_hash_produces_mismatch() {
        let keypair = test_keypair();
        let license_device_hash = [9u8; 32];
        let current_device_hash = [5u8; 32]; // different from license
        let installation_id = Uuid::new_v4().to_string();
        let installation_pubkey = "base64-installation-pubkey";
        let bytes = encode_modern_license_bytes(
            &keypair,
            "primary",
            DEFAULT_APP_ID,
            &installation_id,
            installation_pubkey,
            &hex::encode(license_device_hash),
        );
        let keyring = FixedKeyring {
            key_id: "primary",
            public_key: keypair.public,
        };

        let result = evaluate_license(
            &keyring,
            &bytes,
            &current_device_hash, // different from what's in the license
            None,
            &installation_id,
            installation_pubkey,
            1_700_000_100,
        )
        .unwrap();
        assert_eq!(result.status, LicenseRuntimeStatus::DeviceMismatch);
        assert_eq!(result.license.binding, BindingMatch::Mismatch);
    }

    /// Phase 4: keyring with required key_version accepts a matching version in the license.
    #[test]
    fn key_version_match_passes_verification() {
        use crate::license::runtime::keyring::MultiKeyring;

        let keypair = test_keypair();
        let device_hash = [9u8; 32];
        let installation_id = Uuid::new_v4().to_string();
        let installation_pubkey = "base64-pubkey";
        // encode_modern_license_bytes embeds key_version = "2026.04"
        let bytes = encode_modern_license_bytes(
            &keypair,
            "primary",
            DEFAULT_APP_ID,
            &installation_id,
            installation_pubkey,
            &hex::encode(device_hash),
        );
        let keyring =
            MultiKeyring::new().with_key("primary", keypair.public, Some("2026.04".to_string()));
        let result = evaluate_license(
            &keyring,
            &bytes,
            &device_hash,
            None,
            &installation_id,
            installation_pubkey,
            1_700_000_100,
        )
        .unwrap();
        assert_eq!(result.status, LicenseRuntimeStatus::Active);
    }

    /// Phase 4: keyring with required key_version rejects a license with wrong version.
    #[test]
    fn key_version_mismatch_produces_error() {
        use crate::license::runtime::keyring::MultiKeyring;

        let keypair = test_keypair();
        let device_hash = [9u8; 32];
        let installation_id = Uuid::new_v4().to_string();
        let installation_pubkey = "base64-pubkey";
        // encode_modern_license_bytes embeds key_version = "2026.04"
        let bytes = encode_modern_license_bytes(
            &keypair,
            "primary",
            DEFAULT_APP_ID,
            &installation_id,
            installation_pubkey,
            &hex::encode(device_hash),
        );
        // Keyring requires a *different* version
        let keyring =
            MultiKeyring::new().with_key("primary", keypair.public, Some("2025.01".to_string()));
        let err = evaluate_license(
            &keyring,
            &bytes,
            &device_hash,
            None,
            &installation_id,
            installation_pubkey,
            1_700_000_100,
        )
        .unwrap_err();
        assert_eq!(err.code, "KeyVersionMismatch");
    }

    /// Phase 4: keyring without version constraint accepts any key_version.
    #[test]
    fn keyring_without_version_constraint_accepts_any_version() {
        use crate::license::runtime::keyring::MultiKeyring;

        let keypair = test_keypair();
        let device_hash = [9u8; 32];
        let installation_id = Uuid::new_v4().to_string();
        let installation_pubkey = "base64-pubkey";
        let bytes = encode_modern_license_bytes(
            &keypair,
            "primary",
            DEFAULT_APP_ID,
            &installation_id,
            installation_pubkey,
            &hex::encode(device_hash),
        );
        // No version constraint → any key_version in license is accepted
        let keyring = MultiKeyring::new().with_key("primary", keypair.public, None);
        let result = evaluate_license(
            &keyring,
            &bytes,
            &device_hash,
            None,
            &installation_id,
            installation_pubkey,
            1_700_000_100,
        )
        .unwrap();
        assert_eq!(result.status, LicenseRuntimeStatus::Active);
    }

    #[test]
    fn accepted_key_version_passes_verification() {
        use crate::license::runtime::keyring::{KeyStatus, MultiKeyring};

        let keypair = test_keypair();
        let device_hash = [9u8; 32];
        let installation_id = Uuid::new_v4().to_string();
        let installation_pubkey = "base64-pubkey";
        let bytes = encode_modern_license_bytes_with_key_version(
            &keypair,
            "primary",
            "2026.04",
            DEFAULT_APP_ID,
            &installation_id,
            installation_pubkey,
            &hex::encode(device_hash),
        );
        let keyring = MultiKeyring::new().with_key_status(
            "primary",
            keypair.public,
            Some("2026.04".to_string()),
            KeyStatus::Accepted,
        );
        let result = evaluate_license(
            &keyring,
            &bytes,
            &device_hash,
            None,
            &installation_id,
            installation_pubkey,
            1_700_000_100,
        )
        .unwrap();
        assert_eq!(result.status, LicenseRuntimeStatus::Active);
    }

    #[test]
    fn deprecated_key_version_passes_verification() {
        use crate::license::runtime::keyring::{KeyStatus, MultiKeyring};

        let keypair = test_keypair();
        let device_hash = [9u8; 32];
        let installation_id = Uuid::new_v4().to_string();
        let installation_pubkey = "base64-pubkey";
        let bytes = encode_modern_license_bytes_with_key_version(
            &keypair,
            "primary",
            "2026.04",
            DEFAULT_APP_ID,
            &installation_id,
            installation_pubkey,
            &hex::encode(device_hash),
        );
        let keyring = MultiKeyring::new().with_key_status(
            "primary",
            keypair.public,
            Some("2026.04".to_string()),
            KeyStatus::Deprecated,
        );
        let result = evaluate_license(
            &keyring,
            &bytes,
            &device_hash,
            None,
            &installation_id,
            installation_pubkey,
            1_700_000_100,
        )
        .unwrap();
        assert_eq!(result.status, LicenseRuntimeStatus::Active);
    }

    #[test]
    fn retired_key_version_is_rejected_by_policy() {
        use crate::license::runtime::keyring::{KeyStatus, MultiKeyring};

        let keypair = test_keypair();
        let device_hash = [9u8; 32];
        let installation_id = Uuid::new_v4().to_string();
        let installation_pubkey = "base64-pubkey";
        let bytes = encode_modern_license_bytes_with_key_version(
            &keypair,
            "primary",
            "2026.01",
            DEFAULT_APP_ID,
            &installation_id,
            installation_pubkey,
            &hex::encode(device_hash),
        );
        let keyring = MultiKeyring::new().with_key_status(
            "primary",
            keypair.public,
            Some("2026.01".to_string()),
            KeyStatus::Retired,
        );
        let err = evaluate_license(
            &keyring,
            &bytes,
            &device_hash,
            None,
            &installation_id,
            installation_pubkey,
            1_700_000_100,
        )
        .unwrap_err();
        assert_eq!(err.code, "RetiredKey");
    }
}
