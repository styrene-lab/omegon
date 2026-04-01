//! Extension error types — safety-first distinction between fatal and recoverable errors.

use serde::{Deserialize, Serialize};
use std::fmt;

/// RPC-level error codes. Matched against [`rpc::ErrorCode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ErrorCode {
    /// Method name not recognized.
    MethodNotFound,
    /// Invalid parameters for method.
    InvalidParams,
    /// Extension encountered an internal error (non-fatal).
    InternalError,
    /// Manifest validation failed (fatal — caught at install time).
    ManifestError,
    /// Version incompatibility (fatal — caught at install time).
    VersionMismatch,
    /// Timeout waiting for response.
    Timeout,
    /// RPC parse error (malformed JSON).
    ParseError,
    /// Extension was asked to do something outside its capability.
    NotImplemented,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", self));
        write!(f, "{}", s)
    }
}

impl From<ErrorCode> for String {
    fn from(code: ErrorCode) -> Self {
        code.to_string()
    }
}

impl std::error::Error for ErrorCode {}

/// Extension result type. Always propagates the error code for RPC responses.
#[derive(Debug)]
pub struct Error {
    code: ErrorCode,
    message: String,
    /// Install-time errors (caught before extension runs).
    pub is_install_time: bool,
}

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            is_install_time: false,
        }
    }

    /// Mark this error as discovered during installation/validation.
    /// These errors prevent the extension from running at all.
    pub fn at_install_time(mut self) -> Self {
        self.is_install_time = true;
        self
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(ErrorCode::MethodNotFound, format!("method '{}' not found", method))
    }

    pub fn invalid_params(reason: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidParams, reason)
    }

    pub fn internal_error(reason: impl Into<String>) -> Self {
        Self::new(ErrorCode::InternalError, reason)
    }

    pub fn version_mismatch(expected: &str, actual: &str) -> Self {
        Self::new(
            ErrorCode::VersionMismatch,
            format!("version mismatch: expected {}, got {}", expected, actual),
        )
        .at_install_time()
    }

    pub fn manifest_error(reason: impl Into<String>) -> Self {
        Self::new(ErrorCode::ManifestError, reason).at_install_time()
    }

    pub fn timeout() -> Self {
        Self::new(ErrorCode::Timeout, "RPC call timed out")
    }

    pub fn parse_error(reason: impl Into<String>) -> Self {
        Self::new(ErrorCode::ParseError, reason)
    }

    pub fn not_implemented(feature: &str) -> Self {
        Self::new(
            ErrorCode::NotImplemented,
            format!("feature '{}' not implemented", feature),
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

impl From<ErrorCode> for Error {
    fn from(code: ErrorCode) -> Self {
        Self::new(code, code.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::new(ErrorCode::ParseError, e.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::new(ErrorCode::InternalError, e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
