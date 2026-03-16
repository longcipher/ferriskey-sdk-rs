//! BDD scenarios for the current FerrisKey SDK scaffold.

use std::path::PathBuf;

use cucumber::{World, given, then, when};
use ferriskey_sdk::{CartItem, CheckoutResult, checkout_cart};

mod support;

#[derive(Debug, Default, World)]
struct WorkspaceWorld {
    cli_bearer_token: Option<String>,
    cli_help_output: Option<String>,
    cli_parity: Option<support::CliSdkParity>,
    items: Vec<CartItem>,
    prism_server: Option<support::PrismServer>,
    result: Option<CheckoutResult>,
    secured_invocation: Option<support::SecuredInvocation>,
    sdk_registry: Option<support::SdkRegistryInspection>,
    sweep_report: Option<support::ContractSweepReport>,
}

#[given(expr = "the cart contains {string} priced at {int} cents with quantity {int}")]
async fn cart_contains(world: &mut WorkspaceWorld, name: String, price_cents: u32, quantity: u32) {
    world.items.push(CartItem { name, price_cents, quantity });
}

#[when("the customer checks out")]
async fn checkout(world: &mut WorkspaceWorld) {
    world.result = Some(checkout_cart(&world.items));
}

#[then(expr = "an order should be created with total {int} cents")]
async fn order_total(world: &mut WorkspaceWorld, total_cents: u32) {
    assert!(world.result.is_some());

    if let Some(result) = world.result.as_ref() {
        assert_eq!(result.order.total_cents, total_cents);
    }
}

#[then("the cart should be empty")]
async fn cart_empty(world: &mut WorkspaceWorld) {
    assert!(world.result.is_some());

    if let Some(result) = world.result.as_ref() {
        assert!(result.cart.items.is_empty());
    }
}

#[given("the FerrisKey CLI is built from the normalized contract registry")]
async fn cli_built_from_registry(_world: &mut WorkspaceWorld) {}

#[when("I ask the CLI for its top-level help output")]
async fn request_cli_help(world: &mut WorkspaceWorld) {
    world.cli_help_output = Some(
        support::render_cli_help()
            .expect("Task 3.2 should verify CLI help through the real binary"),
    );
}

#[then("I should see one command group for each documented API tag")]
async fn help_lists_tag_groups(world: &mut WorkspaceWorld) {
    let help_output =
        world.cli_help_output.as_ref().expect("CLI help output should be available for assertion");

    for tag in ferriskey_sdk::TAG_NAMES {
        assert!(help_output.contains(tag), "help output should contain tag {tag}");
    }
}

#[given("the CLI is configured with a base URL and optional bearer token")]
async fn cli_is_configured(world: &mut WorkspaceWorld) {
    world.cli_bearer_token = Some("bdd-token".to_string());
}

#[given("the FerrisKey contract has been normalized from docs/openai.json")]
async fn contract_has_been_normalized(_world: &mut WorkspaceWorld) {}

#[when("I inspect the generated SDK operation registry")]
async fn inspect_generated_sdk_operation_registry(world: &mut WorkspaceWorld) {
    world.sdk_registry = Some(
        support::inspect_sdk_registry()
            .expect("Task 3.2 should expose all generated operations through the SDK registry"),
    );
}

#[given("Prism is serving the normalized FerrisKey contract")]
async fn prism_is_serving_contract(world: &mut WorkspaceWorld) {
    world.prism_server = Some(
        support::launch_prism()
            .await
            .expect("Task 3.1 Prism harness should launch from the normalized contract artifact"),
    );
}

#[given("the SDK is configured with a bearer token")]
async fn sdk_has_bearer_token(_world: &mut WorkspaceWorld) {}

#[when("I run the SDK contract sweep")]
async fn run_sdk_contract_sweep(world: &mut WorkspaceWorld) {
    let prism = world
        .prism_server
        .as_ref()
        .expect("Prism should be running before the contract sweep executes");

    world.sweep_report = Some(
        support::run_contract_sweep(&prism.base_url, Some("bdd-prism-token"))
            .await
            .expect("Task 3.1 contract sweep should cover the generated registry"),
    );
}

#[when("I invoke a secured SDK operation")]
async fn invoke_secured_sdk_operation(world: &mut WorkspaceWorld) {
    let prism = world
        .prism_server
        .as_ref()
        .expect("Prism should be running before invoking a secured SDK operation");

    world.secured_invocation = Some(
        support::invoke_secured_operation(&prism.base_url, "bdd-prism-token")
            .await
            .expect("Task 3.1 secured SDK invocation should succeed through Prism"),
    );
}

