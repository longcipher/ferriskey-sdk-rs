//! SDK client entrypoint and request preparation helpers.
//!
//! ## Design Philosophy
//!
//! This module implements a TypeState pattern for the SDK builder, ensuring
//! that the transport layer is always configured before the SDK can be used.
//! The `FerriskeySdk<T>` type is parameterized by the transport, making
//! invalid states unrepresentable at compile time.
//!
//! ## Architecture
//!
//! ```text
//! FerriskeySdkBuilder<Unconfigured>
//!     │
//!     ▼ .transport(transport)
//! FerriskeySdkBuilder<Configured<T>>
//!     │
//!     ▼ .build()
//! FerriskeySdk<T>
//! ```
//!
//! ## tower::Service Integration
//!
//! The SDK accepts any `Transport` (which is a blanket impl over
//! `tower::Service<SdkRequest>`), enabling middleware composition:
//!
//! ```no_run
//! use ferriskey_sdk::{AuthStrategy, FerriskeySdk, HpxTransport, SdkConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = SdkConfig::new("https://api.example.com", AuthStrategy::None);
//! let transport = HpxTransport::default();
//!
//! let sdk = FerriskeySdk::builder(config).transport(transport).build();
//! # Ok(())
//! # }
//! ```

use std::{collections::BTreeMap, future::Future, marker::PhantomData, pin::Pin};

use serde::de::DeserializeOwned;
use tower::{Service, ServiceExt};

use crate::{
    config::{AuthStrategy, SdkConfig},
    encoding::{DecodedResponse, decode_response, encode_request},
    error::SdkError,
    generated::{self, GeneratedOperationDescriptor},
    transport::{SdkRequest, SdkResponse, Transport},
};

// ---------------------------------------------------------------------------
// TypeState markers for FerriskeySdkBuilder
// ---------------------------------------------------------------------------

/// TypeState: transport has not been configured yet.
#[derive(Debug, Clone, Copy)]
pub struct Unconfigured;

/// TypeState: transport has been configured.
#[derive(Debug, Clone, Copy)]
pub struct Configured<T>(PhantomData<T>);

// ---------------------------------------------------------------------------
// OperationInput - Typed request input
// ---------------------------------------------------------------------------

/// Caller-provided request input for a generated FerrisKey operation.
///
/// ## Fluent Builder
///
/// Use [`OperationInput::builder()`] for a fluent API:
///
/// ```
/// use ferriskey_sdk::OperationInput;
///
/// let input = OperationInput::builder()
///     .path_param("id", "123")
///     .query_param("filter", vec!["active".to_string()])
///     .header("x-request-id", "abc")
///     .body(br#"{"name": "test"}"#)
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OperationInput {
    /// Optional raw request body.
    pub body: Option<Vec<u8>>,
    /// Additional headers to apply to the generated request.
    pub headers: BTreeMap<String, String>,
    /// Path parameters keyed by their template name.
    pub path_params: BTreeMap<String, String>,
    /// Query parameters keyed by name and preserving repeated values.
    pub query_params: BTreeMap<String, Vec<String>>,
}

impl OperationInput {
    /// Create a new empty operation input.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a fluent builder for operation input.
    #[must_use]
    pub fn builder() -> OperationInputBuilder {
        OperationInputBuilder::default()
    }
}

/// Fluent builder for [`OperationInput`].
#[derive(Debug, Default)]
pub struct OperationInputBuilder {
    body: Option<Vec<u8>>,
    headers: BTreeMap<String, String>,
    path_params: BTreeMap<String, String>,
    query_params: BTreeMap<String, Vec<String>>,
}

impl OperationInputBuilder {
    /// Set the request body.
    #[must_use]
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Add a header.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Add a path parameter.
    #[must_use]
    pub fn path_param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.path_params.insert(name.into(), value.into());
        self
    }

    /// Add a query parameter with a single value.
    #[must_use]
    pub fn query_param_single(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.query_params.insert(name.into(), vec![value.into()]);
        self
    }

    /// Add a query parameter with multiple values.
    #[must_use]
    pub fn query_param(mut self, name: impl Into<String>, values: Vec<String>) -> Self {
        self.query_params.insert(name.into(), values);
        self
    }

    /// Build the operation input.
    #[must_use]
    pub fn build(self) -> OperationInput {
        OperationInput {
            body: self.body,
            headers: self.headers,
            path_params: self.path_params,
            query_params: self.query_params,
        }
    }
}

