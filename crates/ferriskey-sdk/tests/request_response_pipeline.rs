//! Request encoding and response decoding tests for representative operations.

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use ferriskey_sdk::{
    AuthStrategy, FerriskeySdk, OperationInput, SdkConfig, SdkError, SdkRequest, SdkResponse,
    TransportError,
    generated::{self, GeneratedOperationDescriptor},
};
use proptest::prelude::*;
use tower::Service;

#[derive(Clone, Debug)]
struct RecordedTransport {
    captured_requests: Arc<Mutex<Vec<SdkRequest>>>,
    response_body: Vec<u8>,
    response_status: u16,
}

impl RecordedTransport {
    fn new(response_status: u16, response_body: Vec<u8>) -> Self {
        Self { captured_requests: Arc::new(Mutex::new(Vec::new())), response_body, response_status }
    }

    fn captured_requests(&self) -> Vec<SdkRequest> {
        self.captured_requests
            .lock()
            .expect("captured requests mutex should not be poisoned")
            .clone()
    }
}

/// Implement tower::Service for RecordedTransport.
///
/// This makes RecordedTransport a valid Transport via the blanket implementation.
impl Service<SdkRequest> for RecordedTransport {
    type Response = SdkResponse;
    type Error = TransportError;
    type Future = Pin<Box<dyn Future<Output = Result<SdkResponse, TransportError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: SdkRequest) -> Self::Future {
        let captured_requests = Arc::clone(&self.captured_requests);
        let response_status = self.response_status;
        let response_body = self.response_body.clone();

        Box::pin(async move {
            captured_requests
                .lock()
                .expect("captured requests mutex should not be poisoned")
                .push(request);

            Ok(SdkResponse {
                body: response_body,
                headers: BTreeMap::new(),
                status: response_status,
            })
        })
    }
}

fn build_sdk(transport: RecordedTransport) -> FerriskeySdk<RecordedTransport> {
    FerriskeySdk::new(
        SdkConfig::new(
            "https://api.ferriskey.test",
            AuthStrategy::Bearer("pipeline-token".to_string()),
        ),
        transport,
    )
}

fn representative_descriptors() -> Vec<&'static GeneratedOperationDescriptor> {
    let mut seen_tags = BTreeSet::new();
    let mut representatives = Vec::new();

    for descriptor in generated::OPERATION_DESCRIPTORS {
        if seen_tags.insert(descriptor.tag) {
            representatives.push(descriptor);
        }
    }

    representatives
}

fn make_input(descriptor: &'static GeneratedOperationDescriptor) -> OperationInput {
    let mut headers = BTreeMap::new();
    headers.insert("x-test-case".to_string(), descriptor.operation_id.to_string());

    OperationInput {
        body: descriptor.request_body.as_ref().map(|_| br#"{"example":true}"#.to_vec()),
        headers,
        path_params: descriptor
            .parameters
            .iter()
            .filter(|parameter| parameter.location == generated::ParameterLocation::Path)
            .map(|parameter| (parameter.name.to_string(), format!("{}-value", parameter.name)))
            .collect(),
        query_params: descriptor
            .parameters
            .iter()
            .filter(|parameter| parameter.location == generated::ParameterLocation::Query)
            .map(|parameter| {
                (parameter.name.to_string(), vec![format!("{}-query", parameter.name)])
            })
            .collect(),
    }
}

#[tokio::test]
async fn request_response_pipeline() {
    for descriptor in representative_descriptors() {
        let response_json = serde_json::json!({
            "operation_id": descriptor.operation_id,
            "tag": descriptor.tag,
        });
        let transport = RecordedTransport::new(
            descriptor.primary_success_status,
            serde_json::to_vec(&response_json).expect("response JSON should serialize"),
        );
        let sdk = build_sdk(transport.clone());

        let decoded = sdk
            .operation(descriptor.operation_id)
            .expect("representative operation should exist")
            .execute_decoded(make_input(descriptor))
            .await
            .expect("representative operation should decode through the generic pipeline");

        let captured_requests = transport.captured_requests();
        let request =
            captured_requests.last().expect("representative operation should emit a request");

        assert_eq!(decoded.status, descriptor.primary_success_status);
        assert_eq!(decoded.schema_name, descriptor.primary_response_schema);
        assert!(!request.path.contains('{'));
        assert!(request.path.starts_with("https://api.ferriskey.test/"));
        assert_eq!(request.headers.get("x-test-case"), Some(&descriptor.operation_id.to_string()),);

        if descriptor.requires_auth {
            assert_eq!(
                request.headers.get("authorization"),
                Some(&"Bearer pipeline-token".to_string()),
            );
        }
    }
}

#[tokio::test]
async fn documented_error_responses_decode_into_api_errors() {
    let descriptor = generated::OPERATION_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.operation_id == "create_realm")
        .expect("create_realm should exist in the generated registry");
    let transport = RecordedTransport::new(
        400,
        br#"{"message":"invalid realm","code":"bad_request"}"#.to_vec(),
    );
    let sdk = build_sdk(transport);
    let error = sdk
        .operation(descriptor.operation_id)
        .expect("create_realm should be callable")
        .execute_decoded(make_input(descriptor))
        .await
        .expect_err("documented error responses should surface as API errors");

    match error {
        SdkError::ApiResponse { status, schema_name, body, .. } => {
            assert_eq!(status, 400);
            assert_eq!(schema_name, Some("ApiErrorResponse"));
            assert_eq!(
                body,
                Some(serde_json::json!({
                    "message": "invalid realm",
                    "code": "bad_request",
                })),
            );
        }
        other => panic!("expected API error response, got {other:?}"),
    }
}

proptest! {
    #[test]
    fn parameter_encoding_properties(
        realm_name in "[A-Za-z0-9-_.~]{1,12}",
        client_id in "[A-Za-z0-9-_.~]{1,12}",
        query_values in proptest::collection::vec("[A-Za-z0-9-_.~]{1,10}", 0..4),
    ) {
        let descriptor = generated::OPERATION_DESCRIPTORS
            .iter()
            .find(|descriptor| descriptor.operation_id == "broker_login")
            .expect("broker_login should exist in the generated registry");
        let sdk = build_sdk(RecordedTransport::new(200, br#"{}"#.to_vec()));
        let mut input = OperationInput::default();

        input.path_params.insert("realm_name".to_string(), realm_name.clone());
        input.path_params.insert("alias".to_string(), client_id.clone());

        if !query_values.is_empty() {
            input.query_params.insert("scope".to_string(), query_values.clone());
        }

        let first_request = sdk
            .operation(descriptor.operation_id)
            .expect("broker_login should be callable")
            .to_request(input.clone())
            .expect("property input should produce a valid request");
        let second_request = sdk
            .operation(descriptor.operation_id)
            .expect("broker_login should be callable")
            .to_request(input)
            .expect("property input should produce a valid request");

        prop_assert_eq!(&first_request, &second_request);
        prop_assert_eq!(first_request.path.contains('{'), false);
        prop_assert_eq!(first_request.path.contains('}'), false);

        if query_values.is_empty() {
            prop_assert!(!first_request.path.contains('?'));
        } else {
            let repeated_key_count = first_request.path.matches("scope=").count();
            prop_assert_eq!(repeated_key_count, query_values.len());
        }
    }
}
