//! Descriptor-driven CLI helpers shared by the FerrisKey binary and tests.
//!
//! ## Design Philosophy
//!
//! The CLI module provides a bridge between command-line arguments and the
//! typed SDK interface. It uses clap for argument parsing and converts
//! the results into `OperationInput` for SDK execution.
//!
//! ## Extension Point
//!
//! Custom CLI commands can be added via extension traits without modifying
//! the core CLI infrastructure.

use std::{collections::BTreeMap, ffi::OsString, fs};

use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{Value, json};
use tower::Service;

use crate::{
    AuthStrategy, DecodedResponse, FerriskeySdk, OperationInput, SdkConfig, SdkError, SdkRequest,
    Transport,
    generated::{self, GeneratedOperationDescriptor, ParameterLocation},
};

/// Errors raised while parsing or executing CLI requests.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// Command-line parsing failed.
    #[error(transparent)]
    Clap(#[from] clap::Error),
    /// Reading a request body from disk failed.
    #[error("failed to read CLI body file {path}: {source}")]
    BodyFile {
        /// Source file path from the `@file` CLI syntax.
        path: String,
        /// Underlying file-system error.
        source: std::io::Error,
    },
    /// The requested CLI command did not resolve to a generated operation.
    #[error("unknown FerrisKey CLI operation: {operation_id}")]
    UnknownOperation {
        /// Operation identifier requested by the CLI.
        operation_id: String,
    },
    /// The SDK execution path failed.
    #[error(transparent)]
    Sdk(#[from] SdkError),
    /// Rendering structured CLI output failed.
    #[error("failed to render CLI output: {0}")]
    Output(#[from] serde_json::Error),
}

/// Output rendering mode for CLI responses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputFormat {
    /// Compact JSON output.
    Json,
    /// Indented JSON output.
    Pretty,
}

impl OutputFormat {
    /// Parse output format from string.
    #[must_use]
    #[expect(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "pretty" => Self::Pretty,
            _ => Self::Json,
        }
    }
}

/// CLI runtime configuration resolved from the command line.
///
/// ## Immutability
///
/// Once built, `CliConfig` is immutable. This prevents accidental mutation
/// during request processing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliConfig {
    /// Base URL used to resolve generated request paths.
    pub base_url: String,
    /// Optional bearer token applied to secured operations.
    pub bearer_token: Option<String>,
    /// Output mode for structured CLI responses.
    pub output_format: OutputFormat,
}

impl CliConfig {
    /// Convert CLI config to SDK config.
    #[must_use]
    pub fn to_sdk_config(&self) -> SdkConfig {
        let auth = self.bearer_token.clone().map_or(AuthStrategy::None, AuthStrategy::Bearer);

        SdkConfig::new(self.base_url.clone(), auth)
    }
}

/// Parsed CLI invocation normalized into the shared SDK request shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliInvocation {
    /// Runtime configuration resolved from global CLI arguments.
    pub config: CliConfig,
    /// Generated operation identifier selected by the CLI subcommand tree.
    pub operation_id: &'static str,
    /// Canonical SDK request input assembled from CLI arguments.
    pub input: OperationInput,
}

/// Render the top-level CLI help text.
#[must_use]
pub fn render_help() -> String {
    let mut command = build_command();
    let mut buffer = Vec::new();

    if command.write_long_help(&mut buffer).is_err() {
        return String::new();
    }

    String::from_utf8(buffer).unwrap_or_default()
}

/// Parse CLI arguments into a normalized invocation.
pub fn parse_args<I, T>(args: I) -> Result<CliInvocation, CliError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let matches = build_command().try_get_matches_from(args)?;
    parse_matches(&matches)
}

/// Execute a parsed CLI invocation through the shared SDK runtime.
///
/// ## Generic Transport
///
/// The transport type is generic, allowing callers to provide any
/// `tower::Service<SdkRequest>` implementation. This enables
/// middleware composition at the call site.
pub async fn execute_with_transport<T>(
    invocation: CliInvocation,
    transport: T,
) -> Result<String, CliError>
where
    T: Transport + Clone,
    <T as Service<SdkRequest>>::Future: Send,
{
    let sdk_config = invocation.config.to_sdk_config();
    let sdk = FerriskeySdk::new(sdk_config, transport);

    let operation = sdk.operation(invocation.operation_id).ok_or_else(|| {
        CliError::UnknownOperation { operation_id: invocation.operation_id.to_string() }
    })?;

    let decoded = operation.execute_decoded(invocation.input.clone()).await?;

    render_output(invocation.operation_id, &decoded, invocation.config.output_format)
}

fn build_command() -> Command {
    let mut command = Command::new("ferriskey-cli")
        .about("FerrisKey CLI")
        .arg(
            Arg::new("base-url")
                .long("base-url")
                .required(true)
                .value_name("URL")
                .help("Base URL for the FerrisKey API"),
        )
        .arg(
            Arg::new("bearer-token")
                .long("bearer-token")
                .global(true)
                .value_name("TOKEN")
                .help("Optional bearer token for secured operations"),
        )
        .arg(
            Arg::new("output")
                .long("output")
                .default_value("json")
                .global(true)
                .value_parser(["json", "pretty"])
                .value_name("FORMAT")
                .help("Structured output mode"),
        );

    for tag in generated::TAG_NAMES {
        let mut tag_command = Command::new(*tag);

        for descriptor in
            generated::OPERATION_DESCRIPTORS.iter().filter(|descriptor| descriptor.tag == *tag)
        {
            tag_command = tag_command.subcommand(operation_command(descriptor));
        }

        command = command.subcommand(tag_command);
    }

    command
}

