pub mod crypto;
pub mod format;
pub mod request;
pub mod validation;

pub use ed25519_dalek::PublicKey;
pub use format::{LicensePayload, ParsedLicense};
pub use request::{LicenseRequest, RequestError, RequestErrorKind, REQUEST_VER};
pub use validation::{ValidationError, ValidationErrorKind};

use crypto::verify_signature;
use format::parse_license_bytes;
use validation::validate_payload;

pub const DEFAULT_APP_ID: &str = "roping_manager";
pub const VALID_PLANS: &[&str] = &["monthly", "yearly", "per_event"];
pub const PAYLOAD_VERSION_CURRENT: u32 = 4;

#[derive(Debug, thiserror::Error)]
pub enum LicenseError {
    #[error("format error: {0}")]
    Format(#[from] format::FormatError),
    #[error("signature error: {0}")]
    Signature(#[from] crypto::SignatureError),
    #[error("validation error: {0}")]
    Validation(#[from] validation::ValidationError),
}

pub fn verify_license(
    public_key: &PublicKey,
    license_bytes: &[u8],
) -> Result<LicensePayload, LicenseError> {
    let parsed = parse_license_bytes(license_bytes)?;
    verify_signature(public_key, &parsed.payload_bytes, &parsed.signature)?;
    validate_payload(&parsed.payload)?;
    Ok(parsed.payload)
}
