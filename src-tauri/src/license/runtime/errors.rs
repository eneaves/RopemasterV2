#![allow(dead_code)]

use crate::license::CommandError;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("io error: {0}")]
    Io(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("other: {0}")]
    Other(String),
}

impl RuntimeError {
    pub fn io(err: impl Into<String>) -> Self {
        Self::Io(err.into())
    }

    pub fn ser(err: impl Into<String>) -> Self {
        Self::Serialization(err.into())
    }
}

impl From<RuntimeError> for CommandError {
    fn from(err: RuntimeError) -> Self {
        match err {
            RuntimeError::Io(msg) => CommandError::io(msg),
            RuntimeError::Serialization(msg) => CommandError::parse(msg),
            RuntimeError::Other(msg) => CommandError::new("RuntimeError", msg),
        }
    }
}
