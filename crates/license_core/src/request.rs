use serde::{Deserialize, Serialize};

use crate::{DEFAULT_APP_ID, VALID_PLANS};

pub const REQUEST_VER: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicenseRequest {
    pub ver: u32,
    pub app_id: String,
    pub plan: String,
    pub device_hash: [u8; 32],
    pub created_at: u64,
    pub nonce: [u8; 16],
    pub customer_name_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestErrorKind {
    InvalidVersion,
    AppIdMismatch,
    InvalidPlan,
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

fn normalize_request(req: &mut LicenseRequest) {
    req.app_id = req.app_id.trim().to_string();
    req.plan = normalize_plan(&req.plan);
    if let Some(customer_name) = req.customer_name_hint.take() {
        let trimmed = normalize_customer_name(&customer_name);
        req.customer_name_hint = if trimmed.is_empty() {
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

    if req.app_id != DEFAULT_APP_ID {
        return Err(RequestError::new(
            RequestErrorKind::AppIdMismatch,
            format!("app_id mismatch: {}", req.app_id),
        ));
    }

    if !VALID_PLANS.iter().any(|plan| req.plan == *plan) {
        return Err(RequestError::new(
            RequestErrorKind::InvalidPlan,
            format!("invalid plan: {}", req.plan),
        ));
    }

    Ok(())
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
