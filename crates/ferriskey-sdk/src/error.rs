//! Error types for the FerrisKey SDK transport layer.

use serde_json::Value;
use thiserror::Error;

/// Errors raised by the transport implementation.
#[derive(Debug, Error)]
pub enum TransportError {
    /// The request method could not be converted into an HTTP verb.
    #[error("invalid HTTP method {method}")]
    InvalidMethod {
        /// Method string provided by the SDK request.
        method: String,
    },
    /// The request could not be sent by the underlying HTTP client.
    #[error("HTTP transport failure: {0}")]
    Http(#[from] hpx::Error),
}

/// Errors raised by the SDK execution layer.
#[derive(Debug, Error)]
pub enum SdkError {
    /// A documented API error payload was returned by the server.
    #[error("operation {operation_id} returned API error status {status}")]
    ApiResponse {
        /// Decoded JSON body when the documented response payload is JSON.
        body: Option<Value>,
        /// Operation identifier used for the request.
        operation_id: String,
        /// Schema name documented for the matched response payload.
        schema_name: Option<&'static str>,
        /// HTTP status code returned by the server.
        status: u16,
    },
    /// A secured request was attempted without configured credentials.
    #[error("request requires bearer authentication but no bearer token is configured")]
    MissingAuth,
    /// A required path, query, or header parameter was not provided.
    #[error("operation {operation_id} is missing required {location} parameter {name}")]
    MissingParameter {
        /// Parameter location in the HTTP request.
        location: &'static str,
        /// Parameter name.
        name: String,
        /// Operation identifier used for the request.
        operation_id: String,
    },
    /// A required request body was omitted.
    #[error("operation {operation_id} requires a request body")]
    MissingRequestBody {
        /// Operation identifier used for the request.
        operation_id: String,
    },
    /// The response status did not match what the caller expected.
    #[error("unexpected response status: expected {expected}, got {actual}")]
    UnexpectedStatus {
        /// Expected HTTP status code.
        expected: u16,
        /// Actual HTTP status code.
        actual: u16,
    },
    /// The response body did not match the expected format.
    #[error("failed to decode response body: {0}")]
    Decode(#[from] serde_json::Error),
    /// The configured base URL or request path could not be resolved.
    #[error("failed to resolve request URL from base {base_url} and path {path}")]
    InvalidUrl {
        /// Base URL configured on the SDK.
        base_url: String,
        /// Path provided by the SDK request.
        path: String,
    },
    /// The operation path template still contained unresolved placeholders after encoding.
    #[error(
        "operation {operation_id} has unresolved placeholders in path template {path_template}"
    )]
    InvalidPathTemplate {
        /// Operation identifier used for the request.
        operation_id: String,
        /// Path template that could not be fully encoded.
        path_template: String,
    },
    /// No generated descriptor matched the requested operation ID.
    #[error("unknown FerrisKey operation: {operation_id}")]
    UnknownOperation {
        /// Operation identifier requested by the caller.
        operation_id: String,
    },
    /// The underlying transport failed.
    #[error(transparent)]
    Transport(#[from] TransportError),
}
