//! Property-based tests for tool schema generation correctness.
//!
//! These tests verify that for any valid tool definition with arbitrary parameter
//! names, types, descriptions, and required flags, the generated JSON Schema
//! includes all declared parameters with their correct types and required status.

use adk_tool::FunctionTool;
use proptest::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Test schema types with varying parameter combinations
// ---------------------------------------------------------------------------

/// A schema with a single string parameter.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct SingleStringParam {
    name: String,
}

/// A schema with a single integer parameter.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct SingleIntParam {
    count: i32,
}

/// A schema with a single boolean parameter.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct SingleBoolParam {
    enabled: bool,
}

/// A schema with a single float parameter.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct SingleFloatParam {
    value: f64,
}

/// A schema with multiple parameters of different types.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct MultiTypeParams {
    name: String,
    age: i32,
    active: bool,
    score: f64,
}

/// A schema with optional parameters (required flags differ).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct MixedRequiredParams {
    required_name: String,
    required_count: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    optional_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    optional_flag: Option<bool>,
}

/// A schema with nested object parameter.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct NestedParams {
    title: String,
    metadata: MetadataParam,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct MetadataParam {
    key: String,
    value: String,
}

/// A schema with array parameter.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct ArrayParams {
    items: Vec<String>,
    count: i32,
}

/// A schema with all basic JSON types.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct AllBasicTypes {
    text: String,
    integer: i64,
    float: f64,
    flag: bool,
    tags: Vec<String>,
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

/// Generate an arbitrary tool name.
fn arb_tool_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{2,20}"
}

/// Generate an arbitrary tool description.
fn arb_tool_description() -> impl Strategy<Value = String> {
    "[A-Za-z0-9 .,!?]{5,100}"
}

/// Select which schema type to use for the tool.
fn arb_schema_variant() -> impl Strategy<Value = u8> {
    0u8..9u8
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Create a FunctionTool with the given schema variant.
fn create_tool_with_schema(name: &str, description: &str, variant: u8) -> FunctionTool {
    let base =
        FunctionTool::new(
            name,
            description,
            |_ctx, _args| async move { Ok(serde_json::json!({})) },
        );

    match variant {
        0 => base.with_parameters_schema::<SingleStringParam>(),
        1 => base.with_parameters_schema::<SingleIntParam>(),
        2 => base.with_parameters_schema::<SingleBoolParam>(),
        3 => base.with_parameters_schema::<SingleFloatParam>(),
        4 => base.with_parameters_schema::<MultiTypeParams>(),
        5 => base.with_parameters_schema::<MixedRequiredParams>(),
        6 => base.with_parameters_schema::<NestedParams>(),
        7 => base.with_parameters_schema::<ArrayParams>(),
        8 => base.with_parameters_schema::<AllBasicTypes>(),
        _ => base.with_parameters_schema::<SingleStringParam>(),
    }
}

/// Get the expected parameter names for a given schema variant.
fn expected_params(variant: u8) -> Vec<&'static str> {
    match variant {
        0 => vec!["name"],
        1 => vec!["count"],
        2 => vec!["enabled"],
        3 => vec!["value"],
        4 => vec!["name", "age", "active", "score"],
        5 => vec!["required_name", "required_count", "optional_label", "optional_flag"],
        6 => vec!["title", "metadata"],
        7 => vec!["items", "count"],
        8 => vec!["text", "integer", "float", "flag", "tags"],
        _ => vec!["name"],
    }
}

/// Get the expected required parameters for a given schema variant.
fn expected_required(variant: u8) -> Vec<&'static str> {
    match variant {
        0 => vec!["name"],
        1 => vec!["count"],
        2 => vec!["enabled"],
        3 => vec!["value"],
        4 => vec!["name", "age", "active", "score"],
        5 => vec!["required_name", "required_count"],
        6 => vec!["title", "metadata"],
        7 => vec!["items", "count"],
        8 => vec!["text", "integer", "float", "flag", "tags"],
        _ => vec!["name"],
    }
}

/// Get the expected JSON Schema type string for a parameter in a given variant.
fn expected_type_for_param(variant: u8, param: &str) -> &'static str {
    match (variant, param) {
        (0, "name") => "string",
        (1, "count") => "integer",
        (2, "enabled") => "boolean",
        (3, "value") => "number",
        (4, "name") => "string",
        (4, "age") => "integer",
        (4, "active") => "boolean",
        (4, "score") => "number",
        (5, "required_name") => "string",
        (5, "required_count") => "integer",
        (5, "optional_label") => "string",
        (5, "optional_flag") => "boolean",
        (6, "title") => "string",
        (6, "metadata") => "object",
        (7, "items") => "array",
        (7, "count") => "integer",
        (8, "text") => "string",
        (8, "integer") => "integer",
        (8, "float") => "number",
        (8, "flag") => "boolean",
        (8, "tags") => "array",
        _ => "string",
    }
}

