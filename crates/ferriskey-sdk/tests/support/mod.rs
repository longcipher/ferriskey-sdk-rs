use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    ffi::OsStr,
    fs::{self, OpenOptions},
    io,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use ferriskey_sdk::{
    AuthStrategy, DecodedResponse, FerriskeySdk, HpxTransport, OperationInput, SdkConfig,
    SdkRequest, Transport, contract,
    generated::{self, GeneratedOperationDescriptor, ParameterLocation},
};
use serde_json::{Map, Number, Value};

const PRISM_HOST: &str = "127.0.0.1";
const PRISM_LOG_TAIL_LINES: usize = 50;
const PRISM_PROBE_PATH: &str = "/realms/test/.well-known/openid-configuration";
const PRISM_READY_TIMEOUT: Duration = Duration::from_secs(20);
const PRISM_RETRY_DELAY: Duration = Duration::from_millis(250);
const SAMPLE_DATE_TIME: &str = "2026-03-17T00:00:00Z";
const SAMPLE_EMAIL: &str = "sdk@example.test";
const SAMPLE_REALM_NAME: &str = "test";
const SAMPLE_STRING: &str = "example";
const SAMPLE_URI: &str = "https://example.test/callback";
const SAMPLE_UUID: &str = "00000000-0000-4000-8000-000000000001";

