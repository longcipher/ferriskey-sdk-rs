use std::collections::{BTreeMap, BTreeSet};

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;

use crate::{
    client::OperationInput,
    error::SdkError,
    generated::{
        GeneratedOperationDescriptor, GeneratedParameterDescriptor, GeneratedResponseDescriptor,
        ParameterLocation,
    },
    transport::{SdkRequest, SdkResponse},
};

/// Decoded response payload returned by the generic SDK pipeline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedResponse {
    /// Response headers preserved as UTF-8 strings when possible.
    pub headers: BTreeMap<String, String>,
    /// Parsed JSON body when the documented response content type is JSON.
    pub json_body: Option<Value>,
    /// Raw response body bytes.
    pub raw_body: Vec<u8>,
    /// Matched response schema name when documented.
    pub schema_name: Option<&'static str>,
    /// HTTP status code returned by the server.
    pub status: u16,
}

impl DecodedResponse {
    /// Access the decoded JSON body when present.
    #[must_use]
    pub const fn json_body(&self) -> Option<&Value> {
        self.json_body.as_ref()
    }
}

pub(crate) fn encode_request(
    descriptor: &'static GeneratedOperationDescriptor,
    input: OperationInput,
) -> Result<SdkRequest, SdkError> {
    let mut headers = input.headers;
    let body = encode_body(descriptor, input.body, &mut headers)?;
    let path = encode_path(descriptor, &input.path_params)?;
    let path = encode_query(descriptor, &path, &input.query_params)?;
    validate_required_headers(descriptor, &headers)?;

    Ok(SdkRequest {
        body,
        headers,
        method: descriptor.method.to_string(),
        path,
        requires_auth: descriptor.requires_auth,
    })
}

pub(crate) fn decode_response(
    descriptor: &'static GeneratedOperationDescriptor,
    response: SdkResponse,
) -> Result<DecodedResponse, SdkError> {
    let matched_response = match_response(descriptor, response.status)?;
    let json_body = decode_json_body(matched_response, &response.body)?;
    let decoded = DecodedResponse {
        headers: response.headers,
        json_body: json_body.clone(),
        raw_body: response.body,
        schema_name: matched_response.schema_name,
        status: matched_response.status,
    };

    if matched_response.is_error {
        return Err(SdkError::ApiResponse {
            body: json_body,
            operation_id: descriptor.operation_id.to_string(),
            schema_name: matched_response.schema_name,
            status: matched_response.status,
        });
    }

    Ok(decoded)
}

fn encode_body(
    descriptor: &'static GeneratedOperationDescriptor,
    body: Option<Vec<u8>>,
    headers: &mut BTreeMap<String, String>,
) -> Result<Option<Vec<u8>>, SdkError> {
    let Some(request_body) = descriptor.request_body else {
        return Ok(None);
    };

    if body.is_none() && request_body.required && !request_body.nullable {
        return Err(SdkError::MissingRequestBody {
            operation_id: descriptor.operation_id.to_string(),
        });
    }

    if body.is_some() &&
        let Some(content_type) = request_body.content_type
    {
        headers.entry("content-type".to_string()).or_insert_with(|| content_type.to_string());
    }

    Ok(body)
}

fn encode_path(
    descriptor: &'static GeneratedOperationDescriptor,
    path_params: &BTreeMap<String, String>,
) -> Result<String, SdkError> {
    let mut encoded_path = descriptor.path.to_string();

    for parameter in descriptor.parameters.iter().filter(is_path_parameter) {
        let value = path_params
            .get(parameter.name)
            .ok_or_else(|| missing_parameter(descriptor, parameter))?;
        let placeholder = format!("{{{}}}", parameter.name);
        encoded_path = encoded_path.replace(&placeholder, &encode_component(value));
    }

    if encoded_path.contains('{') || encoded_path.contains('}') {
        return Err(SdkError::InvalidPathTemplate {
            operation_id: descriptor.operation_id.to_string(),
            path_template: descriptor.path.to_string(),
        });
    }

    Ok(encoded_path)
}

fn encode_query(
    descriptor: &'static GeneratedOperationDescriptor,
    path: &str,
    query_params: &BTreeMap<String, Vec<String>>,
) -> Result<String, SdkError> {
    let known_query_names = descriptor
        .parameters
        .iter()
        .filter(is_query_parameter)
        .map(|parameter| parameter.name)
        .collect::<BTreeSet<_>>();
    let mut encoded_pairs = Vec::new();

    for parameter in descriptor.parameters.iter().filter(is_query_parameter) {
        if parameter.required && !query_params.contains_key(parameter.name) {
            return Err(missing_parameter(descriptor, parameter));
        }

        if let Some(values) = query_params.get(parameter.name) {
            encoded_pairs.extend(values.iter().map(|value| {
                format!("{}={}", encode_component(parameter.name), encode_component(value))
            }));
        }
    }

    for (name, values) in query_params {
        if known_query_names.contains(name.as_str()) {
            continue;
        }

        encoded_pairs.extend(
            values
                .iter()
                .map(|value| format!("{}={}", encode_component(name), encode_component(value))),
        );
    }

    if encoded_pairs.is_empty() {
        return Ok(path.to_string());
    }

    Ok(format!("{path}?{}", encoded_pairs.join("&")))
}

fn validate_required_headers(
    descriptor: &'static GeneratedOperationDescriptor,
    headers: &BTreeMap<String, String>,
) -> Result<(), SdkError> {
    for parameter in descriptor.parameters.iter().filter(is_header_parameter) {
        if parameter.required && !headers.contains_key(parameter.name) {
            return Err(missing_parameter(descriptor, parameter));
        }
    }

    Ok(())
}

fn match_response(
    descriptor: &'static GeneratedOperationDescriptor,
    status: u16,
) -> Result<&'static GeneratedResponseDescriptor, SdkError> {
    descriptor.responses.iter().find(|response| response.status == status).ok_or_else(|| {
        SdkError::UnexpectedStatus { actual: status, expected: descriptor.primary_success_status }
    })
}

fn decode_json_body(
    response: &'static GeneratedResponseDescriptor,
    body: &[u8],
) -> Result<Option<Value>, SdkError> {
    if body.is_empty() {
        return Ok(None);
    }

    let expects_json = response.content_type.map_or_else(
        || response.schema_name.is_some(),
        |content_type| content_type == "application/json" || content_type.ends_with("+json"),
    );

    if !expects_json {
        return Ok(None);
    }

    serde_json::from_slice(body).map(Some).map_err(SdkError::Decode)
}

fn missing_parameter(
    descriptor: &'static GeneratedOperationDescriptor,
    parameter: &'static GeneratedParameterDescriptor,
) -> SdkError {
    SdkError::MissingParameter {
        location: match parameter.location {
            ParameterLocation::Header => "header",
            ParameterLocation::Path => "path",
            ParameterLocation::Query => "query",
        },
        name: parameter.name.to_string(),
        operation_id: descriptor.operation_id.to_string(),
    }
}

fn is_header_parameter(parameter: &&GeneratedParameterDescriptor) -> bool {
    parameter.location == ParameterLocation::Header
}

fn is_path_parameter(parameter: &&GeneratedParameterDescriptor) -> bool {
    parameter.location == ParameterLocation::Path
}

fn is_query_parameter(parameter: &&GeneratedParameterDescriptor) -> bool {
    parameter.location == ParameterLocation::Query
}

fn encode_component(value: &str) -> String {
    utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}