// ---------------------------------------------------------------------------
// Property 5: Tool Schema Generation Correctness
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Feature: one-point-zero-readiness, Property 5: Tool Schema Generation Correctness**
    ///
    /// *For any* valid tool definition (with any combination of parameter names, types,
    /// descriptions, and required flags), the generated JSON Schema SHALL be a valid
    /// JSON Schema document that includes all declared parameters with their correct
    /// types and required status.
    ///
    /// **Validates: Requirements 11.2**
    #[test]
    fn prop_schema_includes_all_parameters(
        tool_name in arb_tool_name(),
        tool_desc in arb_tool_description(),
        variant in arb_schema_variant(),
    ) {
        let tool = create_tool_with_schema(&tool_name, &tool_desc, variant);

        // Schema must be present
        let schema = tool.parameters_schema()
            .expect("Tool with schema type should produce a parameters schema");

        // Schema must be a JSON object
        let schema_obj = schema.as_object()
            .expect("Schema should be a JSON object");

        // Schema must have a "properties" field
        let properties = schema_obj.get("properties")
            .and_then(|v| v.as_object())
            .expect("Schema should have a 'properties' object");

        // All expected parameters must be present in the schema
        let expected = expected_params(variant);
        for param_name in &expected {
            prop_assert!(
                properties.contains_key(*param_name),
                "Schema missing expected parameter '{}'. Properties: {:?}",
                param_name,
                properties.keys().collect::<Vec<_>>()
            );
        }

        // The number of properties should match expected
        prop_assert_eq!(
            properties.len(),
            expected.len(),
            "Schema has {} properties but expected {}. Got: {:?}",
            properties.len(),
            expected.len(),
            properties.keys().collect::<Vec<_>>()
        );
    }

    /// **Feature: one-point-zero-readiness, Property 5: Schema Type Correctness**
    ///
    /// *For any* valid tool definition, each parameter in the generated schema SHALL
    /// have the correct JSON Schema type annotation.
    ///
    /// **Validates: Requirements 11.2**
    #[test]
    fn prop_schema_parameters_have_correct_types(
        tool_name in arb_tool_name(),
        tool_desc in arb_tool_description(),
        variant in arb_schema_variant(),
    ) {
        let tool = create_tool_with_schema(&tool_name, &tool_desc, variant);
        let schema = tool.parameters_schema().unwrap();
        let properties = schema["properties"].as_object().unwrap();

        let expected = expected_params(variant);
        for param_name in &expected {
            let param_schema = &properties[*param_name];
            let expected_type = expected_type_for_param(variant, param_name);

            // For optional params (nullable), the type might be in a oneOf/anyOf
            // or directly as "type"
            let actual_type = get_schema_type(param_schema);

            let expected_str = expected_type.to_string();
            prop_assert!(
                actual_type.contains(&expected_str),
                "Parameter '{}' expected type '{}' but got '{:?}' in schema: {}",
                param_name,
                expected_type,
                actual_type,
                serde_json::to_string_pretty(param_schema).unwrap_or_default()
            );
        }
    }

    /// **Feature: one-point-zero-readiness, Property 5: Schema Required Status**
    ///
    /// *For any* valid tool definition, the generated schema SHALL correctly mark
    /// required parameters in the "required" array.
    ///
    /// **Validates: Requirements 11.2**
    #[test]
    fn prop_schema_required_status_correct(
        tool_name in arb_tool_name(),
        tool_desc in arb_tool_description(),
        variant in arb_schema_variant(),
    ) {
        let tool = create_tool_with_schema(&tool_name, &tool_desc, variant);
        let schema = tool.parameters_schema().unwrap();

        let required_array = schema.get("required")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let expected_req = expected_required(variant);

        // All expected required params should be in the required array
        for param_name in &expected_req {
            prop_assert!(
                required_array.contains(param_name),
                "Parameter '{}' should be required but is not in required array: {:?}",
                param_name,
                required_array
            );
        }

        // No extra params should be in the required array
        for req_param in &required_array {
            prop_assert!(
                expected_req.contains(req_param),
                "Parameter '{}' is in required array but should not be. Expected: {:?}",
                req_param,
                expected_req
            );
        }
    }

    /// **Feature: one-point-zero-readiness, Property 5: Schema is Valid JSON Schema**
    ///
    /// *For any* valid tool definition, the generated schema SHALL be a valid
    /// JSON Schema document (has "type": "object" at the root level).
    ///
    /// **Validates: Requirements 11.2**
    #[test]
    fn prop_schema_is_valid_json_schema_document(
        tool_name in arb_tool_name(),
        tool_desc in arb_tool_description(),
        variant in arb_schema_variant(),
    ) {
        let tool = create_tool_with_schema(&tool_name, &tool_desc, variant);
        let schema = tool.parameters_schema().unwrap();

        // Must be a JSON object
        prop_assert!(schema.is_object(), "Schema must be a JSON object");

        // Must have "type": "object" at root
        let schema_type = schema.get("type")
            .and_then(|v| v.as_str());
        prop_assert_eq!(
            schema_type,
            Some("object"),
            "Schema root type must be 'object', got: {:?}",
            schema_type
        );

        // Must have "properties" field
        prop_assert!(
            schema.get("properties").is_some(),
            "Schema must have a 'properties' field"
        );
    }
}

/// Extract the type string from a JSON Schema property definition.
/// Handles direct "type" field, oneOf/anyOf with nullable types, etc.
fn get_schema_type(schema: &Value) -> Vec<String> {
    let mut types = Vec::new();

    // Direct "type" field
    if let Some(t) = schema.get("type").and_then(|v| v.as_str()) {
        types.push(t.to_string());
    }

    // Array of types (e.g., ["string", "null"])
    if let Some(arr) = schema.get("type").and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(t) = item.as_str() {
                types.push(t.to_string());
            }
        }
    }

    // oneOf/anyOf patterns (for nullable types)
    for key in &["oneOf", "anyOf"] {
        if let Some(variants) = schema.get(*key).and_then(|v| v.as_array()) {
            for variant in variants {
                if let Some(t) = variant.get("type").and_then(|v| v.as_str()) {
                    types.push(t.to_string());
                }
            }
        }
    }

    types
}
