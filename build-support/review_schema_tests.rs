//! Check schema optionality and catalogs directly, before frontend generation consumes them.
//! These tests run in the lightweight exporter and require no embedded application build.

use super::*;
use serde_json::{Value, json};

/// Resolve local references produced by Schemars without fetching external schemas.
fn resolve<'a>(root: &'a Value, mut schema: &'a Value) -> &'a Value {
    while let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        schema = root
            .pointer(reference.strip_prefix('#').expect("local schema reference"))
            .expect("schema reference target");
    }
    schema
}

/// Obtain a catalog member's complete schema after resolving its local definition.
fn member<'a>(root: &'a Value, name: &str) -> &'a Value {
    resolve(root, &root["properties"][name])
}

/// Produce response schemas using exactly the Cargo export configuration.
fn response_schema() -> Value {
    serde_json::to_value(
        SchemaSettings::draft07()
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<ResponseContracts>(),
    )
    .unwrap()
}

/// Produce input schemas using serde deserialization rather than output optionality.
fn request_schema() -> Value {
    serde_json::to_value(
        SchemaSettings::draft07()
            .for_deserialize()
            .into_generator()
            .into_root_schema_for::<RequestContracts>(),
    )
    .unwrap()
}

/// Full responses require nullable fields; patches allow absence but do not invent null for arrays.
#[test]
fn response_contract_preserves_required_nullable_and_optional_nonnullable_members() {
    let root = response_schema();
    assert_eq!(root["$schema"], "http://json-schema.org/draft-07/schema#");
    let state = member(&root, "state");
    assert!(
        state["required"]
            .as_array()
            .unwrap()
            .contains(&json!("invocation"))
    );
    assert_eq!(
        state["properties"]["invocation"]["type"],
        json!(["string", "null"])
    );
    let patch = member(&root, "patch");
    assert_eq!(patch["required"], json!(["type", "version"]));
    assert_eq!(
        resolve(&root, &patch["properties"]["images"])["type"],
        "array"
    );
    assert_eq!(
        resolve(&root, &patch["properties"]["invocation"])["type"],
        json!(["string", "null"])
    );
    let job = member(&root, "diffusion_job");
    assert!(
        !job["required"]
            .as_array()
            .unwrap()
            .contains(&json!("source_url"))
    );
    assert!(
        job["required"]
            .as_array()
            .unwrap()
            .contains(&json!("before_url"))
    );
}

/// Defaulted requests remain partial; labels retain serde's accepted duplicate input behavior.
#[test]
fn request_contract_keeps_defaults_unknown_fields_and_duplicate_labels() {
    let root = request_schema();
    let review = member(&root, "review");
    assert_eq!(review["required"], json!(["image_id", "rating", "tags"]));
    assert_ne!(review["additionalProperties"], false);
    let ui = member(&root, "ui");
    assert!(ui.get("required").is_none());
    assert!(ui["properties"]["labels"].get("uniqueItems").is_none());
    let input: ReviewUiUpdateRequest =
        serde_json::from_value(json!({"labels":["red","red"],"future_field":true})).unwrap();
    assert_eq!(input.labels, vec![ReviewLabel::Red, ReviewLabel::Red]);
    let update: ReviewUpdateRequest =
        serde_json::from_value(json!({"image_id":1,"rating":255,"tags":[],"retouch":{"crop":{}}}))
            .unwrap();
    assert_eq!(update.rating, 255);
    let crop = update.retouch.unwrap().crop.unwrap();
    assert_eq!(
        (crop.x, crop.y, crop.width, crop.height),
        (0.0, 0.0, 1.0, 1.0)
    );
    let diffusion: DiffusionSettings = serde_json::from_value(json!({})).unwrap();
    assert_eq!(diffusion, DiffusionSettings::default());
}

/// Every route references exported contracts and only established empty-body routes opt in.
#[test]
fn operation_manifest_covers_all_json_routes_without_phantom_empty_responses() {
    let requests = request_schema();
    let responses = response_schema();
    assert_eq!(OPERATIONS.len(), 18);
    let mut names = std::collections::BTreeSet::new();
    for operation in OPERATIONS {
        assert!(names.insert(operation.name));
        if let Some(request) = operation.request {
            assert!(requests["properties"].get(request).is_some());
        }
        assert!(responses["properties"].get(operation.response).is_some());
        assert_eq!(
            operation.allow_empty_request,
            matches!(
                operation.name,
                "publish" | "panorama_previews" | "panorama_render"
            )
        );
    }
    assert_eq!(
        OPERATIONS
            .iter()
            .filter(|operation| operation.transport == "sse")
            .count(),
        1
    );
}

/// Public EXIF and job schemas must not expose private camera identities or filesystem paths.
#[test]
fn output_schemas_exclude_private_runtime_metadata() {
    let root = response_schema();
    let exif = &root["definitions"]["GalleryExifData"]["properties"];
    for name in [
        "camera_serial",
        "nikon_burst_key",
        "nikon_burst_shot_number",
    ] {
        assert!(exif.get(name).is_none(), "{name}");
    }
    let job = member(&root, "diffusion_job");
    assert!(job["properties"].get("before_path").is_none());
    assert!(job["properties"].get("after_path").is_none());
    assert!(
        root["definitions"]["ReviewProfile"]["properties"]
            .get("hald_path")
            .is_none()
    );
}

/// Exported examples have stable bytes and exercise the actual patch serializer's null clearing.
#[test]
fn response_fixtures_are_deterministic_public_dto_values() {
    let first = serde_json::to_vec(&fixtures::responses()).unwrap();
    assert_eq!(first, serde_json::to_vec(&fixtures::responses()).unwrap());
    let fixtures: Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(fixtures["patch"]["invocation"], Value::Null);
    assert_eq!(
        fixtures["patch"]["images"][0]["notes"],
        "Updated fixture note"
    );
    assert!(fixtures["state"].get("type").is_none());
    assert!(fixtures["diffusion_job"].get("source_url").is_none());
}