type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug)]
pub(crate) struct PrismServer {
    child: Arc<Mutex<Option<Child>>>,
    pub(crate) base_url: String,
    pub(crate) log_path: PathBuf,
    pub(crate) probe_path: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContractSweepReport {
    pub(crate) covered_operations: BTreeMap<String, usize>,
    pub(crate) uncovered_operations: Vec<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SecuredInvocation {
    pub(crate) authorization_header: Option<String>,
    pub(crate) decoded_response: DecodedResponse,
    pub(crate) operation_id: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CliExecution {
    pub(crate) json_output: Value,
    pub(crate) status: i32,
    pub(crate) stderr: String,
    pub(crate) stdout: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CliSdkParity {
    pub(crate) cli_output: Value,
    pub(crate) cli_stdout: String,
    pub(crate) operation_id: String,
    pub(crate) sdk_request: SdkRequest,
    pub(crate) sdk_response: DecodedResponse,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SdkRegistryInspection {
    pub(crate) callable_operation_ids: BTreeSet<String>,
    pub(crate) documented_operation_ids: BTreeSet<String>,
    pub(crate) documented_tags: BTreeSet<String>,
    pub(crate) grouped_tags: BTreeSet<String>,
}

#[derive(Clone, Debug)]
struct OperationContract {
    parameters: Vec<ParameterContract>,
    request_body: Option<RequestBodyContract>,
}

#[derive(Clone, Debug)]
struct ParameterContract {
    location: ParameterLocation,
    name: String,
    required: bool,
    schema: Value,
}

#[derive(Clone, Debug)]
struct RequestBodyContract {
    content_type: String,
    schema: Value,
}

#[derive(Clone, Debug)]
struct RecordingTransport<T: Transport> {
    captured_requests: Arc<Mutex<Vec<SdkRequest>>>,
    inner: T,
}

impl<T: Transport> RecordingTransport<T> {
    fn new(inner: T) -> Self {
        Self { captured_requests: Arc::new(Mutex::new(Vec::new())), inner }
    }

    fn captured_requests(&self) -> Vec<SdkRequest> {
        lock_or_recover(&self.captured_requests).clone()
    }
}

impl<T: Transport> Transport for RecordingTransport<T> {
    fn send(
        &self,
        request: SdkRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<ferriskey_sdk::SdkResponse, ferriskey_sdk::TransportError>,
                > + Send
                + '_,
        >,
    > {
        let captured_requests = Arc::clone(&self.captured_requests);
        let request_snapshot = request.clone();
        let inner = &self.inner;

        Box::pin(async move {
            lock_or_recover(&captured_requests).push(request_snapshot);
            inner.send(request).await
        })
    }
}

impl PrismServer {
    fn probe_url(&self) -> String {
        format!("{}{}", self.base_url, self.probe_path)
    }
}

impl Drop for PrismServer {
    fn drop(&mut self) {
        if Arc::strong_count(&self.child) != 1 {
            return;
        }

        if let Some(mut child) = lock_or_recover(&self.child).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub(crate) async fn launch_prism() -> TestResult<PrismServer> {
    let manifest_dir = manifest_dir();
    let normalized_contract_path = ensure_normalized_contract(&manifest_dir)?;
    let log_path = prism_log_path(&manifest_dir);
    let port = prism_port()?;
    let base_url = format!("http://{PRISM_HOST}:{port}");
    let log_file = OpenOptions::new().create(true).truncate(true).write(true).open(&log_path)?;
    let stderr = log_file.try_clone()?;
    let child = Command::new(prism_bin())
        .arg("mock")
        .arg(&normalized_contract_path)
        .arg("--host")
        .arg(PRISM_HOST)
        .arg("--port")
        .arg(port.to_string())
        .arg("--dynamic")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr))
        .spawn()?;
    let prism = PrismServer {
        child: Arc::new(Mutex::new(Some(child))),
        base_url,
        log_path,
        probe_path: PRISM_PROBE_PATH.to_string(),
    };

    wait_for_prism_ready(&prism).await?;

    Ok(prism)
}

#[allow(dead_code)]
pub(crate) async fn run_contract_sweep(
    base_url: &str,
    bearer_token: Option<&str>,
) -> TestResult<ContractSweepReport> {
    let manifest_dir = manifest_dir();
    let document = load_normalized_contract(&manifest_dir)?;
    let operation_contracts = build_operation_contracts(&document)?;
    let transport = RecordingTransport::new(HpxTransport::default());
    let sdk = build_sdk(base_url, bearer_token, transport.clone());
    let mut covered_operations = BTreeMap::new();
    let mut uncovered_operations = Vec::new();

    for descriptor in generated::OPERATION_DESCRIPTORS {
        let Some(operation_contract) = operation_contracts.get(descriptor.operation_id) else {
            uncovered_operations.push(descriptor.operation_id.to_string());
            continue;
        };
        let input = synthesize_operation_input(&document, descriptor, operation_contract)?;
        let operation = sdk.operation(descriptor.operation_id).ok_or_else(|| {
            other_error(format!(
                "generated SDK did not expose operation {}",
                descriptor.operation_id
            ))
        })?;
        let decoded = operation.execute_decoded(input).await.map_err(|error| {
            other_error(format!(
                "operation {} failed against Prism: {error}",
                descriptor.operation_id
            ))
        })?;

        validate_decoded_response(descriptor, &decoded)?;
        *covered_operations.entry(descriptor.operation_id.to_string()).or_insert(0) += 1;
    }

    let captured_requests = transport.captured_requests();
    if captured_requests.len() != generated::OPERATION_DESCRIPTORS.len() {
        return Err(other_error(format!(
            "expected {} captured Prism requests but saw {}",
            generated::OPERATION_DESCRIPTORS.len(),
            captured_requests.len()
        )));
    }

    uncovered_operations.extend(
        generated::OPERATION_DESCRIPTORS
            .iter()
            .filter(|descriptor| !covered_operations.contains_key(descriptor.operation_id))
            .map(|descriptor| descriptor.operation_id.to_string()),
    );

    Ok(ContractSweepReport { covered_operations, uncovered_operations })
}

#[allow(dead_code)]
pub(crate) async fn invoke_secured_operation(
    base_url: &str,
    bearer_token: &str,
) -> TestResult<SecuredInvocation> {
    let manifest_dir = manifest_dir();
    let document = load_normalized_contract(&manifest_dir)?;
    let operation_contracts = build_operation_contracts(&document)?;
    let descriptor = generated::OPERATION_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.requires_auth)
        .ok_or_else(|| {
            other_error("the generated registry does not contain a secured operation")
        })?;
    let operation_contract = operation_contracts.get(descriptor.operation_id).ok_or_else(|| {
        other_error(format!(
            "missing contract metadata for secured operation {}",
            descriptor.operation_id
        ))
    })?;
    let input = synthesize_operation_input(&document, descriptor, operation_contract)?;
    let transport = RecordingTransport::new(HpxTransport::default());
    let sdk = build_sdk(base_url, Some(bearer_token), transport.clone());
    let decoded_response = sdk
        .operation(descriptor.operation_id)
        .ok_or_else(|| {
            other_error(format!("missing secured operation {}", descriptor.operation_id))
        })?
        .execute_decoded(input)
        .await
        .map_err(|error| {
            other_error(format!(
                "secured operation {} failed against Prism: {error}",
                descriptor.operation_id
            ))
        })?;
    let captured_requests = transport.captured_requests();
    let authorization_header =
        captured_requests.last().and_then(|request| request.headers.get("authorization")).cloned();

    validate_decoded_response(descriptor, &decoded_response)?;

    Ok(SecuredInvocation {
        authorization_header,
        decoded_response,
        operation_id: descriptor.operation_id.to_string(),
    })
}

#[allow(dead_code)]
pub(crate) fn inspect_sdk_registry() -> TestResult<SdkRegistryInspection> {
    let sdk = FerriskeySdk::new(
        SdkConfig::new("http://127.0.0.1:4010", AuthStrategy::None),
        HpxTransport::default(),
    );
    let documented_operation_ids = generated::OPERATION_DESCRIPTORS
        .iter()
        .map(|descriptor| descriptor.operation_id.to_string())
        .collect::<BTreeSet<_>>();
    let callable_operation_ids = generated::OPERATION_DESCRIPTORS
        .iter()
        .filter_map(|descriptor| sdk.operation(descriptor.operation_id).map(|_| descriptor))
        .map(|descriptor| descriptor.operation_id.to_string())
        .collect::<BTreeSet<_>>();
    let documented_tags =
        generated::TAG_NAMES.iter().map(|tag| (*tag).to_string()).collect::<BTreeSet<_>>();
    let grouped_tags = generated::TAG_NAMES
        .iter()
        .filter(|tag| sdk.tag(tag).descriptors().next().is_some())
        .map(|tag| (*tag).to_string())
        .collect::<BTreeSet<_>>();

    Ok(SdkRegistryInspection {
        callable_operation_ids,
        documented_operation_ids,
        documented_tags,
        grouped_tags,
    })
}

#[allow(dead_code)]
pub(crate) fn render_cli_help() -> TestResult<String> {
    let execution = run_cli_command(["--help"])?;

    Ok(execution.stdout)
}

fn build_sdk(
    base_url: &str,
    bearer_token: Option<&str>,
    transport: RecordingTransport<HpxTransport>,
) -> FerriskeySdk<RecordingTransport<HpxTransport>> {
    let auth =
        bearer_token.map_or(AuthStrategy::None, |token| AuthStrategy::Bearer(token.to_string()));

    FerriskeySdk::new(SdkConfig::new(base_url, auth), transport)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SdkOperationInvocation {
    decoded_response: DecodedResponse,
    operation_id: String,
    request: SdkRequest,
}

#[allow(dead_code)]
async fn invoke_sdk_operation(
    base_url: &str,
    bearer_token: Option<&str>,
    operation_id: &str,
) -> TestResult<SdkOperationInvocation> {
    let manifest_dir = manifest_dir();
    let document = load_normalized_contract(&manifest_dir)?;
    let operation_contracts = build_operation_contracts(&document)?;
    let descriptor = descriptor_by_operation_id(operation_id)?;
    let operation_contract = operation_contracts.get(operation_id).ok_or_else(|| {
        other_error(format!("missing normalized contract metadata for {operation_id}"))
    })?;
    let input = synthesize_operation_input(&document, descriptor, operation_contract)?;
    let transport = RecordingTransport::new(HpxTransport::default());
    let sdk = build_sdk(base_url, bearer_token, transport.clone());
    let decoded_response = sdk
        .operation(operation_id)
        .ok_or_else(|| other_error(format!("missing SDK operation {operation_id}")))?
        .execute_decoded(input)
        .await
        .map_err(|error| {
            other_error(format!("SDK operation {operation_id} failed against Prism: {error}"))
        })?;
    let request = transport.captured_requests().into_iter().last().ok_or_else(|| {
        other_error(format!("SDK operation {operation_id} did not emit a request"))
    })?;

    Ok(SdkOperationInvocation { decoded_response, operation_id: operation_id.to_string(), request })
}

#[allow(dead_code)]
pub(crate) async fn invoke_cli_operation(
    base_url: &str,
    bearer_token: Option<&str>,
    operation_id: &str,
    output_format: ferriskey_sdk::cli::OutputFormat,
) -> TestResult<CliExecution> {
    let manifest_dir = manifest_dir();
    let document = load_normalized_contract(&manifest_dir)?;
    let operation_contracts = build_operation_contracts(&document)?;
    let descriptor = descriptor_by_operation_id(operation_id)?;
    let operation_contract = operation_contracts.get(operation_id).ok_or_else(|| {
        other_error(format!("missing normalized contract metadata for {operation_id}"))
    })?;
    let input = synthesize_operation_input(&document, descriptor, operation_contract)?;
    let args = build_cli_args(base_url, bearer_token, descriptor, &input, output_format)?;

    run_cli_command(args)
}

#[allow(dead_code)]
pub(crate) async fn invoke_cli_sdk_parity(
    base_url: &str,
    bearer_token: &str,
    operation_id: &str,
    output_format: ferriskey_sdk::cli::OutputFormat,
) -> TestResult<CliSdkParity> {
    let execution =
        invoke_cli_operation(base_url, Some(bearer_token), operation_id, output_format).await?;
    let sdk_invocation = invoke_sdk_operation(base_url, Some(bearer_token), operation_id).await?;

    Ok(CliSdkParity {
        cli_output: execution.json_output,
        cli_stdout: execution.stdout,
        operation_id: sdk_invocation.operation_id,
        sdk_request: sdk_invocation.request,
        sdk_response: sdk_invocation.decoded_response,
    })
}

fn build_operation_contracts(document: &Value) -> TestResult<BTreeMap<String, OperationContract>> {
    let paths = document
        .get("paths")
        .and_then(Value::as_object)
        .ok_or_else(|| other_error("normalized contract is missing paths"))?;
    let mut operation_contracts = BTreeMap::new();

    for (_path, path_item) in paths {
        let Some(path_item_object) = path_item.as_object() else {
            continue;
        };

        let path_parameters = collect_parameters(document, path_item_object.get("parameters"))?;

        for method in ["delete", "get", "head", "options", "patch", "post", "put", "trace"] {
            let Some(operation) = path_item_object.get(method) else {
                continue;
            };
            let operation = resolve_reference(document, operation)?;
            let operation_object = operation.as_object().ok_or_else(|| {
                other_error(format!("operation {method} must resolve to an object"))
            })?;
            let operation_id = operation_object
                .get("operationId")
                .and_then(Value::as_str)
                .ok_or_else(|| other_error(format!("operation {method} is missing operationId")))?;
            let mut parameters = path_parameters.clone();

            for parameter in collect_parameters(document, operation_object.get("parameters"))? {
                upsert_parameter(&mut parameters, parameter);
            }

            let request_body = operation_object
                .get("requestBody")
                .map(|request_body| build_request_body_contract(document, request_body))
                .transpose()?;

            operation_contracts
                .insert(operation_id.to_string(), OperationContract { parameters, request_body });
        }
    }

    Ok(operation_contracts)
}

fn build_cli_args(
    base_url: &str,
    bearer_token: Option<&str>,
    descriptor: &'static GeneratedOperationDescriptor,
    input: &OperationInput,
    output_format: ferriskey_sdk::cli::OutputFormat,
) -> TestResult<Vec<String>> {
    let mut args = vec![
        "--base-url".to_string(),
        base_url.to_string(),
        "--output".to_string(),
        match output_format {
            ferriskey_sdk::cli::OutputFormat::Json => "json".to_string(),
            ferriskey_sdk::cli::OutputFormat::Pretty => "pretty".to_string(),
        },
        descriptor.tag.to_string(),
        command_name(descriptor.operation_id),
    ];

    if let Some(token) = bearer_token {
        args.splice(2..2, ["--bearer-token".to_string(), token.to_string()]);
    }

    for parameter in descriptor.parameters {
        let flag = format!("--{}", parameter.name.replace('_', "-"));

        match parameter.location {
            ParameterLocation::Header => {
                if let Some(value) = input.headers.get(parameter.name) {
                    args.push(flag);
                    args.push(value.clone());
                }
            }
            ParameterLocation::Path => {
                if let Some(value) = input.path_params.get(parameter.name) {
                    args.push(flag);
                    args.push(value.clone());
                }
            }
            ParameterLocation::Query => {
                if let Some(values) = input.query_params.get(parameter.name) {
                    for value in values {
                        args.push(flag.clone());
                        args.push(value.clone());
                    }
                }
            }
        }
    }

    if let Some(body) = &input.body {
        args.push("--body".to_string());
        args.push(
            String::from_utf8(body.clone())
                .map_err(|error| other_error(format!("CLI body must be valid UTF-8: {error}")))?,
        );
    }

    Ok(args)
}

fn command_name(operation_id: &str) -> String {
    operation_id.replace('_', "-")
}

fn build_request_body_contract(
    document: &Value,
    request_body: &Value,
) -> TestResult<RequestBodyContract> {
    let request_body = resolve_reference(document, request_body)?;
    let request_body_object = request_body
        .as_object()
        .ok_or_else(|| other_error("request body must resolve to an object"))?;
    let content = request_body_object
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| other_error("request body is missing content"))?;
    let (content_type, media_type) =
        content.iter().next().ok_or_else(|| other_error("request body content is empty"))?;
    let media_type = resolve_reference(document, media_type)?;
    let schema = media_type
        .get("schema")
        .cloned()
        .ok_or_else(|| other_error("request body media type is missing a schema"))?;

    Ok(RequestBodyContract { content_type: content_type.clone(), schema })
}

fn collect_parameters(
    document: &Value,
    parameters: Option<&Value>,
) -> TestResult<Vec<ParameterContract>> {
    let Some(parameters) = parameters else {
        return Ok(Vec::new());
    };
    let parameters =
        parameters.as_array().ok_or_else(|| other_error("parameters must be an array"))?;
    let mut collected = Vec::new();

    for parameter in parameters {
        let parameter = resolve_reference(document, parameter)?;
        let parameter_object = parameter
            .as_object()
            .ok_or_else(|| other_error("parameter must resolve to an object"))?;
        let location = match parameter_object.get("in").and_then(Value::as_str) {
            Some("header") => ParameterLocation::Header,
            Some("path") => ParameterLocation::Path,
            Some("query") => ParameterLocation::Query,
            _ => continue,
        };
        let name = parameter_object
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| other_error("parameter is missing a name"))?;
        let required = parameter_object.get("required").and_then(Value::as_bool).unwrap_or(false);
        let schema =
            parameter_object.get("schema").cloned().unwrap_or_else(|| Value::Object(Map::new()));

        upsert_parameter(
            &mut collected,
            ParameterContract { location, name: name.to_string(), required, schema },
        );
    }

    Ok(collected)
}

fn descriptor_by_operation_id(
    operation_id: &str,
) -> TestResult<&'static GeneratedOperationDescriptor> {
    generated::OPERATION_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.operation_id == operation_id)
        .ok_or_else(|| other_error(format!("missing generated descriptor for {operation_id}")))
}

fn ensure_normalized_contract(manifest_dir: &Path) -> TestResult<PathBuf> {
    let artifacts = contract::generate_artifacts(manifest_dir)?;
    let normalized_contract_path = contract::normalized_contract_path(manifest_dir);

    if let Some(parent) = normalized_contract_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&normalized_contract_path, artifacts.normalized_json)?;

    Ok(normalized_contract_path)
}

fn load_normalized_contract(manifest_dir: &Path) -> TestResult<Value> {
    let normalized_contract_path = contract::normalized_contract_path(manifest_dir);

    if !normalized_contract_path.exists() {
        let _ = ensure_normalized_contract(manifest_dir)?;
    }

    contract::load_contract(&normalized_contract_path).map_err(Into::into)
}

fn lock_or_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_root() -> PathBuf {
    manifest_dir().join("../..")
}

fn other_error(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::other(message.into()))
}

