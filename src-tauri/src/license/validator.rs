use std::convert::TryFrom;

use ed25519_dalek::PublicKey;
use license_core::{
    LicenseError, LicensePayload, ValidationErrorKind, DEFAULT_APP_ID, PAYLOAD_VERSION_CURRENT,
};

use super::CommandError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseRuntimeStatus {
    Active,
    NotYetValid,
    Expired,
    DeviceMismatch,
}

#[derive(Debug, Clone)]
pub struct LicenseEvaluation {
    pub payload: LicensePayload,
    pub status: LicenseRuntimeStatus,
}

pub fn evaluate_license(
    public_key: &PublicKey,
    license_bytes: &[u8],
    device_hash: &[u8; 32],
    now: i64,
) -> Result<LicenseEvaluation, CommandError> {
    let payload =
        license_core::verify_license(public_key, license_bytes).map_err(map_core_error)?;
    let status = runtime_state(&payload, device_hash, now)?;
    Ok(LicenseEvaluation { payload, status })
}

pub fn runtime_state(
    payload: &LicensePayload,
    device_hash: &[u8; 32],
    now: i64,
) -> Result<LicenseRuntimeStatus, CommandError> {
    if payload.ver < PAYLOAD_VERSION_CURRENT {
        return Err(CommandError::new(
            "LegacyUnsupported",
            format!("legacy license version {}", payload.ver),
        ));
    }

    let app_id = payload.app_id.trim();
    if app_id != DEFAULT_APP_ID {
        return Err(CommandError::new(
            "AppIdMismatch",
            format!("license targets app_id={app_id}"),
        ));
    }

    if payload.max_clock_skew == 0 {
        return Err(CommandError::parse("max_clock_skew must be > 0"));
    }

    let not_before = to_i64(payload.not_before, "not_before")?;
    let not_after = to_i64(payload.not_after, "not_after")?;
    let skew = i64::from(payload.max_clock_skew);

    if payload.allowed_device_hash != *device_hash {
        return Ok(LicenseRuntimeStatus::DeviceMismatch);
    }

    let now_plus_skew = now
        .checked_add(skew)
        .ok_or_else(|| CommandError::parse("clock skew overflow"))?;
    if now_plus_skew < not_before {
        return Ok(LicenseRuntimeStatus::NotYetValid);
    }

    let now_minus_skew = now
        .checked_sub(skew)
        .ok_or_else(|| CommandError::parse("clock skew underflow"))?;
    if now_minus_skew > not_after {
        return Ok(LicenseRuntimeStatus::Expired);
    }

    Ok(LicenseRuntimeStatus::Active)
}

fn to_i64(value: u64, label: &str) -> Result<i64, CommandError> {
    i64::try_from(value).map_err(|_| {
        CommandError::parse(format!(
            "{label} excede el rango soportado por la aplicación"
        ))
    })
}

fn map_core_error(err: LicenseError) -> CommandError {
    match err {
        LicenseError::Format(e) => CommandError::parse(e.to_string()),
        LicenseError::Signature(e) => CommandError::new("SignatureFailed", e.to_string()),
        LicenseError::Validation(e) => match e.kind() {
            ValidationErrorKind::LegacyVersion => {
                CommandError::new("LegacyUnsupported", e.to_string())
            }
            ValidationErrorKind::AppIdMismatch => CommandError::new("AppIdMismatch", e.to_string()),
            _ => CommandError::parse(e.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signer};
    use std::collections::BTreeMap;

    fn sample_payload(device_hash: [u8; 32]) -> LicensePayload {
        LicensePayload {
            ver: PAYLOAD_VERSION_CURRENT,
            key_id: 1,
            serial: 42,
            license_id: "LIC-TEST-001".to_string(),
            issued_at: 1_700_000_000,
            not_before: 1_700_000_000,
            not_after: 1_800_000_000,
            max_clock_skew: 300,
            allowed_device_hash: device_hash,
            plan: "monthly".to_string(),
            features: BTreeMap::new(),
            policy: BTreeMap::new(),
            customer_name: Some("Demo".to_string()),
            app_id: DEFAULT_APP_ID.to_string(),
        }
    }

    fn encode_license_bytes(payload: &LicensePayload, keypair: &Keypair) -> Vec<u8> {
        let payload_bytes = serde_cbor::to_vec(payload).unwrap();
        let mut bytes = Vec::with_capacity(4 + payload_bytes.len() + 64);
        bytes.extend_from_slice(&(payload_bytes.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&payload_bytes);
        let signature = keypair.sign(&payload_bytes);
        bytes.extend_from_slice(signature.as_ref());
        bytes
    }

    fn test_keypair() -> Keypair {
        let secret =
            SecretKey::from_bytes(&[0x11; 32]).expect("secret key should be exactly 32 bytes");
        let public: PublicKey = (&secret).into();
        Keypair { secret, public }
    }

    #[test]
    fn valid_license_classifies_as_active() {
        let keypair = test_keypair();
        let device_hash = [7u8; 32];
        let payload = sample_payload(device_hash);
        let bytes = encode_license_bytes(&payload, &keypair);
        let now = payload.not_before as i64 + 10;

        let result = evaluate_license(&keypair.public, &bytes, &device_hash, now).unwrap();
        assert_eq!(result.status, LicenseRuntimeStatus::Active);
    }

    #[test]
    fn expired_license_is_rejected() {
        let keypair = test_keypair();
        let device_hash = [3u8; 32];
        let payload = sample_payload(device_hash);
        let bytes = encode_license_bytes(&payload, &keypair);
        let now = payload.not_after as i64 + payload.max_clock_skew as i64 + 10;

        let evaluation = evaluate_license(&keypair.public, &bytes, &device_hash, now).unwrap();
        assert_eq!(evaluation.status, LicenseRuntimeStatus::Expired);
    }

    #[test]
    fn device_mismatch_is_rejected() {
        let keypair = test_keypair();
        let payload = sample_payload([1u8; 32]);
        let bytes = encode_license_bytes(&payload, &keypair);
        let now = payload.not_before as i64 + 5;

        let evaluation = evaluate_license(&keypair.public, &bytes, &[2u8; 32], now).unwrap();
        assert_eq!(evaluation.status, LicenseRuntimeStatus::DeviceMismatch);
    }
}