#[then("every documented operation should be exercised exactly once")]
async fn every_documented_operation_is_exercised_once(world: &mut WorkspaceWorld) {
    let report =
        world.sweep_report.as_ref().expect("contract sweep should run before asserting coverage");

    assert_eq!(report.covered_operations.len(), ferriskey_sdk::OPERATION_DESCRIPTORS.len());
    assert!(report.covered_operations.values().all(|count| *count == 1));
}

#[then("no documented operation should remain uncovered")]
async fn no_documented_operation_remains_uncovered(world: &mut WorkspaceWorld) {
    let report = world
        .sweep_report
        .as_ref()
        .expect("contract sweep should run before asserting uncovered operations");

    assert!(report.uncovered_operations.is_empty());
}

#[then("the request should include the bearer authorization header")]
async fn request_includes_bearer_authorization_header(world: &mut WorkspaceWorld) {
    let invocation = world
        .secured_invocation
        .as_ref()
        .expect("secured SDK invocation should run before asserting authorization headers");

    assert_eq!(invocation.authorization_header.as_deref(), Some("Bearer bdd-prism-token"));
}

#[then("the response should decode into the documented typed result")]
async fn response_decodes_into_documented_typed_result(world: &mut WorkspaceWorld) {
    let invocation = world
        .secured_invocation
        .as_ref()
        .expect("secured SDK invocation should run before asserting decoded response output");
    let descriptor = ferriskey_sdk::OPERATION_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.operation_id == invocation.operation_id)
        .expect("secured invocation should correspond to a generated descriptor");

    assert_eq!(invocation.decoded_response.status, descriptor.primary_success_status);
    assert_eq!(invocation.decoded_response.schema_name, descriptor.primary_response_schema);
    assert!(
        invocation.decoded_response.json_body().is_some() ||
            invocation.decoded_response.raw_body.is_empty()
    );
}

#[when("I invoke a documented operation through CLI subcommands and arguments")]
async fn invoke_documented_operation(world: &mut WorkspaceWorld) {
    let transport =
        world.prism_server.as_ref().expect("Prism should be running before CLI invocation");
    let bearer_token = world
        .cli_bearer_token
        .as_deref()
        .expect("CLI bearer token should be configured before invocation");

    world.cli_parity = Some(
        support::invoke_cli_sdk_parity(
            &transport.base_url,
            bearer_token,
            "update_realm",
            ferriskey_sdk::cli::OutputFormat::Json,
        )
        .await
        .expect("Task 3.2 CLI invocation should execute through Prism and the real binary"),
    );
}

#[then("the CLI should call the same contract-defined operation as the SDK")]
async fn cli_matches_sdk_contract(world: &mut WorkspaceWorld) {
    let parity = world
        .cli_parity
        .as_ref()
        .expect("CLI parity helper should run before asserting SDK parity");

    assert_eq!(parity.operation_id, "update_realm");
    assert_eq!(parity.cli_output["operation_id"], parity.operation_id);
    assert_eq!(parity.cli_output["status"], parity.sdk_response.status);
    assert_eq!(parity.cli_output["schema_name"].as_str(), parity.sdk_response.schema_name,);
    assert_eq!(parity.sdk_request.method, "PUT");
    assert!(parity.sdk_request.path.contains("/realms/"));
    assert!(parity.sdk_request.body.is_some());
}

#[then("the CLI should print the documented response in a structured format")]
async fn cli_prints_structured_output(world: &mut WorkspaceWorld) {
    let parity = world
        .cli_parity
        .as_ref()
        .expect("CLI parity helper should run before asserting rendered output");

    assert!(parity.cli_stdout.starts_with('{'));
    assert_eq!(parity.cli_output["operation_id"], "update_realm");
    assert_eq!(
        parity.cli_output["response"].is_object(),
        parity.sdk_response.json_body().is_some(),
    );
}

#[then("every documented operationId should have a callable SDK entrypoint")]
async fn every_documented_operation_has_callable_sdk_entrypoint(world: &mut WorkspaceWorld) {
    let registry = world
        .sdk_registry
        .as_ref()
        .expect("SDK registry inspection should run before asserting operation coverage");

    assert_eq!(registry.callable_operation_ids, registry.documented_operation_ids);
}

#[then("the SDK should group those operations by the documented API tags")]
async fn sdk_groups_operations_by_documented_tags(world: &mut WorkspaceWorld) {
    let registry = world
        .sdk_registry
        .as_ref()
        .expect("SDK registry inspection should run before asserting tag group coverage");

    assert_eq!(registry.grouped_tags, registry.documented_tags);
}

#[tokio::main]
async fn main() {
    let feature_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../features");
    WorkspaceWorld::cucumber().fail_on_skipped().run_and_exit(feature_path.as_path()).await;
}