fn prism_bin() -> &'static OsStr {
    OsStr::new("prism")
}

fn prism_log_path(manifest_dir: &Path) -> PathBuf {
    manifest_dir.join("../../target/prism/prism.log")
}

fn run_cli_command<I, S>(args: I) -> TestResult<CliExecution>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut command = Command::new("cargo");
    command
        .arg("run")
        .arg("--quiet")
        .arg("-p")
        .arg("ferriskey-cli")
        .arg("--")
        .current_dir(workspace_root());

    for arg in args {
        command.arg(arg.as_ref());
    }

    let output = command.output()?;
    let status = output.status.code().unwrap_or_default();
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| other_error(format!("CLI stdout was not valid UTF-8: {error}")))?;
    let stderr = String::from_utf8(output.stderr)
        .map_err(|error| other_error(format!("CLI stderr was not valid UTF-8: {error}")))?;

    if !output.status.success() {
        return Err(other_error(format!(
            "CLI command failed with status {status}: {}",
            stderr.trim()
        )));
    }

    let json_output = if stdout.trim_start().starts_with('{') {
        serde_json::from_str(stdout.trim()).map_err(|error| {
            other_error(format!("CLI stdout did not contain valid JSON: {error}"))
        })?
    } else {
        Value::Null
    };

    Ok(CliExecution { json_output, status, stderr, stdout })
}