// ---------------------------------------------------------------------------
// OperationCall - Bound operation
// ---------------------------------------------------------------------------

/// Generated operation entrypoint bound to a specific SDK instance.
///
/// ## Associated Types
///
/// The transport type `T` flows through the entire call chain, ensuring
/// type safety from SDK construction through request execution.
#[derive(Clone, Copy, Debug)]
pub struct OperationCall<'sdk, T: Transport + Clone> {
    descriptor: &'static GeneratedOperationDescriptor,
    sdk: &'sdk FerriskeySdk<T>,
}

impl<T: Transport + Clone> OperationCall<'_, T> {
    /// Access the generated descriptor for this operation.
    #[must_use]
    pub const fn descriptor(&self) -> &'static GeneratedOperationDescriptor {
        self.descriptor
    }

    /// Build a canonical SDK request for this generated operation.
    pub fn to_request(&self, input: OperationInput) -> Result<SdkRequest, SdkError> {
        encode_request(self.descriptor, input)
    }

    /// Execute this operation through the SDK transport.
    pub fn execute(
        &self,
        input: OperationInput,
    ) -> Pin<Box<dyn Future<Output = Result<SdkResponse, SdkError>> + Send + '_>>
    where
        <T as Service<SdkRequest>>::Future: Send,
    {
        Box::pin(async move {
            let request = self.to_request(input)?;
            self.sdk.execute(request).await
        })
    }

    /// Execute this operation and decode the documented response payload.
    pub fn execute_decoded(
        &self,
        input: OperationInput,
    ) -> Pin<Box<dyn Future<Output = Result<DecodedResponse, SdkError>> + Send + '_>>
    where
        <T as Service<SdkRequest>>::Future: Send,
    {
        Box::pin(async move {
            let response = self.execute(input).await?;
            decode_response(self.descriptor, response)
        })
    }
}

// ---------------------------------------------------------------------------
// TagClient - Tag-scoped view
// ---------------------------------------------------------------------------

/// Tag-scoped SDK view over the generated operation registry.
///
/// ## Extension Trait Pattern
///
/// Tag-specific convenience methods can be added via extension traits
/// without modifying the core `TagClient` type.
#[derive(Clone, Copy, Debug)]
pub struct TagClient<'sdk, T: Transport + Clone> {
    sdk: &'sdk FerriskeySdk<T>,
    tag: &'static str,
}

impl<T: Transport + Clone> TagClient<'_, T> {
    /// Access the tag name associated with this client.
    #[must_use]
    pub const fn tag(&self) -> &'static str {
        self.tag
    }

    /// Iterate over the generated descriptors assigned to this tag.
    pub fn descriptors(&self) -> impl Iterator<Item = &'static GeneratedOperationDescriptor> + '_ {
        generated::OPERATION_DESCRIPTORS.iter().filter(move |descriptor| descriptor.tag == self.tag)
    }

    /// Resolve an operation within this tag-scoped view.
    #[must_use]
    pub fn operation(&self, operation_id: &str) -> Option<OperationCall<'_, T>> {
        self.descriptors()
            .find(|descriptor| descriptor.operation_id == operation_id)
            .map(|descriptor| OperationCall { descriptor, sdk: self.sdk })
    }
}

// ---------------------------------------------------------------------------
// FerriskeySdk - Main SDK type
// ---------------------------------------------------------------------------

