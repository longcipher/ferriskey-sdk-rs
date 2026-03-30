//! CLI smoke and property tests for the descriptor-driven FerrisKey command tree.

use ferriskey_sdk::cli;
use proptest::prelude::*;

mod support;

#[test]
fn cli_lists_tag_groups() {
    let help = support::render_cli_help()
        .expect("Task 3.2 should verify top-level CLI help through the real binary");

    for tag in ferriskey_sdk::TAG_NAMES {
        assert!(help.contains(tag), "top-level help should list tag group {tag}");
    }
}

#[tokio::test]
async fn cli_invokes_operation_with_arguments() {
    let prism = support::shared_prism().await;
    let parity = support::invoke_cli_sdk_parity(
        &prism.base_url,
        "cli-token",
        "update_realm",
        cli::OutputFormat::Json,
    )
    .await
    .expect("Task 3.2 CLI smoke should invoke Prism through the real CLI binary");

    assert_eq!(parity.operation_id, "update_realm");
    assert_eq!(parity.cli_output["operation_id"], "update_realm");
    assert_eq!(parity.cli_output["schema_name"].as_str(), parity.sdk_response.schema_name,);
    assert_eq!(parity.cli_output["status"], parity.sdk_response.status);
    assert_eq!(parity.sdk_request.method, "PUT");
    assert!(parity.sdk_request.path.contains("/realms/"));
    assert!(parity.sdk_request.body.is_some());
    assert_eq!(
        parity.cli_output["response"].is_object(),
        parity.sdk_response.json_body().is_some(),
    );
}

#[tokio::test]
async fn cli_formats_pretty_output_for_auth_probe() {
    let prism = support::shared_prism().await;
    let execution = support::invoke_cli_operation(
        &prism.base_url,
        None,
        "get_openid_configuration",
        cli::OutputFormat::Pretty,
    )
    .await
    .expect("Task 3.2 CLI smoke should support auth-tag probes against Prism");

    assert_eq!(execution.json_output["operation_id"], "get_openid_configuration");
    assert_eq!(execution.json_output["status"], 200);
    assert!(execution.stdout.contains('\n'));
    assert!(
        execution.json_output["response"].is_object() ||
            execution.json_output["response"].is_null()
    );
}

proptest! {
    #[test]
    fn cli_argument_properties(
        realm_name in "[A-Za-z][A-Za-z0-9_.~]{0,11}",
        alias in "[A-Za-z][A-Za-z0-9_.~]{0,11}",
        client_id in "[A-Za-z][A-Za-z0-9_.~]{0,11}",
    ) {
        let args = vec![
            "ferriskey-cli".to_string(),
            "--base-url".to_string(),
            "https://api.ferriskey.test".to_string(),
            "broker".to_string(),
            "broker-login".to_string(),
            "--realm-name".to_string(),
            realm_name.clone(),
            "--alias".to_string(),
            alias.clone(),
            "--client-id".to_string(),
            client_id.clone(),
        ];

        let first = cli::parse_args(args.clone()).expect("generated CLI args should parse");
        let second = cli::parse_args(args).expect("generated CLI args should parse repeatedly");

        prop_assert_eq!(first.operation_id, "broker_login");
        prop_assert_eq!(second.operation_id, "broker_login");
        prop_assert_eq!(&first.input, &second.input);
        prop_assert_eq!(first.input.path_params.get("realm_name"), Some(&realm_name));
        prop_assert_eq!(first.input.path_params.get("alias"), Some(&alias));
        prop_assert_eq!(
            first.input.query_params.get("client_id"),
            Some(&vec![client_id]),
        );
    }
}