fn prism_port() -> TestResult<u16> {
    match std::env::var("PRISM_PORT") {
        Ok(value) => value
            .parse::<u16>()
            .map_err(|error| other_error(format!("invalid PRISM_PORT {value}: {error}"))),
        Err(_) => {
            let listener = TcpListener::bind((PRISM_HOST, 0))?;
            let port = listener.local_addr()?.port();
            drop(listener);
            Ok(port)
        }
    }
}

fn read_log_tail(path: &Path, line_count: usize) -> String {
    match fs::read_to_string(path) {
        Ok(contents) => contents
            .lines()
            .rev()
            .take(line_count)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n"),
        Err(_) => String::new(),
    }
}

async fn wait_for_prism_ready(prism: &PrismServer) -> TestResult<()> {
    let client = hpx::Client::new();
    let deadline = Instant::now() + PRISM_READY_TIMEOUT;
    let probe_url = prism.probe_url();

    loop {
        if Instant::now() >= deadline {
            return Err(other_error(format!(
                "Prism did not become ready at {probe_url}. Log tail:\n{}",
                read_log_tail(&prism.log_path, PRISM_LOG_TAIL_LINES)
            )));
        }

        match client.get(&probe_url).send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(_) | Err(_) => std::thread::sleep(PRISM_RETRY_DELAY),
        }
    }
}

