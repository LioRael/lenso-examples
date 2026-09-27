use super::*;
use lenso_capability_http_endpoint::OpenApiContract;
use serde_json::{Map, Value};

fn generated_schema<T: JsonSchema>() -> Value {
    let contract = OpenApiContract::new("schema.check", "GET", "/schema-check")
        .success_json::<T>(200)
        .unwrap()
        .build()
        .unwrap();
    contract["operation"]["responses"]["200"]["content"]["application/json"]["schema"].clone()
}

fn wire_shape(schema: &Value) -> Value {
    wire_shape_with_root(schema, schema)
}

fn wire_shape_with_root(schema: &Value, root: &Value) -> Value {
    let Some(object) = schema.as_object() else {
        return schema.clone();
    };
    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        let name = reference
            .strip_prefix("#/$defs/")
            .expect("public DTO schema has an unsupported reference");
        let definition = &root["$defs"][name];
        assert!(
            !definition.is_null(),
            "missing generated schema definition {name}"
        );
        return wire_shape_with_root(definition, root);
    }
    if let Some(variants) = object.get("anyOf").and_then(Value::as_array)
        && variants.len() == 2
        && let Some(non_null) = variants.iter().find(|variant| variant["type"] != "null")
        && variants.iter().any(|variant| variant["type"] == "null")
    {
        return wire_shape_with_root(non_null, root);
    }
    assert!(
        !object.contains_key("anyOf")
            && !object.contains_key("oneOf")
            && !object.contains_key("allOf"),
        "public DTO schema uses an unsupported composition"
    );

    let mut shape = Map::new();
    if let Some(kind) = object.get("type") {
        let kind = match kind {
            Value::Array(kinds) if kinds.len() == 2 && kinds.contains(&Value::from("null")) => {
                kinds.iter().find(|kind| **kind != "null").unwrap().clone()
            }
            kind => kind.clone(),
        };
        shape.insert("type".to_owned(), kind);
    }
    if let Some(variants) = object.get("enum").and_then(Value::as_array) {
        let mut variants = variants.clone();
        variants.sort_by_key(ToString::to_string);
        shape.insert("enum".to_owned(), Value::Array(variants));
    }
    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        shape.insert(
            "properties".to_owned(),
            Value::Object(
                properties
                    .iter()
                    .map(|(name, schema)| (name.clone(), wire_shape_with_root(schema, root)))
                    .collect(),
            ),
        );
    }
    if let Some(items) = object.get("items") {
        shape.insert("items".to_owned(), wire_shape_with_root(items, root));
    }
    if let Some(required) = object.get("required").and_then(Value::as_array) {
        let mut required = required.clone();
        required.sort_by_key(ToString::to_string);
        shape.insert("required".to_owned(), Value::Array(required));
    }
    Value::Object(shape)
}

fn assert_component<T: JsonSchema>(document: &Value, name: &str) {
    let authored = &document["components"]["schemas"][name];
    assert!(!authored.is_null(), "missing public schema {name}");
    assert_eq!(
        wire_shape(authored),
        wire_shape(&generated_schema::<T>()),
        "public schema {name} drifted from its Rust DTO wire shape"
    );
}

#[test]
fn public_component_wire_shapes_match_typed_dtos() {
    let document: Value =
        serde_json::from_str(include_str!("../../../frontend/openapi.json")).unwrap();

    assert_component::<CreateNote>(&document, "CreateNote");
    assert_component::<Note>(&document, "Note");
    assert_component::<InspectExcerptResponse>(&document, "JobState");
    assert_component::<BusinessSettings>(&document, "BusinessSettings");
    assert_component::<UpdateBusinessSettings>(&document, "UpdateBusinessSettings");
    assert_component::<UploadAttachment>(&document, "UploadAttachment");
    assert_component::<Attachment>(&document, "Attachment");
}

#[test]
fn public_path_parameter_shapes_match_typed_dtos() {
    let document: Value =
        serde_json::from_str(include_str!("../../../frontend/openapi.json")).unwrap();
    for (path, method, name, derived) in [
        (
            "/notes/{note_id}",
            "get",
            "note_id",
            generated_schema::<NotePath>(),
        ),
        (
            "/job-status/{job_id}",
            "get",
            "job_id",
            generated_schema::<JobPath>(),
        ),
        (
            "/note-attachments/{note_id}",
            "post",
            "note_id",
            generated_schema::<NotePath>(),
        ),
    ] {
        let parameter = document["paths"][path][method]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|parameter| parameter["name"] == name)
            .unwrap();
        assert_eq!(parameter["in"], "path");
        assert_eq!(parameter["required"], true);
        assert_eq!(
            wire_shape(&parameter["schema"]),
            wire_shape(&derived["properties"][name]),
            "{method} {path} parameter {name} drifted from its Rust DTO"
        );
    }
}
