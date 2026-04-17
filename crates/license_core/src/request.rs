//! Legacy CBOR request contract kept only for backward-compatible tooling.
//! Modern request generation must use `shared_core`.

use serde::{Deserialize, Serialize};

use crate::{DEFAULT_APP_ID, VALID_PLANS};

pub const REQUEST_VER: u32 = 2;
pub const REQUEST_AUTH_VER: u32 = 1;
pub const REQUEST_SIGNATURE_LEN: usize = 64;
pub const REQUEST_ID_LEN: usize = 16;
pub const INSTALLATION_PUBKEY_LEN: usize = 32;
pub const BINDING_HASH_LEN: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LicenseRequest {
    pub ver: u32,
    pub payload: LicenseRequestPayload,
    pub auth: RequestAuth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LicenseRequestPayload {
    pub request_id: [u8; REQUEST_ID_LEN],
    pub app_id: String,
    pub plan: String,
    pub installation_id: String,
    pub installation_pubkey: [u8; INSTALLATION_PUBKEY_LEN],
    pub binding_hash: [u8; BINDING_HASH_LEN],
    pub fingerprint: RequestFingerprint,
    pub created_at: u64,
    pub customer_name_hint: Option<String>,
    pub key_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RequestAuth {
    pub ver: u32,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RequestFingerprint {
    pub schema: u32,
    pub anchors: RequestFingerprintAnchors,
    pub observations: RequestFingerprintObservations,
}

impl RequestFingerprint {
    pub const SCHEMA: u32 = 1;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RequestFingerprintAnchors {
    pub platform: String,
    pub arch: String,
    pub hostname: String,
    pub username: String,
    pub distro: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RequestFingerprintObservations {
    pub timezone: Option<String>,
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestErrorKind {
    InvalidVersion,
    InvalidAuthVersion,
    AppIdMismatch,
    InvalidPlan,
    InvalidIdentity,
    InvalidFingerprint,
    Serialization,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct RequestError {
    kind: RequestErrorKind,
    message: String,
}

impl RequestError {
    pub fn new(kind: RequestErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> RequestErrorKind {
        self.kind
    }
}

pub fn parse_request_bytes(bytes: &[u8]) -> Result<LicenseRequest, RequestError> {
    let mut req: LicenseRequest = serde_cbor::from_slice(bytes)
        .map_err(|err| RequestError::new(RequestErrorKind::Serialization, err.to_string()))?;
    normalize_request(&mut req);
    validate_request(&req)?;
    Ok(req)
}

pub fn request_to_bytes(req: &LicenseRequest) -> Result<Vec<u8>, RequestError> {
    let mut normalized = req.clone();
    normalize_request(&mut normalized);
    validate_request(&normalized)?;
    serde_cbor::to_vec(&normalized)
        .map_err(|err| RequestError::new(RequestErrorKind::Serialization, err.to_string()))
}

pub fn payload_signing_bytes(payload: &LicenseRequestPayload) -> Result<Vec<u8>, RequestError> {
    let mut normalized = payload.clone();
    normalize_payload(&mut normalized);
    serde_cbor::to_vec(&normalized)
        .map_err(|err| RequestError::new(RequestErrorKind::Serialization, err.to_string()))
}

fn normalize_request(req: &mut LicenseRequest) {
    normalize_payload(&mut req.payload);
}

fn normalize_payload(payload: &mut LicenseRequestPayload) {
    payload.app_id = payload.app_id.trim().to_string();
    payload.plan = normalize_plan(&payload.plan);
    payload.installation_id = payload.installation_id.trim().to_string();

    if let Some(customer_name) = payload.customer_name_hint.take() {
        let trimmed = normalize_customer_name(&customer_name);
        payload.customer_name_hint = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
    }
}

fn validate_request(req: &LicenseRequest) -> Result<(), RequestError> {
    if req.ver != REQUEST_VER {
        return Err(RequestError::new(
            RequestErrorKind::InvalidVersion,
            format!("request version {} unsupported", req.ver),
        ));
    }

    if req.auth.ver != REQUEST_AUTH_VER {
        return Err(RequestError::new(
            RequestErrorKind::InvalidAuthVersion,
            format!("request auth version {} unsupported", req.auth.ver),
        ));
    }

    if req.auth.signature.len() != REQUEST_SIGNATURE_LEN {
        return Err(RequestError::new(
            RequestErrorKind::InvalidIdentity,
            "signature length invalid",
        ));
    }

    let payload = &req.payload;
    if payload.app_id != DEFAULT_APP_ID {
        return Err(RequestError::new(
            RequestErrorKind::AppIdMismatch,
            format!("app_id mismatch: {}", payload.app_id),
        ));
    }

    if !VALID_PLANS.iter().any(|plan| payload.plan == *plan) {
        return Err(RequestError::new(
            RequestErrorKind::InvalidPlan,
            format!("invalid plan: {}", payload.plan),
        ));
    }

    if payload.installation_id.is_empty() {
        return Err(RequestError::new(
            RequestErrorKind::InvalidIdentity,
            "installation_id missing",
        ));
    }

    if payload.key_id == 0 {
        return Err(RequestError::new(
            RequestErrorKind::InvalidIdentity,
            "installation key_id must be > 0",
        ));
    }

    if is_zero(&payload.installation_pubkey) {
        return Err(RequestError::new(
            RequestErrorKind::InvalidIdentity,
            "installation_pubkey invalid",
        ));
    }

    if is_zero(&payload.binding_hash) {
        return Err(RequestError::new(
            RequestErrorKind::InvalidIdentity,
            "binding_hash invalid",
        ));
    }

    if is_zero(&payload.request_id) {
        return Err(RequestError::new(
            RequestErrorKind::InvalidIdentity,
            "request_id invalid",
        ));
    }

    validate_fingerprint(&payload.fingerprint)?;

    Ok(())
}

fn validate_fingerprint(fp: &RequestFingerprint) -> Result<(), RequestError> {
    if fp.schema != RequestFingerprint::SCHEMA {
        return Err(RequestError::new(
            RequestErrorKind::InvalidFingerprint,
            format!("fingerprint schema {} unsupported", fp.schema),
        ));
    }

    let anchors = &fp.anchors;
    if anchors.platform.trim().is_empty()
        || anchors.arch.trim().is_empty()
        || anchors.hostname.trim().is_empty()
        || anchors.username.trim().is_empty()
        || anchors.distro.trim().is_empty()
    {
        return Err(RequestError::new(
            RequestErrorKind::InvalidFingerprint,
            "fingerprint anchors incomplete",
        ));
    }

    Ok(())
}

fn is_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|b| *b == 0)
}

fn normalize_plan(plan: &str) -> String {
    plan.trim().to_ascii_lowercase()
}

fn normalize_customer_name(input: &str) -> String {
    let mut collapsed = String::with_capacity(input.len());
    let mut prev_space = true;
    for ch in input.trim().chars() {
        if ch.is_whitespace() {
            if !prev_space {
                collapsed.push(' ');
            }
            prev_space = true;
        } else {
            collapsed.push(ch);
            prev_space = false;
        }
    }
    collapsed
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{SecretKey, Signer};

    fn sample_payload() -> LicenseRequestPayload {
        LicenseRequestPayload {
            request_id: [0xAB; REQUEST_ID_LEN],
            app_id: DEFAULT_APP_ID.to_string(),
            plan: "Monthly".into(),
            installation_id: "install-123".into(),
            installation_pubkey: [1u8; INSTALLATION_PUBKEY_LEN],
            binding_hash: [2u8; BINDING_HASH_LEN],
            fingerprint: RequestFingerprint {
                schema: RequestFingerprint::SCHEMA,
                anchors: RequestFingerprintAnchors {
                    platform: "test-os".into(),
                    arch: "x86_64".into(),
                    hostname: "host".into(),
                    username: "user".into(),
                    distro: "distro".into(),
                },
                observations: RequestFingerprintObservations::default(),
            },
            created_at: 1_700_000_000,
            customer_name_hint: Some("  Cliente   QA  ".into()),
            key_id: 1,
        }
    }

    fn fixed_keypair() -> ed25519_dalek::Keypair {
        let secret = SecretKey::from_bytes(&[7u8; 32]).unwrap();
        let public = ed25519_dalek::PublicKey::from(&secret);
        ed25519_dalek::Keypair { secret, public }
    }

    #[test]
    fn request_roundtrip_serializes_and_parses() {
        let keypair = fixed_keypair();
        let mut payload = sample_payload();
        payload.plan = "monthly".into();
        let signing = payload_signing_bytes(&payload).expect("signing bytes");
        let signature = keypair.sign(&signing).to_bytes();
        let request = LicenseRequest {
            ver: REQUEST_VER,
            payload,
            auth: RequestAuth {
                ver: REQUEST_AUTH_VER,
                signature: signature.to_vec(),
            },
        };

        let bytes = request_to_bytes(&request).expect("to bytes");
        let parsed = parse_request_bytes(&bytes).expect("parse request");
        assert_eq!(parsed.payload.installation_id, "install-123");
        assert_eq!(parsed.payload.plan, "monthly");
        assert_eq!(parsed.payload.customer_name_hint, Some("Cliente QA".into()));
        assert_eq!(parsed.auth.ver, REQUEST_AUTH_VER);
    }

    #[test]
    fn signing_bytes_are_canonical() {
        let mut payload = sample_payload();
        payload.plan = "   MONTHLY   ".into();
        let bytes_a = payload_signing_bytes(&payload).expect("bytes A");

        payload.plan = "monthly".into();
        let bytes_b = payload_signing_bytes(&payload).expect("bytes B");
        assert_eq!(bytes_a, bytes_b);
    }

    #[test]
    fn invalid_plan_is_rejected() {
        let mut payload = sample_payload();
        payload.plan = "invalid".into();
        let signing = payload_signing_bytes(&payload).expect("sign bytes");
        let keypair = fixed_keypair();
        let signature = keypair.sign(&signing).to_bytes();
        let request = LicenseRequest {
            ver: REQUEST_VER,
            payload,
            auth: RequestAuth {
                ver: REQUEST_AUTH_VER,
                signature: signature.to_vec(),
            },
        };
        let err = request_to_bytes(&request).unwrap_err();
        assert_eq!(err.kind(), RequestErrorKind::InvalidPlan);
    }

    #[test]
    fn default_app_id_matches_shared_core() {
        assert_eq!(DEFAULT_APP_ID, shared_core::DEFAULT_APP_ID);
        assert_eq!(DEFAULT_APP_ID, "roping_manager");
    }
}