fn resolve_reference<'a>(document: &'a Value, value: &'a Value) -> TestResult<&'a Value> {
    match value.get("$ref").and_then(Value::as_str) {
        Some(reference) => {
            let pointer = reference.strip_prefix('#').ok_or_else(|| {
                other_error(format!("unsupported external reference {reference}"))
            })?;
            document
                .pointer(pointer)
                .ok_or_else(|| other_error(format!("failed to resolve JSON pointer {reference}")))
        }
        None => Ok(value),
    }
}

fn synthesize_operation_input(
    document: &Value,
    descriptor: &'static GeneratedOperationDescriptor,
    operation_contract: &OperationContract,
) -> TestResult<OperationInput> {
    let mut input = OperationInput::default();

    for parameter in &operation_contract.parameters {
        if !parameter.required {
            continue;
        }

        match parameter.location {
            ParameterLocation::Header => {
                if parameter.name.eq_ignore_ascii_case("authorization") {
                    continue;
                }
                input.headers.insert(
                    parameter.name.clone(),
                    synthesize_scalar_string(document, &parameter.schema, &parameter.name)?,
                );
            }
            ParameterLocation::Path => {
                input.path_params.insert(
                    parameter.name.clone(),
                    synthesize_scalar_string(document, &parameter.schema, &parameter.name)?,
                );
            }
            ParameterLocation::Query => {
                input.query_params.insert(
                    parameter.name.clone(),
                    synthesize_query_values(document, &parameter.schema, &parameter.name)?,
                );
            }
        }
    }

    if descriptor.request_body.is_some() {
        let request_body = operation_contract.request_body.as_ref().ok_or_else(|| {
            other_error(format!(
                "generated descriptor {} has a request body but the normalized contract metadata is missing it",
                descriptor.operation_id
            ))
        })?;
        let request_value = synthesize_value(document, &request_body.schema, 0)?;

        if request_body.content_type.ends_with("json") ||
            request_body.content_type.contains("+json")
        {
            input.body = Some(serde_json::to_vec(&request_value)?);
        } else if let Some(string_value) = request_value.as_str() {
            input.body = Some(string_value.as_bytes().to_vec());
        } else {
            input.body = Some(serde_json::to_vec(&request_value)?);
        }
    }

    Ok(input)
}

