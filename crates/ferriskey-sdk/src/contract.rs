use std::{
    collections::{BTreeMap, BTreeSet},
    env::VarError,
    fs, io,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value, json};
use thiserror::Error;

const HTTP_METHODS: [&str; 8] =
    ["delete", "get", "head", "options", "patch", "post", "put", "trace"];
const SYNTHETIC_SERVER_URL: &str = "http://127.0.0.1:4010";
const AUTHORIZATION_SCHEME_NAME: &str = "Authorization";
const BEARER_SCOPE_NAME: &str = "Bearer";

/// Errors raised while normalizing or generating FerrisKey contract artifacts.
#[derive(Debug, Error)]
pub enum ContractError {
    /// Environment variable access failed.
    #[error("failed to read environment variable: {0}")]
    Env(#[from] VarError),
    /// File-system access failed.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// JSON parsing or serialization failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// The OpenAPI document is structurally invalid for this generator.
    #[error("invalid OpenAPI document: {0}")]
    InvalidDocument(String),
}

/// Supported OpenAPI parameter locations during registry generation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ParameterLocationSeed {
    /// Header parameter.
    Header,
    /// Path parameter.
    Path,
    /// Query parameter.
    Query,
}

/// Parameter descriptor seed before rendering the generated metadata module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterDescriptorSeed {
    /// Parameter location in the HTTP request.
    pub location: ParameterLocationSeed,
    /// Parameter name from the OpenAPI document.
    pub name: String,
    /// Whether the parameter is required.
    pub required: bool,
    /// Parameter description from the OpenAPI document.
    pub description: Option<String>,
}

/// Request-body descriptor seed before rendering the generated metadata module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestBodyDescriptorSeed {
    /// Preferred request content type.
    pub content_type: Option<String>,
    /// Whether the request body is nullable.
    pub nullable: bool,
    /// Whether the request body is required.
    pub required: bool,
    /// Referenced schema name when present.
    pub schema_name: Option<String>,
}

/// Response descriptor seed before rendering the generated metadata module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseDescriptorSeed {
    /// Preferred response content type.
    pub content_type: Option<String>,
    /// Whether the response represents an error status.
    pub is_error: bool,
    /// Referenced schema name when present.
    pub schema_name: Option<String>,
    /// HTTP status code documented for the response.
    pub status: u16,
}

/// Operation descriptor seed before rendering the generated metadata module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationDescriptorSeed {
    /// Whether the operation accepts a request body.
    pub has_request_body: bool,
    /// HTTP method.
    pub method: String,
    /// Unique operation identifier.
    pub operation_id: String,
    /// Parameter descriptors for the operation.
    pub parameters: Vec<ParameterDescriptorSeed>,
    /// Path template from the contract.
    pub path: String,
    /// Primary success response schema when present.
    pub primary_response_schema: Option<String>,
    /// Primary success status code.
    pub primary_success_status: u16,
    /// Request-body descriptor when present.
    pub request_body: Option<RequestBodyDescriptorSeed>,
    /// Whether the operation requires authorization.
    pub requires_auth: bool,
    /// Documented response descriptors.
    pub responses: Vec<ResponseDescriptorSeed>,
    /// Primary API tag.
    pub tag: String,
    /// Short summary from the OpenAPI document.
    pub summary: Option<String>,
    /// Detailed description from the OpenAPI document.
    pub description: Option<String>,
}

/// Normalized contract registry used by code generation and verification tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractRegistry {
    /// Number of documented operations.
    pub operation_count: usize,
    /// Generated operation descriptors.
    pub operations: Vec<OperationDescriptorSeed>,
    /// Number of documented paths.
    pub path_count: usize,
    /// Number of documented schemas.
    pub schema_count: usize,
    /// Generated schema names.
    pub schemas: Vec<String>,
    /// Generated tag names.
    pub tags: Vec<String>,
}

