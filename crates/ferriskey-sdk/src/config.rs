//! SDK configuration primitives.
//!
//! ## Design Philosophy
//!
//! Configuration uses a typed builder pattern with compile-time validation.
//! The `SdkConfigBuilder` uses TypeState markers to ensure that required
//! fields (like `base_url`) are set before calling `.build()`.
//!
//! ## Example
//!
//! ```
//! use std::time::Duration;
//!
//! use ferriskey_sdk::{AuthStrategy, SdkConfig};
//!
//! let config = SdkConfig::builder("https://api.example.com")
//!     .auth(AuthStrategy::Bearer("token".into()))
//!     .timeout(Duration::from_secs(30))
//!     .build();
//! ```

use std::time::Duration;

// ---------------------------------------------------------------------------
// Authentication Strategy
// ---------------------------------------------------------------------------

/// Authentication strategy applied to outgoing SDK requests.
///
/// ## Type Safety
///
/// Using an enum rather than `Option<String>` ensures that authentication
/// modes are explicitly named and exhaustive pattern matching is possible.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum AuthStrategy {
    /// Do not attach authentication.
    #[default]
    None,
    /// Attach an HTTP bearer token.
    Bearer(String),
    // Future variants can be added without breaking changes:
    // ApiKey { header: String, key: String },
    // OAuth2 { token: String, refresh: String },
}

impl AuthStrategy {
    /// Returns `true` if authentication is configured.
    #[must_use]
    pub const fn is_configured(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Returns the bearer token if this strategy is `Bearer`.
    #[must_use]
    pub const fn bearer_token(&self) -> Option<&String> {
        match self {
            Self::Bearer(token) => Some(token),
            Self::None => None,
        }
    }
}

// ---------------------------------------------------------------------------
// SdkConfig with Typed Builder
// ---------------------------------------------------------------------------

/// Runtime configuration for the FerrisKey SDK.
///
/// ## Immutability
///
/// Once built, `SdkConfig` is immutable. This prevents accidental mutation
/// of configuration during request processing and enables safe sharing
/// across async tasks without locks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SdkConfig {
    /// Base URL used to resolve relative request paths.
    base_url: String,
    /// Authentication strategy used for outgoing requests.
    auth: AuthStrategy,
    /// Request timeout duration.
    timeout: Option<Duration>,
    /// Custom user-agent header value.
    user_agent: Option<String>,
}

impl SdkConfig {
    /// Create a new SDK configuration.
    ///
    /// Prefer using [`Self::builder()`] for more complex configurations.
    #[must_use]
    pub fn new(base_url: impl Into<String>, auth: AuthStrategy) -> Self {
        Self { base_url: base_url.into(), auth, timeout: None, user_agent: None }
    }

    /// Create a typed builder with the required base URL.
    ///
    /// The builder ensures the base URL is always provided.
    #[must_use]
    pub fn builder(base_url: impl Into<String>) -> SdkConfigBuilder<BaseUrlSet> {
        SdkConfigBuilder {
            base_url: base_url.into(),
            auth: AuthStrategy::default(),
            timeout: None,
            user_agent: None,
            _state: std::marker::PhantomData,
        }
    }

    /// Access the base URL.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Access the authentication strategy.
    #[must_use]
    pub const fn auth(&self) -> &AuthStrategy {
        &self.auth
    }

    /// Access the configured timeout.
    #[must_use]
    pub const fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Access the user-agent string.
    #[must_use]
    pub fn user_agent(&self) -> Option<&str> {
        self.user_agent.as_deref()
    }
}

// ---------------------------------------------------------------------------
// TypeState marker for SdkConfigBuilder
// ---------------------------------------------------------------------------

/// TypeState marker: base URL has been set.
#[derive(Debug, Clone, Copy)]
pub struct BaseUrlSet;

/// Typed builder for [`SdkConfig`] with compile-time validation.
///
/// ## Type-State Pattern
///
/// The builder uses a phantom type parameter to track whether the required
/// `base_url` field has been set. Calling `.build()` on an incomplete
/// builder is a compile-time error.
///
/// ```compile_fail
/// // This will not compile - base_url not set:
/// let config = SdkConfigBuilder::<()>::new().build();
/// ```
#[derive(Debug)]
pub struct SdkConfigBuilder<S> {
    base_url: String,
    auth: AuthStrategy,
    timeout: Option<Duration>,
    user_agent: Option<String>,
    _state: std::marker::PhantomData<S>,
}

impl SdkConfigBuilder<BaseUrlSet> {
    /// Build the configuration. Available only when base_url is set.
    #[must_use]
    pub fn build(self) -> SdkConfig {
        SdkConfig {
            base_url: self.base_url,
            auth: self.auth,
            timeout: self.timeout,
            user_agent: self.user_agent,
        }
    }
}

impl<S> SdkConfigBuilder<S> {
    /// Set the authentication strategy.
    #[must_use]
    pub fn auth(mut self, auth: AuthStrategy) -> Self {
        self.auth = auth;
        self
    }

    /// Set the request timeout.
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set the user-agent header.
    #[must_use]
    pub fn user_agent(mut self, agent: impl Into<String>) -> Self {
        self.user_agent = Some(agent.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Extension trait for fluent AuthStrategy construction
// ---------------------------------------------------------------------------

/// Extension trait for building auth strategies fluently.
pub trait AuthStrategyExt {
    /// Create a bearer auth strategy.
    fn bearer(token: impl Into<String>) -> AuthStrategy;
}

impl AuthStrategyExt for AuthStrategy {
    fn bearer(token: impl Into<String>) -> Self {
        Self::Bearer(token.into())
    }
}