fn synthesize_query_values(
    document: &Value,
    schema: &Value,
    parameter_name: &str,
) -> TestResult<Vec<String>> {
    let schema = resolve_reference(document, schema)?;
    let resolved_type = schema_type(schema);

    if resolved_type == Some("array") {
        let item_schema = schema.get("items").unwrap_or(schema);
        return Ok(vec![synthesize_scalar_string(document, item_schema, parameter_name)?]);
    }

    Ok(vec![synthesize_scalar_string(document, schema, parameter_name)?])
}

fn synthesize_scalar_string(
    document: &Value,
    schema: &Value,
    parameter_name: &str,
) -> TestResult<String> {
    let value = synthesize_value(document, schema, 0)?;

    match value {
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) => {
            Ok(if value.is_empty() { fallback_string(parameter_name).to_string() } else { value })
        }
        Value::Null => Ok(fallback_string(parameter_name).to_string()),
        other => Ok(other.to_string()),
    }
}

fn synthesize_value(document: &Value, schema: &Value, depth: usize) -> TestResult<Value> {
    if depth > 16 {
        return Ok(Value::Null);
    }

    let schema = resolve_reference(document, schema)?;

    if let Some(value) = schema.get("const") {
        return Ok(value.clone());
    }

    if let Some(value) = schema.get("default") &&
        !value.is_null()
    {
        return Ok(value.clone());
    }

    if let Some(values) = schema.get("enum").and_then(Value::as_array) &&
        let Some(value) = values.first()
    {
        return Ok(value.clone());
    }

    if let Some(branches) = schema.get("oneOf").and_then(Value::as_array) &&
        let Some(branch) = branches.first()
    {
        return synthesize_value(document, branch, depth + 1);
    }

    if let Some(branches) = schema.get("anyOf").and_then(Value::as_array) &&
        let Some(branch) = branches.iter().find(|branch| !branch_is_null(branch))
    {
        return synthesize_value(document, branch, depth + 1);
    }

    if let Some(branches) = schema.get("allOf").and_then(Value::as_array) {
        let mut merged = Map::new();

        for branch in branches {
            let synthesized = synthesize_value(document, branch, depth + 1)?;
            if let Value::Object(object) = synthesized {
                merged.extend(object);
            }
        }

        if !merged.is_empty() {
            return Ok(Value::Object(merged));
        }
    }

    match schema_type(schema) {
        Some("array") => {
            let item_schema = schema.get("items").unwrap_or(schema);
            Ok(Value::Array(vec![synthesize_value(document, item_schema, depth + 1)?]))
        }
        Some("boolean") => Ok(Value::Bool(true)),
        Some("integer") => Ok(Value::Number(integer_number(schema))),
        Some("number") => Ok(Value::Number(number_value(schema)?)),
        Some("object") => synthesize_object(document, schema, depth + 1),
        Some("string") => Ok(Value::String(string_value(schema, SAMPLE_STRING))),
        Some(_) | None => {
            if schema.get("properties").is_some() || schema.get("additionalProperties").is_some() {
                synthesize_object(document, schema, depth + 1)
            } else {
                Ok(Value::String(string_value(schema, SAMPLE_STRING)))
            }
        }
    }
}