/// FerrisKey SDK entrypoint parameterized by a transport implementation.
///
/// ## Type-Driven Design
///
/// The generic parameter `T: Transport` ensures that:
/// 1. The transport type is known at compile time
/// 2. Invalid transport configurations are caught before runtime
/// 3. The compiler can optimize based on the concrete transport type
///
/// ## Builder Pattern
///
/// Use [`FerriskeySdk::builder()`] for a fluent, type-safe construction:
///
/// ```no_run
/// use ferriskey_sdk::{AuthStrategy, FerriskeySdk, HpxTransport, SdkConfig};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = SdkConfig::new("https://api.example.com", AuthStrategy::None);
/// let sdk = FerriskeySdk::builder(config).transport(HpxTransport::default()).build();
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct FerriskeySdk<T: Transport + Clone> {
    config: SdkConfig,
    transport: T,
}

impl<T: Transport + Clone> FerriskeySdk<T> {
    /// Construct a new SDK instance directly.
    ///
    /// Prefer using [`Self::builder()`] for a more fluent API.
    #[must_use]
    pub const fn new(config: SdkConfig, transport: T) -> Self {
        Self { config, transport }
    }

    /// Create a typed builder with the required configuration.
    ///
    /// The builder ensures the transport is set before calling `.build()`.
    #[must_use]
    pub const fn builder(config: SdkConfig) -> FerriskeySdkBuilder<T, Unconfigured> {
        FerriskeySdkBuilder { config, transport: None, _state: PhantomData }
    }

    /// Access the SDK configuration.
    #[must_use]
    pub const fn config(&self) -> &SdkConfig {
        &self.config
    }

    /// Access the underlying transport.
    #[must_use]
    pub const fn transport(&self) -> &T {
        &self.transport
    }

    /// Access the full generated operation registry.
    #[must_use]
    pub const fn operations(&self) -> &'static [GeneratedOperationDescriptor] {
        generated::OPERATION_DESCRIPTORS
    }

    /// Access a tag-scoped SDK view.
    #[must_use]
    pub const fn tag(&self, tag: &'static str) -> TagClient<'_, T> {
        TagClient { sdk: self, tag }
    }

    /// Resolve a generated operation by its operation ID.
    #[must_use]
    pub fn operation(&self, operation_id: &str) -> Option<OperationCall<'_, T>> {
        generated::OPERATION_DESCRIPTORS
            .iter()
            .find(|descriptor| descriptor.operation_id == operation_id)
            .map(|descriptor| OperationCall { descriptor, sdk: self })
    }

    /// Execute a generated operation through the canonical SDK request path.
    pub fn execute_operation(
        &self,
        operation_id: &str,
        input: OperationInput,
    ) -> Pin<Box<dyn Future<Output = Result<SdkResponse, SdkError>> + Send + '_>>
    where
        <T as Service<SdkRequest>>::Future: Send,
    {
        let resolved_operation = self.operation(operation_id);
        let requested_operation_id = operation_id.to_string();

        Box::pin(async move {
            let Some(operation) = resolved_operation else {
                return Err(SdkError::UnknownOperation { operation_id: requested_operation_id });
            };

            operation.execute(input).await
        })
    }

    /// Prepare a request by resolving its URL and applying auth.
    ///
    /// ## Design Decision: Result Type
    ///
    /// Returns `Result<SdkRequest, SdkError>` rather than panicking,
    /// enabling callers to handle configuration errors gracefully.
    pub fn prepare_request(&self, mut request: SdkRequest) -> Result<SdkRequest, SdkError> {
        request.path = resolve_url(self.config.base_url(), &request.path)?;

        if request.requires_auth {
            match self.config.auth() {
                AuthStrategy::Bearer(token) => {
                    request.headers.insert("authorization".to_string(), format!("Bearer {token}"));
                }
                AuthStrategy::None => return Err(SdkError::MissingAuth),
            }
        }

        Ok(request)
    }

    /// Execute a request through the configured transport.
    ///
    /// Uses `tower::ServiceExt::oneshot` for clean single-request execution.
    pub fn execute(
        &self,
        request: SdkRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SdkResponse, SdkError>> + Send + '_>>
    where
        <T as Service<SdkRequest>>::Future: Send,
    {
        // We need to clone transport for the async block since oneshot consumes self
        let transport = self.transport.clone();

        Box::pin(async move {
            let prepared_request = self.prepare_request(request)?;

            // Use tower's oneshot for clean execution
            transport.oneshot(prepared_request).await.map_err(SdkError::Transport)
        })
    }

    /// Execute a request and decode a JSON response for the expected status.
    pub fn execute_json<Output>(
        &self,
        request: SdkRequest,
        expected_status: u16,
    ) -> Pin<Box<dyn Future<Output = Result<Output, SdkError>> + Send + '_>>
    where
        Output: DeserializeOwned + Send + 'static,
        <T as Service<SdkRequest>>::Future: Send,
    {
        Box::pin(async move {
            let response = self.execute(request).await?;

            if response.status != expected_status {
                return Err(SdkError::UnexpectedStatus {
                    expected: expected_status,
                    actual: response.status,
                });
            }

            serde_json::from_slice(&response.body).map_err(SdkError::Decode)
        })
    }
}

