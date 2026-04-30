// Authoritative wire contract — defined once in shared_core to prevent drift
// between the app parser and the generator issuer.
#[cfg(test)]
use ed25519_dalek::{PublicKey, Signature};

#[cfg(test)]
pub(crate) const FORMAT_VERSION: u16 = shared_core::licgen_envelope::FORMAT_VERSION;
#[cfg(test)]
pub(crate) const LICENSE_MAGIC: &[u8; 6] = shared_core::licgen_envelope::LICENSE_MAGIC;
#[cfg(test)]
pub(crate) const LICENSE_VERSION: u16 = shared_core::licgen_envelope::LICENSE_VERSION;
#[cfg(test)]
pub(crate) type ModernLicensePayload = shared_core::ModernLicensePayload;
pub(crate) const DEFAULT_MAX_CLOCK_SKEW_SECS: i64 = 300;

#[cfg(test)]
#[derive(Debug, thiserror::Error)]
pub(crate) enum ModernLicenseError {
    #[error("invalid modern license format: {0}")]
    InvalidFormat(&'static str),
    #[error("unsupported modern format_version {received}")]
    UnsupportedFormatVersion { received: u16 },
    #[error("unsupported modern license_version {received}")]
    UnsupportedLicenseVersion { received: u16 },
    #[error("modern license signature is invalid")]
    InvalidSignature,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct ParsedModernLicense {
    pub format_version: u16,
    pub payload_bytes: Vec<u8>,
    pub payload: ModernLicensePayload,
    pub signature: Vec<u8>,
}

#[cfg(test)]
impl ParsedModernLicense {
    pub fn metadata_string(&self, key: &str) -> Option<&str> {
        self.payload.metadata_string(key)
    }

    pub fn verify_signature(&self, public_key: &PublicKey) -> Result<(), ModernLicenseError> {
        if self.signature.len() != 64 {
            return Err(ModernLicenseError::InvalidFormat(
                "signature must be 64 bytes for ed25519",
            ));
        }
        let sig_bytes: [u8; 64] = self
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| ModernLicenseError::InvalidFormat("signature length mismatch"))?;
        let signature =
            Signature::from_bytes(&sig_bytes).map_err(|_| ModernLicenseError::InvalidSignature)?;
        public_key
            .verify_strict(&self.payload_bytes, &signature)
            .map_err(|_| ModernLicenseError::InvalidSignature)
    }
}

#[cfg(test)]
pub(crate) fn parse_license(bytes: &[u8]) -> Result<ParsedModernLicense, ModernLicenseError> {
    let envelope = shared_core::parse_signed_license_blob(bytes).map_err(map_envelope_error)?;
    let payload_bytes = envelope.payload;
    let payload: ModernLicensePayload = serde_json::from_slice(&payload_bytes)
        .map_err(|_| ModernLicenseError::InvalidFormat("payload is not valid JSON"))?;
    if payload.license_version != LICENSE_VERSION {
        return Err(ModernLicenseError::UnsupportedLicenseVersion {
            received: payload.license_version,
        });
    }
    payload
        .validate()
        .map_err(|err| ModernLicenseError::InvalidFormat(err.message()))?;

    Ok(ParsedModernLicense {
        format_version: envelope.format_version,
        payload_bytes,
        payload,
        signature: envelope.signature,
    })
}

#[cfg(test)]
fn map_envelope_error(err: shared_core::LicenseEnvelopeError) -> ModernLicenseError {
    match err {
        shared_core::LicenseEnvelopeError::BlobTooShort => {
            ModernLicenseError::InvalidFormat("file shorter than modern header")
        }
        shared_core::LicenseEnvelopeError::InvalidMagic { .. } => {
            ModernLicenseError::InvalidFormat("invalid modern magic")
        }
        shared_core::LicenseEnvelopeError::UnsupportedFormatVersion { received } => {
            ModernLicenseError::UnsupportedFormatVersion { received }
        }
        shared_core::LicenseEnvelopeError::PayloadLengthOverflow => {
            ModernLicenseError::InvalidFormat("payload length overflow")
        }
        shared_core::LicenseEnvelopeError::TruncatedPayload
        | shared_core::LicenseEnvelopeError::MissingSignatureLength => {
            ModernLicenseError::InvalidFormat("payload truncated or signature length missing")
        }
        shared_core::LicenseEnvelopeError::SignatureLengthOverflow => {
            ModernLicenseError::InvalidFormat("signature length overflow")
        }
        shared_core::LicenseEnvelopeError::TruncatedSignature
        | shared_core::LicenseEnvelopeError::TrailingBytes => {
            ModernLicenseError::InvalidFormat("modern license contains truncated or trailing bytes")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_license;
    use chrono::Duration;
    use licgen_core::{format::decode_signed_license, LicensePayloadV5};
    use uuid::Uuid;

    #[test]
    fn app_and_generator_parse_same_modern_payload() {
        let mut payload = LicensePayloadV5::default();
        payload.installation.installation_id = Uuid::new_v4();
        payload
            .installation
            .device_fingerprint
            .replace_with_legacy_hash(&"ab".repeat(32));
        payload.installation.device_fingerprint.platform = "linux".into();
        payload.device_fingerprint_v2 = payload.installation.device_fingerprint.clone();
        payload.expires_at = payload.issued_at + Duration::days(30);
        payload.security_policy.key_id = Some("primary".into());
        payload.security_policy.key_version = Some("v1".into());
        payload.security_policy.revocation_epoch = Some(7);
        payload.metadata = serde_json::json!({
            "app_id": "roping_manager",
            "plan": "monthly",
            "customer_name_hint": "ACME Ranch"
        });

        let bytes = licgen_core::format::encode_signed_license(&payload, &vec![0u8; 64])
            .expect("encode signed modern license");
        let parsed_app = parse_license(&bytes).expect("app parses modern license");
        let (parsed_generator, _) =
            decode_signed_license(&bytes).expect("generator parses modern license");

        assert_eq!(parsed_app.payload.license_id, parsed_generator.license_id);
        assert_eq!(
            parsed_app.payload.installation.installation_id,
            parsed_generator.installation.installation_id
        );
        assert_eq!(
            parsed_app.payload.installation.first_seen_at,
            Some(parsed_generator.installation.first_seen_at)
        );
        assert_eq!(
            parsed_app.payload.offline_policy.max_offline_days,
            parsed_generator.offline_policy.max_offline_days
        );
        assert_eq!(
            parsed_app.payload.offline_policy.lease_required,
            parsed_generator.offline_policy.lease_required
        );
        assert_eq!(
            parsed_app.payload.security_policy.key_id,
            parsed_generator.security_policy.key_id
        );
        assert_eq!(
            parsed_app.payload.security_policy.key_version,
            parsed_generator.security_policy.key_version
        );
        assert_eq!(
            parsed_app.payload.security_policy.revocation_epoch,
            parsed_generator.security_policy.revocation_epoch
        );
        assert_eq!(
            parsed_app.payload.device_fingerprint_v2.hardware_hash,
            parsed_generator.device_fingerprint_v2.hardware_hash
        );
        assert_eq!(
            parsed_app.payload.metadata_string("app_id"),
            parsed_generator
                .metadata
                .get("app_id")
                .and_then(|value| value.as_str())
        );
        assert_eq!(
            parsed_app.payload.metadata_string("plan"),
            parsed_generator
                .metadata
                .get("plan")
                .and_then(|value| value.as_str())
        );
    }
}