fn synthesize_object(document: &Value, schema: &Value, depth: usize) -> TestResult<Value> {
    let mut object = Map::new();
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|required| required.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>())
        .unwrap_or_default();

    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (name, property_schema) in properties {
            if required.contains(name.as_str()) {
                object
                    .insert(name.clone(), synthesize_value(document, property_schema, depth + 1)?);
            }
        }
    }

    if object.is_empty() &&
        let Some(additional_properties) = schema.get("additionalProperties") &&
        additional_properties.is_object()
    {
        object.insert(
            SAMPLE_STRING.to_string(),
            synthesize_value(document, additional_properties, depth + 1)?,
        );
    }

    Ok(Value::Object(object))
}

fn upsert_parameter(parameters: &mut Vec<ParameterContract>, parameter: ParameterContract) {
    if let Some(index) = parameters.iter().position(|candidate| {
        candidate.location == parameter.location && candidate.name == parameter.name
    }) {
        parameters[index] = parameter;
    } else {
        parameters.push(parameter);
    }
}

fn validate_decoded_response(
    descriptor: &'static GeneratedOperationDescriptor,
    decoded_response: &DecodedResponse,
) -> TestResult<()> {
    if decoded_response.status != descriptor.primary_success_status {
        return Err(other_error(format!(
            "operation {} returned status {} instead of {}",
            descriptor.operation_id, decoded_response.status, descriptor.primary_success_status
        )));
    }

    if decoded_response.schema_name != descriptor.primary_response_schema {
        return Err(other_error(format!(
            "operation {} decoded schema {:?} instead of {:?}",
            descriptor.operation_id,
            decoded_response.schema_name,
            descriptor.primary_response_schema
        )));
    }

    if descriptor.primary_response_schema.is_some() &&
        decoded_response.json_body().is_none() &&
        !decoded_response.raw_body.is_empty()
    {
        return Err(other_error(format!(
            "operation {} returned a non-empty body that did not decode as JSON",
            descriptor.operation_id
        )));
    }

    Ok(())
}

