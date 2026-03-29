//! Transport seam and HTTP request/response types for the FerrisKey SDK.
//!
//! ## Design Philosophy
//!
//! This module leverages `tower::Service` as the foundational abstraction for transport,
//! enabling composition of middleware layers (retry, timeout, rate-limiting) without
//! custom framework abstractions. The `Transport` trait is implemented as a blanket
//! implementation over any `tower::Service<SdkRequest>`, providing maximum flexibility.

use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use tower::{Service, ServiceExt};

use crate::error::TransportError;

/// Canonical SDK request passed to a transport implementation.
///
/// ## Type Safety
///
/// The builder pattern ensures required fields are set at compile time.
/// Use [`SdkRequest::builder()`] to construct requests with guaranteed validity.
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
    /// Create a typed builder for constructing requests.
    ///
    /// # Examples
    ///
    /// ```
    /// use ferriskey_sdk::SdkRequest;
    ///
    /// let request = SdkRequest::builder("GET", "/api/users")
    ///     .header("accept", "application/json")
    ///     .auth_required(true)
    ///     .build();
    /// ```
    #[must_use]
    pub fn builder(
        method: impl Into<String>,
        path: impl Into<String>,
    ) -> SdkRequestBuilder<MethodSet, PathSet> {
        SdkRequestBuilder {
            method: method.into(),
            path: path.into(),
            body: None,
            requires_auth: false,
            headers: BTreeMap::new(),
            _state: std::marker::PhantomData,
        }
    }

    /// Construct a request with an HTTP method and request path.
    /// Prefer using [`Self::builder()`] for more complex requests.
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

// ---------------------------------------------------------------------------
// TypeState markers for SdkRequestBuilder
// ---------------------------------------------------------------------------

/// TypeState marker: HTTP method has been set.
#[derive(Debug, Clone, Copy)]
pub struct MethodSet;

/// TypeState marker: path has been set.
#[derive(Debug, Clone, Copy)]
pub struct PathSet;

/// Typed builder for [`SdkRequest`] with compile-time field validation.
///
/// ## Type-State Pattern
///
/// The builder uses phantom type parameters to track which required fields
/// have been set. This prevents calling `.build()` before all required
/// fields are provided—caught at compile time, not runtime.
#[derive(Debug)]
pub struct SdkRequestBuilder<M, P> {
    method: String,
    path: String,
    body: Option<Vec<u8>>,
    requires_auth: bool,
    headers: BTreeMap<String, String>,
    _state: std::marker::PhantomData<(M, P)>,
}

impl SdkRequestBuilder<MethodSet, PathSet> {
    /// Build the request. Available only when both method and path are set.
    #[must_use]
    pub fn build(self) -> SdkRequest {
        SdkRequest {
            method: self.method,
            path: self.path,
            body: self.body,
            requires_auth: self.requires_auth,
            headers: self.headers,
        }
    }
}

impl<M, P> SdkRequestBuilder<M, P> {
    /// Set the request body.
    #[must_use]
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Mark the request as requiring authentication.
    #[must_use]
    pub const fn auth_required(mut self, required: bool) -> Self {
        self.requires_auth = required;
        self
    }

    /// Add a header to the request.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Add multiple headers to the request.
    #[must_use]
    pub fn headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.headers.extend(headers);
        self
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

/// Transport contract using `tower::Service` as the foundation.
///
/// ## Design Decision: Blanket Implementation
///
/// Rather than defining a custom `Transport` trait, we implement a blanket
/// `Transport` impl for any type that implements `Service<SdkRequest>`.
/// This allows seamless integration with the tower ecosystem:
/// - `tower::retry::Retry` for automatic retries
/// - `tower::timeout::Timeout` for request timeouts
/// - `tower::limit::rate::RateLimit` for rate limiting
/// - Custom middleware via `tower::Layer`
///
/// ## Example: Composing Middleware
///
/// ```no_run
/// use std::time::Duration;
///
/// use ferriskey_sdk::HpxTransport;
/// use tower::ServiceBuilder;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let transport =
///     ServiceBuilder::new().timeout(Duration::from_secs(30)).service(HpxTransport::default());
/// # Ok(())
/// # }
/// ```
pub trait Transport:
    Service<SdkRequest, Response = SdkResponse, Error = TransportError> + Send + Sync
{
}

/// Blanket implementation: any Service<SdkRequest> with the right associated types is a Transport.
impl<T> Transport for T where
    T: Service<SdkRequest, Response = SdkResponse, Error = TransportError> + Send + Sync
{
}

/// Extension trait providing convenience methods for Transport implementors.
///
/// ## Why an Extension Trait?
///
/// Extension traits allow adding methods to all `Transport` implementors
/// without modifying the core trait. This follows the Open/Closed Principle
/// and avoids trait method bloat.
pub trait TransportExt: Transport {
    /// Execute a request and return the response, consuming `self` for one-shot use.
    ///
    /// This is a convenience wrapper around `tower::ServiceExt::oneshot`.
    fn execute(
        &mut self,
        request: SdkRequest,
    ) -> impl Future<Output = Result<SdkResponse, TransportError>> + Send;
}

impl<T> TransportExt for T
where
    T: Transport + Clone,
    <T as Service<SdkRequest>>::Future: Send,
{
    async fn execute(&mut self, request: SdkRequest) -> Result<SdkResponse, TransportError> {
        // Use tower's oneshot for clean single-request execution
        let transport = self.clone();
        transport.oneshot(request).await
    }
}

/// Primary HTTP transport adapter backed by `hpx`.
///
/// ## Usage
///
/// `HpxTransport` implements `tower::Service` directly, making it composable
/// with any tower middleware layer.
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

/// Implement `tower::Service` for `HpxTransport`.
///
/// This makes HpxTransport composable with any tower middleware.
impl Service<SdkRequest> for HpxTransport {
    type Response = SdkResponse;
    type Error = TransportError;
    type Future = Pin<Box<dyn Future<Output = Result<SdkResponse, TransportError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // hpx::Client is always ready
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: SdkRequest) -> Self::Future {
        let client = self.client.clone();
        Box::pin(async move {
            let method = hpx::Method::from_bytes(request.method.as_bytes())
                .map_err(|_| TransportError::InvalidMethod { method: request.method.clone() })?;
            let mut builder = client.request(method, request.path);

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