fn operation_command(descriptor: &'static GeneratedOperationDescriptor) -> Command {
    let mut command = Command::new(leak_string(command_name(descriptor.operation_id)));

    for parameter in descriptor.parameters {
        let long_name = leak_string(parameter.name.replace('_', "-"));
        let mut arg = Arg::new(parameter.name)
            .long(long_name)
            .value_name(parameter.name)
            .required(parameter.required)
            .help(parameter_help(parameter.location));

        if parameter.location == ParameterLocation::Query {
            arg = arg.action(ArgAction::Append);
        }

        command = command.arg(arg);
    }

    if let Some(request_body) = descriptor.request_body {
        let mut body_arg = Arg::new("body")
            .long("body")
            .value_name("JSON_OR_@FILE")
            .help("Request body as inline JSON or @path/to/file.json");

        if request_body.required && !request_body.nullable {
            body_arg = body_arg.required(true);
        }

        command = command.arg(body_arg);
    }

    command
}

fn parse_matches(matches: &ArgMatches) -> Result<CliInvocation, CliError> {
    let config = CliConfig {
        base_url: required_string(matches, "base-url")?,
        bearer_token: matches.get_one::<String>("bearer-token").cloned(),
        output_format: OutputFormat::from_str(&required_string(matches, "output")?),
    };

    let (_, tag_matches) = matches.subcommand().ok_or_else(|| {
        clap::Error::raw(clap::error::ErrorKind::MissingSubcommand, "an API tag is required")
    })?;

    let (operation_name, operation_matches) = tag_matches.subcommand().ok_or_else(|| {
        clap::Error::raw(clap::error::ErrorKind::MissingSubcommand, "an operation is required")
    })?;

    let descriptor = generated::OPERATION_DESCRIPTORS
        .iter()
        .find(|descriptor| command_name(descriptor.operation_id) == operation_name)
        .ok_or_else(|| CliError::UnknownOperation {
            operation_id: operation_name.replace('-', "_"),
        })?;

    let input = parse_operation_input(descriptor, operation_matches)?;

    Ok(CliInvocation { config, operation_id: descriptor.operation_id, input })
}

fn parse_operation_input(
    descriptor: &'static GeneratedOperationDescriptor,
    matches: &ArgMatches,
) -> Result<OperationInput, CliError> {
    let mut headers = BTreeMap::new();
    let mut path_params = BTreeMap::new();
    let mut query_params = BTreeMap::new();

    for parameter in descriptor.parameters {
        let values = matches
            .get_many::<String>(parameter.name)
            .map(|values| values.cloned().collect::<Vec<_>>())
            .unwrap_or_default();

        if values.is_empty() {
            continue;
        }

        match parameter.location {
            ParameterLocation::Header => {
                headers.insert(parameter.name.to_string(), values[0].clone());
            }
            ParameterLocation::Path => {
                path_params.insert(parameter.name.to_string(), values[0].clone());
            }
            ParameterLocation::Query => {
                query_params.insert(parameter.name.to_string(), values);
            }
        }
    }

    let body = if descriptor.request_body.is_some() {
        matches.get_one::<String>("body").map(|value| read_body(value)).transpose()?
    } else {
        None
    };

    Ok(OperationInput { body, headers, path_params, query_params })
}

fn read_body(value: &str) -> Result<Vec<u8>, CliError> {
    if let Some(path) = value.strip_prefix('@') {
        return fs::read(path)
            .map_err(|source| CliError::BodyFile { path: path.to_string(), source });
    }

    Ok(value.as_bytes().to_vec())
}

fn render_output(
    operation_id: &str,
    response: &DecodedResponse,
    output_format: OutputFormat,
) -> Result<String, CliError> {
    let response_value = response.json_body().cloned().unwrap_or_else(|| {
        if response.raw_body.is_empty() {
            Value::Null
        } else {
            Value::String(String::from_utf8_lossy(&response.raw_body).into_owned())
        }
    });

    let rendered = json!({
        "operation_id": operation_id,
        "schema_name": response.schema_name,
        "status": response.status,
        "response": response_value,
    });

    match output_format {
        OutputFormat::Json => serde_json::to_string(&rendered).map_err(CliError::Output),
        OutputFormat::Pretty => serde_json::to_string_pretty(&rendered).map_err(CliError::Output),
    }
}

fn required_string(matches: &ArgMatches, name: &str) -> Result<String, CliError> {
    matches.get_one::<String>(name).cloned().ok_or_else(|| {
        clap::Error::raw(
            clap::error::ErrorKind::MissingRequiredArgument,
            format!("missing required argument --{name}"),
        )
        .into()
    })
}

const fn parameter_help(location: ParameterLocation) -> &'static str {
    match location {
        ParameterLocation::Header => "Header parameter",
        ParameterLocation::Path => "Path parameter",
        ParameterLocation::Query => "Query parameter",
    }
}

fn command_name(operation_id: &str) -> String {
    operation_id.replace('_', "-")
}

fn leak_string(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}