fn branch_is_null(branch: &Value) -> bool {
    match branch.get("type") {
        Some(Value::String(value)) => value == "null",
        Some(Value::Array(values)) => values.iter().all(|value| value.as_str() == Some("null")),
        _ => false,
    }
}

fn fallback_string(parameter_name: &str) -> &'static str {
    if parameter_name.contains("email") {
        SAMPLE_EMAIL
    } else if parameter_name.contains("realm") {
        SAMPLE_REALM_NAME
    } else if parameter_name.contains("uri") || parameter_name.contains("url") {
        SAMPLE_URI
    } else if parameter_name.contains("uuid") ||
        parameter_name.ends_with("_id") ||
        parameter_name == "id"
    {
        SAMPLE_UUID
    } else {
        SAMPLE_STRING
    }
}

fn integer_number(schema: &Value) -> Number {
    let minimum = schema.get("minimum").and_then(Value::as_i64).unwrap_or(1);

    Number::from(minimum.max(1))
}

fn number_value(schema: &Value) -> TestResult<Number> {
    let minimum = schema.get("minimum").and_then(Value::as_f64).unwrap_or(1.0);

    Number::from_f64(if minimum.is_finite() { minimum.max(1.0) } else { 1.0 })
        .ok_or_else(|| other_error("failed to synthesize a JSON number"))
}

fn schema_type(schema: &Value) -> Option<&str> {
    match schema.get("type") {
        Some(Value::String(value)) => Some(value.as_str()),
        Some(Value::Array(values)) => {
            values.iter().filter_map(Value::as_str).find(|value| *value != "null")
        }
        _ => None,
    }
}

fn string_value(schema: &Value, fallback: &str) -> String {
    if let Some(format) = schema.get("format").and_then(Value::as_str) {
        return match format {
            "date-time" => SAMPLE_DATE_TIME.to_string(),
            "email" => SAMPLE_EMAIL.to_string(),
            "uri" | "url" => SAMPLE_URI.to_string(),
            "uuid" => SAMPLE_UUID.to_string(),
            _ => fallback_string(fallback).to_string(),
        };
    }

    let minimum_length = schema.get("minLength").and_then(Value::as_u64).unwrap_or(1) as usize;
    let seed = fallback_string(fallback);
    let repeated = seed.repeat(minimum_length.max(1));

    repeated.chars().take(minimum_length.max(seed.len())).collect()
}