/// Build-time artifacts produced from the normalized FerrisKey contract.
#[derive(Clone, Debug)]
pub struct GeneratedArtifacts {
    /// Pretty-printed normalized contract JSON.
    pub normalized_json: String,
    /// Generated contract registry metadata.
    pub registry: ContractRegistry,
}

/// Resolve the source FerrisKey contract path from a crate manifest directory.
pub fn source_contract_path(manifest_dir: &Path) -> PathBuf {
    manifest_dir.join("../../docs/openai.json")
}

/// Resolve the normalized Prism contract path from a crate manifest directory.
pub fn normalized_contract_path(manifest_dir: &Path) -> PathBuf {
    manifest_dir.join("../../target/prism/openai.prism.json")
}

/// Generate normalized contract artifacts and the in-memory registry.
pub fn generate_artifacts(manifest_dir: &Path) -> Result<GeneratedArtifacts, ContractError> {
    let source_document = load_contract(&source_contract_path(manifest_dir))?;
    let normalized_document = normalize_contract(&source_document)?;
    let registry = build_registry(&normalized_document)?;
    let normalized_json = serde_json::to_string_pretty(&normalized_document)?;

    Ok(GeneratedArtifacts { normalized_json, registry })
}

/// Load a JSON contract document from disk.
pub fn load_contract(path: &Path) -> Result<Value, ContractError> {
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(ContractError::Json)
}

/// Normalize the FerrisKey OpenAPI document for generation and Prism use.
pub fn normalize_contract(source_document: &Value) -> Result<Value, ContractError> {
    let mut normalized_document = source_document.clone();
    let all_tags = collect_operation_tags(source_document)?;
    let requires_authorization = operation_requires_authorization(source_document)?;

    ensure_servers(&mut normalized_document)?;
    ensure_complete_root_tags(&mut normalized_document, &all_tags)?;

    if requires_authorization {
        ensure_authorization_security_scheme(&mut normalized_document)?;
    }

    Ok(normalized_document)
}

/// Build a contract registry from a normalized OpenAPI document.
pub fn build_registry(document: &Value) -> Result<ContractRegistry, ContractError> {
    let paths = top_level_object(document, "paths")?;
    let schema_names = schema_names(document)?;
    let tags = collect_operation_tags(document)?;
    let mut operations = Vec::new();

    for (path, path_item) in paths {
        let path_item_object = path_item.as_object().ok_or_else(|| {
            ContractError::InvalidDocument(format!("path item for {path} must be an object"))
        })?;

        for method in HTTP_METHODS {
            let Some(operation) = path_item_object.get(method) else {
                continue;
            };
            let operation_object = operation.as_object().ok_or_else(|| {
                ContractError::InvalidDocument(format!(
                    "operation {method} {path} must be an object"
                ))
            })?;

            let operation_id = string_field(operation_object, "operationId")?;
            let tag = first_operation_tag(operation_object)?;
            let parameters = collect_parameter_descriptors(path_item_object, operation_object)?;
            let request_body = request_body_descriptor(operation_object)?;
            let responses = response_descriptors(operation_object)?;
            let primary_success_status = primary_success_status(&responses)?;
            let primary_response_schema = responses
                .iter()
                .find(|response| response.status == primary_success_status)
                .and_then(|response| response.schema_name.clone());
            let requires_auth = operation_has_authorization(operation_object)?;
            let summary =
                operation_object.get("summary").and_then(Value::as_str).map(ToOwned::to_owned);
            let description =
                operation_object.get("description").and_then(Value::as_str).map(ToOwned::to_owned);

            operations.push(OperationDescriptorSeed {
                has_request_body: request_body.is_some(),
                method: method.to_ascii_uppercase(),
                operation_id,
                parameters,
                path: path.clone(),
                primary_response_schema,
                primary_success_status,
                request_body,
                requires_auth,
                responses,
                tag,
                summary,
                description,
            });
        }
    }

    operations.sort_by(|left, right| left.operation_id.cmp(&right.operation_id));

    Ok(ContractRegistry {
        operation_count: operations.len(),
        operations,
        path_count: paths.len(),
        schema_count: schema_names.len(),
        schemas: schema_names,
        tags,
    })
}

