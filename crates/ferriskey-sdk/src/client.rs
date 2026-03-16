//! SDK client entrypoint and request preparation helpers.

use std::{collections::BTreeMap, future::Future, pin::Pin};

use serde::de::DeserializeOwned;

use crate::{
    config::{AuthStrategy, SdkConfig},
    encoding::{DecodedResponse, decode_response, encode_request},
    error::SdkError,
    generated::{self, GeneratedOperationDescriptor},
    transport::{SdkRequest, SdkResponse, Transport},
};

/// Caller-provided request input for a generated FerrisKey operation.
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

/// Generated operation entrypoint bound to a specific SDK instance.
#[derive(Clone, Copy, Debug)]
pub struct OperationCall<'sdk, T: Transport> {
    descriptor: &'static GeneratedOperationDescriptor,
    sdk: &'sdk FerriskeySdk<T>,
}

/// Tag-scoped SDK view over the generated operation registry.
#[derive(Clone, Copy, Debug)]
pub struct TagClient<'sdk, T: Transport> {
    sdk: &'sdk FerriskeySdk<T>,
    tag: &'static str,
}

/// FerrisKey SDK entrypoint parameterized by a transport implementation.
#[derive(Clone, Debug)]
pub struct FerriskeySdk<T: Transport> {
    config: SdkConfig,
    transport: T,
}

impl<T: Transport> FerriskeySdk<T> {
    /// Construct a new SDK instance.
    #[must_use]
    pub const fn new(config: SdkConfig, transport: T) -> Self {
        Self { config, transport }
    }

    /// Access the SDK configuration.
    #[must_use]
    pub const fn config(&self) -> &SdkConfig {
        &self.config
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
    ) -> Pin<Box<dyn Future<Output = Result<SdkResponse, SdkError>> + Send + '_>> {
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
    pub fn prepare_request(&self, mut request: SdkRequest) -> Result<SdkRequest, SdkError> {
        request.path = resolve_url(&self.config.base_url, &request.path)?;

        if request.requires_auth {
            match &self.config.auth {
                AuthStrategy::Bearer(token) => {
                    request.headers.insert("authorization".to_string(), format!("Bearer {token}"));
                }
                AuthStrategy::None => return Err(SdkError::MissingAuth),
            }
        }

        Ok(request)
    }

    /// Execute a request through the configured transport.
    pub fn execute(
        &self,
        request: SdkRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SdkResponse, SdkError>> + Send + '_>> {
        Box::pin(async move {
            let prepared_request = self.prepare_request(request)?;
            self.transport.send(prepared_request).await.map_err(SdkError::Transport)
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

impl<T: Transport> OperationCall<'_, T> {
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
    ) -> Pin<Box<dyn Future<Output = Result<SdkResponse, SdkError>> + Send + '_>> {
        Box::pin(async move {
            let request = self.to_request(input)?;
            self.sdk.execute(request).await
        })
    }

    /// Execute this operation and decode the documented response payload.
    pub fn execute_decoded(
        &self,
        input: OperationInput,
    ) -> Pin<Box<dyn Future<Output = Result<DecodedResponse, SdkError>> + Send + '_>> {
        Box::pin(async move {
            let response = self.execute(input).await?;
            decode_response(self.descriptor, response)
        })
    }
}

impl<T: Transport> TagClient<'_, T> {
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
