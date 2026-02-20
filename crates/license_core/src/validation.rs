use crate::{format::LicensePayload, DEFAULT_APP_ID, PAYLOAD_VERSION_CURRENT, VALID_PLANS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationErrorKind {
    LegacyVersion,
    AppIdMismatch,
    InvalidPlan,
    InvalidWindow,
    ZeroClockSkew,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ValidationError {
    kind: ValidationErrorKind,
    message: String,
}

impl ValidationError {
    pub fn new(kind: ValidationErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> ValidationErrorKind {
        self.kind
    }
}

pub fn validate_payload(payload: &LicensePayload) -> Result<(), ValidationError> {
    if payload.ver < PAYLOAD_VERSION_CURRENT {
        return Err(ValidationError::new(
            ValidationErrorKind::LegacyVersion,
            format!("legacy license version {}", payload.ver),
        ));
    }

    let app_id = payload.app_id.trim();
    if !app_id.eq(DEFAULT_APP_ID) {
        return Err(ValidationError::new(
            ValidationErrorKind::AppIdMismatch,
            format!("app_id mismatch: {app_id}"),
        ));
    }

    let plan = payload.plan.trim().to_ascii_lowercase();
    if !VALID_PLANS.iter().any(|allowed| plan == *allowed) {
        return Err(ValidationError::new(
            ValidationErrorKind::InvalidPlan,
            format!("invalid plan: {}", payload.plan),
        ));
    }

    if payload.not_after <= payload.not_before {
        return Err(ValidationError::new(
            ValidationErrorKind::InvalidWindow,
            "not_after must be greater than not_before",
        ));
    }

    if payload.max_clock_skew == 0 {
        return Err(ValidationError::new(
            ValidationErrorKind::ZeroClockSkew,
            "max_clock_skew must be > 0",
        ));
    }

    Ok(())
}