/// Render the generated Rust metadata module for the normalized contract registry.
pub fn render_generated_module(registry: &ContractRegistry) -> String {
    let tag_modules = registry
        .tags
        .iter()
        .map(|tag| {
            let operation_ids = registry
                .operations
                .iter()
                .filter(|operation| operation.tag == *tag)
                .map(|operation| format!("            {:?}", operation.operation_id))
                .collect::<Vec<_>>()
                .join(",\n");

            format!(
                "    #[doc = \"Generated operation identifiers for this FerrisKey API tag.\"]\n    pub mod {} {{\n        /// Operation identifiers grouped under this API tag.\n        pub const OPERATION_IDS: &[&str] = &[\n{}\n        ];\n    }}",
                sanitize_module_identifier(tag),
                operation_ids,
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let tag_names =
        registry.tags.iter().map(|tag| format!("    {:?}", tag)).collect::<Vec<_>>().join(",\n");
    let schema_names = registry
        .schemas
        .iter()
        .map(|schema| format!("        {:?}", schema))
        .collect::<Vec<_>>()
        .join(",\n");
    let operation_descriptors =
        registry.operations.iter().map(render_operation_descriptor).collect::<Vec<_>>().join(",\n");
    let schema_aliases = registry
        .schemas
        .iter()
        .map(|schema| {
            format!(
                "    #[doc = \"Generated schema alias from the FerrisKey OpenAPI document.\"]\n    pub type {} = serde_json::Value;",
                sanitize_identifier(schema, true)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "/// Generated parameter location metadata.\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub enum ParameterLocation {{\n    /// Header-bound parameter.\n    Header,\n    /// Path-bound parameter.\n    Path,\n    /// Query-bound parameter.\n    Query,\n}}\n\n/// Generated parameter descriptor metadata.\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub struct GeneratedParameterDescriptor {{\n    /// Parameter location in the HTTP request.\n    pub location: ParameterLocation,\n    /// Parameter name from the contract.\n    pub name: &'static str,\n    /// Whether the parameter is required by the contract.\n    pub required: bool,\n    /// Parameter description from the contract.\n    pub description: Option<&'static str>,\n}}\n\n/// Generated request-body descriptor metadata.\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub struct GeneratedRequestBodyDescriptor {{\n    /// Preferred request content type.\n    pub content_type: Option<&'static str>,\n    /// Whether the request body is nullable.\n    pub nullable: bool,\n    /// Whether the request body is required.\n    pub required: bool,\n    /// Referenced schema name when present.\n    pub schema_name: Option<&'static str>,\n}}\n\n/// Generated response descriptor metadata.\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub struct GeneratedResponseDescriptor {{\n    /// Preferred response content type.\n    pub content_type: Option<&'static str>,\n    /// Whether the response represents an error status.\n    pub is_error: bool,\n    /// Referenced schema name when present.\n    pub schema_name: Option<&'static str>,\n    /// HTTP status code documented for the response.\n    pub status: u16,\n}}\n\n/// Generated operation descriptor metadata.\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub struct GeneratedOperationDescriptor {{\n    /// Unique operation identifier.\n    pub operation_id: &'static str,\n    /// HTTP method.\n    pub method: &'static str,\n    /// Path template from the contract.\n    pub path: &'static str,\n    /// Primary API tag.\n    pub tag: &'static str,\n    /// Whether the operation accepts a request body.\n    pub has_request_body: bool,\n    /// Short summary from the contract.\n    pub summary: Option<&'static str>,\n    /// Detailed description from the contract.\n    pub description: Option<&'static str>,\n    /// Schema name for the primary success response when present.\n    pub primary_response_schema: Option<&'static str>,\n    /// Primary success status code.\n    pub primary_success_status: u16,\n    /// Contract parameter descriptors.\n    pub parameters: &'static [GeneratedParameterDescriptor],\n    /// Contract request-body descriptor.\n    pub request_body: Option<GeneratedRequestBodyDescriptor>,\n    /// Whether the operation requires authorization.\n    pub requires_auth: bool,\n    /// Documented response descriptors.\n    pub responses: &'static [GeneratedResponseDescriptor],\n}}\n\n/// Number of documented paths in the normalized contract.\npub const PATH_COUNT: usize = {};\n/// Number of documented operations in the normalized contract.\npub const OPERATION_COUNT: usize = {};\n/// Number of documented schemas in the normalized contract.\npub const SCHEMA_COUNT: usize = {};\n/// Ordered tag names derived from the normalized contract.\npub const TAG_NAMES: &[&str] = &[\n{}\n];\n/// Generated operation descriptors derived from the normalized contract.\npub const OPERATION_DESCRIPTORS: &[GeneratedOperationDescriptor] = &[\n{}\n];\n\n/// Generated schema aliases derived from the normalized contract.\npub mod models {{\n    /// Ordered schema names derived from the normalized contract.\n    pub const SCHEMA_NAMES: &[&str] = &[\n{}\n    ];\n\n{}\n}}\n\n/// Generated tag groupings derived from the normalized contract.\npub mod tags {{\n{}\n}}\n",
        registry.path_count,
        registry.operation_count,
        registry.schema_count,
        tag_names,
        operation_descriptors,
        schema_names,
        schema_aliases,
        tag_modules,
    )
}

fn render_operation_descriptor(operation: &OperationDescriptorSeed) -> String {
    format!(
        "    GeneratedOperationDescriptor {{ operation_id: {:?}, method: {:?}, path: {:?}, tag: {:?}, has_request_body: {}, summary: {}, description: {}, primary_response_schema: {}, primary_success_status: {}, parameters: &[{}], request_body: {}, requires_auth: {}, responses: &[{}] }}",
        operation.operation_id,
        operation.method,
        operation.path,
        operation.tag,
        operation.has_request_body,
        render_option_str(operation.summary.as_deref()),
        render_option_str(operation.description.as_deref()),
        render_option_str(operation.primary_response_schema.as_deref()),
        operation.primary_success_status,
        render_parameter_descriptors(&operation.parameters),
        render_request_body_descriptor(operation.request_body.as_ref()),
        operation.requires_auth,
        render_response_descriptors(&operation.responses),
    )
}

fn render_parameter_descriptors(parameters: &[ParameterDescriptorSeed]) -> String {
    parameters
        .iter()
        .map(|parameter| {
            format!(
                "GeneratedParameterDescriptor {{ location: ParameterLocation::{}, name: {:?}, required: {}, description: {} }}",
                render_parameter_location(parameter.location),
                parameter.name,
                parameter.required,
                render_option_str(parameter.description.as_deref()),
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_request_body_descriptor(request_body: Option<&RequestBodyDescriptorSeed>) -> String {
    match request_body {
        Some(request_body) => format!(
            "Some(GeneratedRequestBodyDescriptor {{ content_type: {}, nullable: {}, required: {}, schema_name: {} }})",
            render_option_str(request_body.content_type.as_deref()),
            request_body.nullable,
            request_body.required,
            render_option_str(request_body.schema_name.as_deref()),
        ),
        None => "None".to_string(),
    }
}

fn render_response_descriptors(responses: &[ResponseDescriptorSeed]) -> String {
    responses
        .iter()
        .map(|response| {
            format!(
                "GeneratedResponseDescriptor {{ content_type: {}, is_error: {}, schema_name: {}, status: {} }}",
                render_option_str(response.content_type.as_deref()),
                response.is_error,
                render_option_str(response.schema_name.as_deref()),
                response.status,
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_option_str(value: Option<&str>) -> String {
    match value {
        Some(value) => format!("Some({value:?})"),
        None => "None".to_string(),
    }
}

const fn render_parameter_location(location: ParameterLocationSeed) -> &'static str {
    match location {
        ParameterLocationSeed::Header => "Header",
        ParameterLocationSeed::Path => "Path",
        ParameterLocationSeed::Query => "Query",
    }
}

fn primary_success_status(responses: &[ResponseDescriptorSeed]) -> Result<u16, ContractError> {
    responses
        .iter()
        .find(|response| response.status >= 200 && response.status < 300)
        .map(|response| response.status)
        .or_else(|| responses.first().map(|response| response.status))
        .ok_or_else(|| {
            ContractError::InvalidDocument(
                "operation responses must contain at least one numeric status code".to_string(),
            )
        })
}

fn collect_parameter_descriptors(
    path_item_object: &Map<String, Value>,
    operation_object: &Map<String, Value>,
) -> Result<Vec<ParameterDescriptorSeed>, ContractError> {
    let mut parameters = BTreeMap::new();

    for value in [path_item_object.get("parameters"), operation_object.get("parameters")]
        .into_iter()
        .flatten()
    {
        let parameter_array = value.as_array().ok_or_else(|| {
            ContractError::InvalidDocument("operation parameters must be an array".to_string())
        })?;

        for parameter in parameter_array {
            let parameter_object = parameter.as_object().ok_or_else(|| {
                ContractError::InvalidDocument("parameter entry must be an object".to_string())
            })?;
            let name = string_field(parameter_object, "name")?;
            let location = parse_parameter_location(&string_field(parameter_object, "in")?)?;
            let required = match location {
                ParameterLocationSeed::Path => true,
                _ => parameter_object.get("required").and_then(Value::as_bool).unwrap_or(false),
            };

            let description =
                parameter_object.get("description").and_then(Value::as_str).map(ToOwned::to_owned);

            parameters.insert(
                (location, name.clone()),
                ParameterDescriptorSeed { location, name, required, description },
            );
        }
    }

    Ok(parameters.into_values().collect())
}

fn parse_parameter_location(value: &str) -> Result<ParameterLocationSeed, ContractError> {
    match value {
        "header" => Ok(ParameterLocationSeed::Header),
        "path" => Ok(ParameterLocationSeed::Path),
        "query" => Ok(ParameterLocationSeed::Query),
        other => {
            Err(ContractError::InvalidDocument(format!("unsupported parameter location: {other}")))
        }
    }
}

fn request_body_descriptor(
    operation_object: &Map<String, Value>,
) -> Result<Option<RequestBodyDescriptorSeed>, ContractError> {
    let Some(request_body_value) = operation_object.get("requestBody") else {
        return Ok(None);
    };
    let request_body_object = request_body_value.as_object().ok_or_else(|| {
        ContractError::InvalidDocument("requestBody must be an object".to_string())
    })?;
    let content =
        request_body_object.get("content").and_then(Value::as_object).ok_or_else(|| {
            ContractError::InvalidDocument("requestBody.content must be an object".to_string())
        })?;
    let (content_type, media_type) = preferred_media_type(content).ok_or_else(|| {
        ContractError::InvalidDocument("requestBody.content cannot be empty".to_string())
    })?;

    Ok(Some(RequestBodyDescriptorSeed {
        content_type: Some(content_type.to_string()),
        nullable: schema_nullable(media_type)?,
        required: request_body_object.get("required").and_then(Value::as_bool).unwrap_or(false),
        schema_name: schema_name_from_media_type(media_type)?,
    }))
}

fn response_descriptors(
    operation_object: &Map<String, Value>,
) -> Result<Vec<ResponseDescriptorSeed>, ContractError> {
    let responses_value = operation_object.get("responses").ok_or_else(|| {
        ContractError::InvalidDocument("operation responses are required".to_string())
    })?;
    let responses = responses_value.as_object().ok_or_else(|| {
        ContractError::InvalidDocument("operation responses must be an object".to_string())
    })?;
    let mut descriptors = responses
        .iter()
        .filter_map(|(status, response)| {
            status.parse::<u16>().ok().map(|status| (status, response))
        })
        .map(|(status, response)| {
            let response_object = response.as_object().ok_or_else(|| {
                ContractError::InvalidDocument("response entry must be an object".to_string())
            })?;
            let media = response_object.get("content").and_then(Value::as_object);
            let (content_type, media_type) = media
                .and_then(preferred_media_type)
                .map_or((None, None), |(content_type, media_type)| {
                    (Some(content_type.to_string()), Some(media_type))
                });

            Ok(ResponseDescriptorSeed {
                content_type,
                is_error: status >= 400,
                schema_name: media_type.map(schema_name_from_media_type).transpose()?.flatten(),
                status,
            })
        })
        .collect::<Result<Vec<_>, ContractError>>()?;

    descriptors.sort_by_key(|response| response.status);
    Ok(descriptors)
}

fn preferred_media_type(content: &Map<String, Value>) -> Option<(&str, &Map<String, Value>)> {
    content
        .iter()
        .find(|(content_type, _)| is_json_content_type(content_type))
        .or_else(|| content.iter().next())
        .and_then(|(content_type, media_type)| {
            media_type.as_object().map(|media_type| (content_type.as_str(), media_type))
        })
}

fn schema_nullable(media_type: &Map<String, Value>) -> Result<bool, ContractError> {
    let Some(schema) = media_type.get("schema") else {
        return Ok(false);
    };
    let schema_object = schema
        .as_object()
        .ok_or_else(|| ContractError::InvalidDocument("schema must be an object".to_string()))?;

    if schema_object.get("nullable").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(true);
    }

    match schema_object.get("type") {
        Some(Value::Array(items)) => Ok(items.iter().any(|item| item.as_str() == Some("null"))),
        _ => Ok(false),
    }
}

fn schema_name_from_media_type(
    media_type: &Map<String, Value>,
) -> Result<Option<String>, ContractError> {
    let Some(schema) = media_type.get("schema") else {
        return Ok(None);
    };
    let schema_object = schema
        .as_object()
        .ok_or_else(|| ContractError::InvalidDocument("schema must be an object".to_string()))?;
    let Some(reference) = schema_object.get("$ref").and_then(Value::as_str) else {
        return Ok(None);
    };

    Ok(reference.strip_prefix("#/components/schemas/").map(ToOwned::to_owned))
}

fn is_json_content_type(content_type: &str) -> bool {
    content_type == "application/json" || content_type.ends_with("+json")
}

fn collect_operation_tags(document: &Value) -> Result<Vec<String>, ContractError> {
    let paths = top_level_object(document, "paths")?;
    let mut tags = BTreeSet::new();

    for path_item in paths.values() {
        let path_item_object = path_item.as_object().ok_or_else(|| {
            ContractError::InvalidDocument("path item must be an object".to_string())
        })?;

        for method in HTTP_METHODS {
            let Some(operation) = path_item_object.get(method) else {
                continue;
            };
            let operation_object = operation.as_object().ok_or_else(|| {
                ContractError::InvalidDocument("operation must be an object".to_string())
            })?;
            tags.insert(first_operation_tag(operation_object)?);
        }
    }

    Ok(tags.into_iter().collect())
}

fn ensure_servers(document: &mut Value) -> Result<(), ContractError> {
    let object = root_object_mut(document)?;
    let needs_servers = match object.get("servers") {
        None => true,
        Some(Value::Array(servers)) => servers.is_empty(),
        Some(_) => false,
    };

    if needs_servers {
        object.insert("servers".to_string(), json!([{ "url": SYNTHETIC_SERVER_URL }]));
    }

    Ok(())
}

fn ensure_complete_root_tags(
    document: &mut Value,
    operation_tags: &[String],
) -> Result<(), ContractError> {
    let object = root_object_mut(document)?;
    let mut existing_tags = BTreeMap::new();

    if let Some(tags_value) = object.get("tags") {
        let tags_array = tags_value.as_array().ok_or_else(|| {
            ContractError::InvalidDocument("top-level tags must be an array".to_string())
        })?;
        for tag_value in tags_array {
            let tag_object = tag_value.as_object().ok_or_else(|| {
                ContractError::InvalidDocument("tag entry must be an object".to_string())
            })?;
            let name = string_field(tag_object, "name")?;
            existing_tags.insert(name, tag_value.clone());
        }
    }

    let normalized_tags = operation_tags
        .iter()
        .map(|tag| existing_tags.get(tag).cloned().unwrap_or_else(|| json!({ "name": tag })))
        .collect::<Vec<_>>();

    object.insert("tags".to_string(), Value::Array(normalized_tags));
    Ok(())
}

fn ensure_authorization_security_scheme(document: &mut Value) -> Result<(), ContractError> {
    let object = root_object_mut(document)?;
    let components =
        object.entry("components".to_string()).or_insert_with(|| Value::Object(Map::new()));
    let components_object = components.as_object_mut().ok_or_else(|| {
        ContractError::InvalidDocument("components must be an object".to_string())
    })?;
    let security_schemes = components_object
        .entry("securitySchemes".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let security_schemes_object = security_schemes.as_object_mut().ok_or_else(|| {
        ContractError::InvalidDocument("components.securitySchemes must be an object".to_string())
    })?;

    security_schemes_object.entry(AUTHORIZATION_SCHEME_NAME.to_string()).or_insert_with(|| {
        json!({
            "type": "http",
            "scheme": "bearer",
            "bearerFormat": "JWT"
        })
    });

    Ok(())
}

fn operation_requires_authorization(document: &Value) -> Result<bool, ContractError> {
    let paths = top_level_object(document, "paths")?;

    for path_item in paths.values() {
        let path_item_object = path_item.as_object().ok_or_else(|| {
            ContractError::InvalidDocument("path item must be an object".to_string())
        })?;
        for method in HTTP_METHODS {
            let Some(operation) = path_item_object.get(method) else {
                continue;
            };
            let operation_object = operation.as_object().ok_or_else(|| {
                ContractError::InvalidDocument("operation must be an object".to_string())
            })?;
            if operation_has_authorization(operation_object)? {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn operation_has_authorization(
    operation_object: &Map<String, Value>,
) -> Result<bool, ContractError> {
    let Some(security_value) = operation_object.get("security") else {
        return Ok(false);
    };
    let security_array = security_value.as_array().ok_or_else(|| {
        ContractError::InvalidDocument("operation security must be an array".to_string())
    })?;

    for security_entry in security_array {
        let security_object = security_entry.as_object().ok_or_else(|| {
            ContractError::InvalidDocument("security entry must be an object".to_string())
        })?;
        if let Some(scopes_value) = security_object.get(AUTHORIZATION_SCHEME_NAME) {
            let scopes = scopes_value.as_array().ok_or_else(|| {
                ContractError::InvalidDocument(
                    "security scheme scopes must be an array".to_string(),
                )
            })?;
            if scopes.iter().any(|scope| scope.as_str() == Some(BEARER_SCOPE_NAME)) {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn schema_names(document: &Value) -> Result<Vec<String>, ContractError> {
    let components = top_level_object(document, "components")?;
    let schemas_value = components.get("schemas").ok_or_else(|| {
        ContractError::InvalidDocument("components.schemas is required".to_string())
    })?;
    let schemas_object = schemas_value.as_object().ok_or_else(|| {
        ContractError::InvalidDocument("components.schemas must be an object".to_string())
    })?;
    let mut names = schemas_object.keys().cloned().collect::<Vec<_>>();
    names.sort();
    Ok(names)
}

fn first_operation_tag(operation_object: &Map<String, Value>) -> Result<String, ContractError> {
    let tags_value = operation_object
        .get("tags")
        .ok_or_else(|| ContractError::InvalidDocument("operation tags are required".to_string()))?;
    let tags = tags_value.as_array().ok_or_else(|| {
        ContractError::InvalidDocument("operation tags must be an array".to_string())
    })?;
    let Some(first_tag) = tags.first().and_then(Value::as_str) else {
        return Err(ContractError::InvalidDocument(
            "operation tag list must contain a string".to_string(),
        ));
    };

    Ok(first_tag.to_string())
}

fn root_object_mut(document: &mut Value) -> Result<&mut Map<String, Value>, ContractError> {
    document.as_object_mut().ok_or_else(|| {
        ContractError::InvalidDocument("OpenAPI document must be a JSON object".to_string())
    })
}

fn top_level_object<'a>(
    document: &'a Value,
    field: &str,
) -> Result<&'a Map<String, Value>, ContractError> {
    let object = document.as_object().ok_or_else(|| {
        ContractError::InvalidDocument("OpenAPI document must be a JSON object".to_string())
    })?;
    let value = object.get(field).ok_or_else(|| {
        ContractError::InvalidDocument(format!("missing top-level field: {field}"))
    })?;

    value.as_object().ok_or_else(|| {
        ContractError::InvalidDocument(format!("top-level field {field} must be an object"))
    })
}

fn string_field(object: &Map<String, Value>, field: &str) -> Result<String, ContractError> {
    let value = object.get(field).ok_or_else(|| {
        ContractError::InvalidDocument(format!("missing required string field: {field}"))
    })?;

    value
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| ContractError::InvalidDocument(format!("field {field} must be a string")))
}

fn sanitize_identifier(raw: &str, pascal_case: bool) -> String {
    let mut segments = raw
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_string())
        .collect::<Vec<_>>();

    if segments.is_empty() {
        return if pascal_case {
            "GeneratedValue".to_string()
        } else {
            "generated_value".to_string()
        };
    }

    if pascal_case {
        let mut identifier = String::new();

        for segment in &mut segments {
            let mut characters = segment.chars();
            let Some(first_character) = characters.next() else {
                continue;
            };
            identifier.push(first_character.to_ascii_uppercase());
            identifier.push_str(&characters.as_str().to_ascii_lowercase());
        }

        if identifier.chars().next().is_some_and(|character| character.is_ascii_digit()) {
            identifier.insert(0, '_');
        }

        identifier
    } else {
        let mut identifier = segments
            .iter_mut()
            .enumerate()
            .map(|(index, segment)| {
                if index == 0 {
                    segment.to_ascii_lowercase()
                } else {
                    let mut characters = segment.chars();
                    let Some(first_character) = characters.next() else {
                        return String::new();
                    };
                    let remainder = characters.as_str().to_ascii_lowercase();
                    format!("{}{}", first_character.to_ascii_uppercase(), remainder)
                }
            })
            .collect::<String>();

        if identifier.chars().next().is_some_and(|character| character.is_ascii_digit()) {
            identifier.insert(0, '_');
        }

        identifier
    }
}

fn sanitize_module_identifier(raw: &str) -> String {
    let mut identifier = String::new();
    let mut previous_was_separator = true;

    for character in raw.chars() {
        if !character.is_ascii_alphanumeric() {
            if !identifier.is_empty() && !identifier.ends_with('_') {
                identifier.push('_');
            }
            previous_was_separator = true;
            continue;
        }

        if character.is_ascii_uppercase() && !previous_was_separator && !identifier.ends_with('_') {
            identifier.push('_');
        }

        identifier.push(character.to_ascii_lowercase());
        previous_was_separator = false;
    }

    while identifier.ends_with('_') {
        identifier.pop();
    }

    if identifier.is_empty() {
        return "generated_value".to_string();
    }

    if identifier.chars().next().is_some_and(|character| character.is_ascii_digit()) {
        identifier.insert(0, '_');
    }

    identifier
}
