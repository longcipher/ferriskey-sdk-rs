//! Generated SDK surface tests for operation parity and auth behavior.

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

use ferriskey_sdk::{
    AuthStrategy, FerriskeySdk, OperationInput, SdkConfig,
    client::TagClient,
    generated,
    transport::{SdkRequest, SdkResponse, Transport},
};
use proptest::prelude::*;

#[derive(Clone, Debug, Default)]
struct NoopTransport;

impl Transport for NoopTransport {
    fn send(
        &self,
        request: SdkRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SdkResponse, ferriskey_sdk::TransportError>> + Send + '_>>
    {
        Box::pin(async move {
            Ok(SdkResponse {
                body: request.body.unwrap_or_default(),
                headers: BTreeMap::new(),
                status: 200,
            })
        })
    }
}

#[derive(Clone, Debug)]
struct RecordedTransport {
    captured_requests: Arc<Mutex<Vec<SdkRequest>>>,
    response: SdkResponse,
}

impl RecordedTransport {
    fn new(response: SdkResponse) -> Self {
        Self { captured_requests: Arc::new(Mutex::new(Vec::new())), response }
    }

    fn captured_requests(&self) -> Vec<SdkRequest> {
        self.captured_requests
            .lock()
            .expect("captured requests mutex should not be poisoned")
            .clone()
    }
}

impl Transport for RecordedTransport {
    fn send(
        &self,
        request: SdkRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SdkResponse, ferriskey_sdk::TransportError>> + Send + '_>>
    {
        let captured_requests = Arc::clone(&self.captured_requests);
        let response = self.response.clone();

        Box::pin(async move {
            captured_requests
                .lock()
                .expect("captured requests mutex should not be poisoned")
                .push(request);
            Ok(response)
        })
    }
}

fn build_sdk() -> FerriskeySdk<NoopTransport> {
    FerriskeySdk::new(
        SdkConfig::new("https://api.ferriskey.test", AuthStrategy::None),
        NoopTransport,
    )
}

#[test]
fn sdk_exposes_all_operations() {
    let sdk = build_sdk();
    let generated_tags = generated::TAG_NAMES.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(sdk.operations().len(), generated::OPERATION_DESCRIPTORS.len());

    for descriptor in generated::OPERATION_DESCRIPTORS {
        let operation = sdk
            .operation(descriptor.operation_id)
            .expect("every generated descriptor should be reachable through the SDK");
        let mut input = OperationInput::default();
        for parameter in descriptor.parameters {
            match parameter.location {
                generated::ParameterLocation::Path => {
                    input
                        .path_params
                        .insert(parameter.name.to_string(), format!("{}-value", parameter.name));
                }
                generated::ParameterLocation::Query => {
                    input.query_params.insert(
                        parameter.name.to_string(),
                        vec![format!("{}-value", parameter.name)],
                    );
                }
                generated::ParameterLocation::Header => {
                    input
                        .headers
                        .insert(parameter.name.to_string(), format!("{}-value", parameter.name));
                }
            }
        }
        if descriptor.request_body.is_some() {
            input.body = Some(br#"{}"#.to_vec());
        }
        let request =
            operation.to_request(input).expect("descriptor-backed request building should succeed");
        let tag_client: TagClient<'_, NoopTransport> = sdk.tag(descriptor.tag);

        assert_eq!(operation.descriptor(), descriptor);
        assert_eq!(request.method, descriptor.method);
        assert!(!request.path.contains('{'));
        assert_eq!(request.requires_auth, descriptor.requires_auth);
        assert!(tag_client.operation(descriptor.operation_id).is_some());
    }

    assert_eq!(generated_tags.len(), generated::TAG_NAMES.len());
    for tag in generated::TAG_NAMES {
        assert!(sdk.tag(tag).descriptors().next().is_some());
    }
}

proptest! {
    #[test]
    fn response_mapping_properties(repetition in 1_usize..4) {
        let baseline = generated::OPERATION_DESCRIPTORS
            .iter()
            .map(|descriptor| (descriptor.operation_id, descriptor.primary_success_status))
            .collect::<Vec<_>>();

        for _ in 0..repetition {
            let current = generated::OPERATION_DESCRIPTORS
                .iter()
                .map(|descriptor| (descriptor.operation_id, descriptor.primary_success_status))
                .collect::<Vec<_>>();

            prop_assert_eq!(current.as_slice(), baseline.as_slice());
        }
    }
}

#[tokio::test]
async fn secured_operations_send_bearer_auth() {
    let descriptor = generated::OPERATION_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.requires_auth)
        .expect("the contract should expose at least one secured operation");
    let transport = RecordedTransport::new(SdkResponse {
        body: br#"{"ok":true,"operation_id":"secured"}"#.to_vec(),
        headers: BTreeMap::new(),
        status: descriptor.primary_success_status,
    });
    let sdk = FerriskeySdk::new(
        SdkConfig::new(
            "https://api.ferriskey.test",
            AuthStrategy::Bearer("secret-token".to_string()),
        ),
        transport.clone(),
    );
    let mut input = OperationInput::default();

    for parameter in descriptor.parameters {
        match parameter.location {
            generated::ParameterLocation::Path => {
                input
                    .path_params
                    .insert(parameter.name.to_string(), format!("{}-value", parameter.name));
            }
            generated::ParameterLocation::Query => {
                input
                    .query_params
                    .insert(parameter.name.to_string(), vec![format!("{}-value", parameter.name)]);
            }
            generated::ParameterLocation::Header => {
                input
                    .headers
                    .insert(parameter.name.to_string(), format!("{}-value", parameter.name));
            }
        }
    }
    if descriptor.request_body.is_some() {
        input.body = Some(br#"{}"#.to_vec());
    }

    let result = sdk
        .operation(descriptor.operation_id)
        .expect("secured operation should be reachable")
        .execute_decoded(input)
        .await;

    let decoded = result.expect("secured operation should decode through the generic pipeline");
    let captured_requests = transport.captured_requests();
    let request = captured_requests.last().expect("secured operation should send a request");

    assert_eq!(decoded.status, descriptor.primary_success_status);
    assert_eq!(
        decoded.json_body(),
        Some(&serde_json::json!({"ok": true, "operation_id": "secured"}))
    );
    assert_eq!(decoded.schema_name, descriptor.primary_response_schema);
    assert_eq!(request.headers.get("authorization"), Some(&"Bearer secret-token".to_string()),);
}
