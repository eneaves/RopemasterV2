use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const LENGTH_PREFIX_LEN: usize = 4;
pub const SIGNATURE_LEN: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicensePayload {
    pub ver: u32,
    pub key_id: u32,
    pub serial: u64,
    pub license_id: String,
    pub issued_at: u64,
    pub not_before: u64,
    pub not_after: u64,
    pub max_clock_skew: u32,
    pub allowed_device_hash: [u8; 32],
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub features: BTreeMap<String, bool>,
    #[serde(default)]
    pub policy: BTreeMap<String, i64>,
    #[serde(default)]
    pub customer_name: Option<String>,
    #[serde(default)]
    pub app_id: String,
}

#[derive(Debug, Clone)]
pub struct ParsedLicense {
    pub payload: LicensePayload,
    pub payload_bytes: Vec<u8>,
    pub signature: [u8; SIGNATURE_LEN],
}

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("license file too small")]
    TooSmall,
    #[error("license payload length mismatch")]
    LengthMismatch,
    #[error("license payload decode error: {0}")]
    PayloadDecode(String),
}

pub fn parse_license_bytes(bytes: &[u8]) -> Result<ParsedLicense, FormatError> {
    if bytes.len() < LENGTH_PREFIX_LEN + SIGNATURE_LEN + 1 {
        return Err(FormatError::TooSmall);
    }

    let mut len_buf = [0u8; LENGTH_PREFIX_LEN];
    len_buf.copy_from_slice(&bytes[..LENGTH_PREFIX_LEN]);
    let payload_len = u32::from_be_bytes(len_buf) as usize;

    let total_needed = LENGTH_PREFIX_LEN + payload_len + SIGNATURE_LEN;
    if bytes.len() != total_needed {
        return Err(FormatError::LengthMismatch);
    }

    let payload_bytes = bytes[LENGTH_PREFIX_LEN..LENGTH_PREFIX_LEN + payload_len].to_vec();
    let mut signature = [0u8; SIGNATURE_LEN];
    signature.copy_from_slice(&bytes[LENGTH_PREFIX_LEN + payload_len..total_needed]);

    let payload: LicensePayload = serde_cbor::from_slice(&payload_bytes)
        .map_err(|err| FormatError::PayloadDecode(err.to_string()))?;

    Ok(ParsedLicense {
        payload,
        payload_bytes,
        signature,
    })
}
