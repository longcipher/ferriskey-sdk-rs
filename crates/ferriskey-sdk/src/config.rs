//! SDK configuration primitives.

/// Authentication strategy applied to outgoing SDK requests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthStrategy {
    /// Do not attach authentication.
    None,
    /// Attach an HTTP bearer token.
    Bearer(String),
}

/// Runtime configuration for the FerrisKey SDK.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SdkConfig {
    /// Base URL used to resolve relative request paths.
    pub base_url: String,
    /// Authentication strategy used for outgoing requests.
    pub auth: AuthStrategy,
}

impl SdkConfig {
    /// Create a new SDK configuration.
    #[must_use]
    pub fn new(base_url: impl Into<String>, auth: AuthStrategy) -> Self {
        Self { base_url: base_url.into(), auth }
    }
}
