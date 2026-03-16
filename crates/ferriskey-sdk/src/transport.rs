//! Transport seam and HTTP request/response types for the FerrisKey SDK.

use std::{collections::BTreeMap, future::Future, pin::Pin};

use crate::error::TransportError;

/// Canonical SDK request passed to a transport implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SdkRequest {
    /// Raw request body bytes.
    pub body: Option<Vec<u8>>,
    /// Whether the request requires bearer authentication.
    pub requires_auth: bool,
    /// Header values to attach to the outgoing request.
    pub headers: BTreeMap<String, String>,
    /// HTTP method to execute.
    pub method: String,
    /// Absolute or relative path for the request.
    pub path: String,
}

impl SdkRequest {
    /// Construct a request with an HTTP method and request path.
    #[must_use]
    pub fn new(method: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            body: None,
            requires_auth: false,
            headers: BTreeMap::new(),
            method: method.into(),
            path: path.into(),
        }
    }
}

/// Canonical SDK response returned by a transport implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SdkResponse {
    /// Response body bytes.
    pub body: Vec<u8>,
    /// Response headers represented as UTF-8 strings when possible.
    pub headers: BTreeMap<String, String>,
    /// HTTP status code.
    pub status: u16,
}

/// Async transport contract used by the SDK.
pub trait Transport: Send + Sync {
    /// Send a canonical SDK request and return the raw HTTP response.
    fn send(
        &self,
        request: SdkRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SdkResponse, TransportError>> + Send + '_>>;
}

/// Primary HTTP transport adapter backed by `hpx`.
#[derive(Clone)]
pub struct HpxTransport {
    client: hpx::Client,
}

impl std::fmt::Debug for HpxTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("HpxTransport").finish_non_exhaustive()
    }
}

impl Default for HpxTransport {
    fn default() -> Self {
        Self::new(hpx::Client::new())
    }
}

impl HpxTransport {
    /// Build a transport from an `hpx` client instance.
    #[must_use]
    pub const fn new(client: hpx::Client) -> Self {
        Self { client }
    }
}

impl Transport for HpxTransport {
    fn send(
        &self,
        request: SdkRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SdkResponse, TransportError>> + Send + '_>> {
        Box::pin(async move {
            let method = hpx::Method::from_bytes(request.method.as_bytes())
                .map_err(|_| TransportError::InvalidMethod { method: request.method.clone() })?;
            let mut builder = self.client.request(method, request.path);

            for (name, value) in request.headers {
                builder = builder.header(name, value);
            }

            if let Some(body) = request.body {
                builder = builder.body(body);
            }

            let response = builder.send().await?;
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value.to_str().ok().map(|value| (name.to_string(), value.to_string()))
                })
                .collect::<BTreeMap<_, _>>();
            let body = response.bytes().await?.to_vec();

            Ok(SdkResponse { body, headers, status })
        })
    }
}