// ---------------------------------------------------------------------------
// FerriskeySdkBuilder - Type-safe builder
// ---------------------------------------------------------------------------

/// Typed builder for [`FerriskeySdk`] with compile-time validation.
///
/// ## Type-State Pattern
///
/// The builder uses phantom type parameters to track whether the transport
/// has been configured. Calling `.build()` before setting the transport
/// is a compile-time error.
#[derive(Debug)]
pub struct FerriskeySdkBuilder<T: Transport + Clone, S> {
    config: SdkConfig,
    transport: Option<T>,
    _state: PhantomData<S>,
}

impl<T: Transport + Clone> FerriskeySdkBuilder<T, Unconfigured> {
    /// Set the transport. Transitions to `Configured` state.
    #[must_use]
    pub fn transport(mut self, transport: T) -> FerriskeySdkBuilder<T, Configured<T>> {
        self.transport = Some(transport);
        FerriskeySdkBuilder { config: self.config, transport: self.transport, _state: PhantomData }
    }
}

impl<T: Transport + Clone> FerriskeySdkBuilder<T, Configured<T>> {
    /// Build the SDK instance. Available only when transport is configured.
    ///
    /// # Panics
    ///
    /// Panics if the transport was somehow not set (should be impossible
    /// due to type-state guarantees).
    #[must_use]
    #[expect(clippy::expect_used)]
    pub fn build(self) -> FerriskeySdk<T> {
        FerriskeySdk {
            config: self.config,
            transport: self.transport.expect("transport must be set in Configured state"),
        }
    }
}

// ---------------------------------------------------------------------------
// Extension traits for fluent API
// ---------------------------------------------------------------------------

/// Extension trait for convenient SDK construction.
pub trait SdkExt: Sized {
    /// The transport type for this SDK.
    type Transport: Transport + Clone;

    /// Create an SDK with a fluent one-liner.
    ///
    /// ```no_run
    /// use ferriskey_sdk::{AuthStrategy, FerriskeySdk, HpxTransport, SdkConfig, SdkExt};
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = SdkConfig::new("https://api.example.com", AuthStrategy::None);
    /// let sdk = FerriskeySdk::with_transport(config, HpxTransport::default());
    /// # Ok(())
    /// # }
    /// ```
    fn with_transport(
        config: SdkConfig,
        transport: Self::Transport,
    ) -> FerriskeySdk<Self::Transport>;
}

impl<T: Transport + Clone> SdkExt for FerriskeySdk<T> {
    type Transport = T;

    fn with_transport(config: SdkConfig, transport: T) -> Self {
        Self::new(config, transport)
    }
}

// ---------------------------------------------------------------------------
// URL resolution
// ---------------------------------------------------------------------------

/// Resolve a URL from base and path components.
fn resolve_url(base_url: &str, path: &str) -> Result<String, SdkError> {
    if path.starts_with("http://") || path.starts_with("https://") {
        return Ok(path.to_string());
    }

    let trimmed_base = base_url.trim_end_matches('/');
    let trimmed_path = path.trim_start_matches('/');

    if trimmed_base.is_empty() || trimmed_path.is_empty() {
        return Err(SdkError::InvalidUrl { base_url: base_url.to_string(), path: path.to_string() });
    }

    Ok(format!("{trimmed_base}/{trimmed_path}"))
}
