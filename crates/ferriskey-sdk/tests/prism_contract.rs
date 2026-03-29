//! Prism contract normalization and registry property tests.

use std::path::PathBuf;

use ferriskey_sdk::{
    contract,
    generated::{self, models},
};
use proptest::prelude::*;

mod support;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn normalized_contract_preserves_operation_counts() {
    let manifest_dir = manifest_dir();
    let artifacts = contract::generate_artifacts(&manifest_dir)
        .expect("Task 1.2 normalization should produce artifacts");
    let normalized_contract_path = contract::normalized_contract_path(&manifest_dir);
    let normalized_document = contract::load_contract(&normalized_contract_path)
        .expect("build script should have written the normalized contract artifact");
    let normalized_registry = contract::build_registry(&normalized_document)
        .expect("normalized contract artifact should produce a registry");

    assert_eq!(artifacts.registry.path_count, 83);
    assert_eq!(artifacts.registry.operation_count, 126);
    assert_eq!(artifacts.registry.schema_count, 179);
    assert_eq!(artifacts.registry.tags.len(), 15);

    assert_eq!(generated::PATH_COUNT, artifacts.registry.path_count);
    assert_eq!(generated::OPERATION_COUNT, artifacts.registry.operation_count);
    assert_eq!(generated::SCHEMA_COUNT, artifacts.registry.schema_count);
    assert_eq!(generated::TAG_NAMES.len(), artifacts.registry.tags.len());
    assert_eq!(models::SCHEMA_NAMES.len(), artifacts.registry.schemas.len());
    assert_eq!(generated::OPERATION_DESCRIPTORS.len(), artifacts.registry.operations.len());
    assert_eq!(normalized_registry, artifacts.registry);
}

proptest! {
    #[test]
    fn contract_registry_properties(normalization_rounds in 1_usize..4) {
        let manifest_dir = manifest_dir();
        let source_document = contract::load_contract(&contract::source_contract_path(&manifest_dir))
            .expect("source contract should load");
        let baseline = contract::build_registry(
            &contract::normalize_contract(&source_document).expect("source normalization should succeed"),
        ).expect("baseline registry should build");

        let mut document = source_document;
        for _ in 0..normalization_rounds {
            document = contract::normalize_contract(&document)
                .expect("repeated normalization should stay valid");
            let round_registry = contract::build_registry(&document)
                .expect("registry should build on every normalized round");

            prop_assert_eq!(round_registry.path_count, baseline.path_count);
            prop_assert_eq!(round_registry.operation_count, baseline.operation_count);
            prop_assert_eq!(round_registry.schema_count, baseline.schema_count);
            prop_assert_eq!(round_registry.tags.as_slice(), baseline.tags.as_slice());
            prop_assert_eq!(
                round_registry.operations.as_slice(),
                baseline.operations.as_slice(),
            );
            prop_assert_eq!(round_registry.schemas.as_slice(), baseline.schemas.as_slice());
        }
    }
}

#[tokio::test]
async fn prism_sweep_validates_every_documented_operation() {
    let prism = support::launch_prism()
        .await
        .expect("Task 3.1 Prism launcher should boot from the normalized contract");
    let report = support::run_contract_sweep(&prism.base_url, Some("prism-sweep-token"))
        .await
        .expect("Task 3.1 contract sweep should execute every documented operation");

    assert_eq!(report.covered_operations.len(), generated::OPERATION_DESCRIPTORS.len());
    assert!(report.covered_operations.values().all(|count| *count == 1));
    assert!(report.uncovered_operations.is_empty());
}

#[tokio::test]
async fn secured_operations_apply_bearer_auth_and_decode_structured_responses() {
    let prism = support::launch_prism()
        .await
        .expect("Task 3.1 Prism launcher should boot from the normalized contract");
    let invocation = support::invoke_secured_operation(&prism.base_url, "prism-secured-token")
        .await
        .expect("Task 3.1 should exercise a secured operation through Prism");
    let descriptor = generated::OPERATION_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.operation_id == invocation.operation_id)
        .expect("the secured invocation should map to a generated descriptor");

    assert_eq!(invocation.authorization_header.as_deref(), Some("Bearer prism-secured-token"));
    assert_eq!(invocation.decoded_response.status, descriptor.primary_success_status);
    assert_eq!(invocation.decoded_response.schema_name, descriptor.primary_response_schema);
    assert!(
        invocation.decoded_response.json_body().is_some() ||
            invocation.decoded_response.raw_body.is_empty()
    );
}
