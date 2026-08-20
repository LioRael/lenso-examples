use std::{env, fs, path::PathBuf};

fn pascal_case(value: &str) -> String {
    value
        .split(['.', '-', '_', '@'])
        .filter(|part| !part.is_empty() && !part.chars().all(char::is_numeric))
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        })
        .collect()
}

fn object_string_field(schema_path: &str) -> String {
    let schema: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(schema_path).expect("read object Schema"))
            .expect("parse object Schema");
    assert_eq!(
        schema["type"], "object",
        "generated fixture expects an object Schema"
    );
    let properties = schema["properties"].as_object().expect("Schema properties");
    assert_eq!(
        properties.len(),
        1,
        "generated fixture expects one property"
    );
    let (name, shape) = properties.iter().next().expect("one Schema property");
    assert_eq!(
        shape["type"], "string",
        "generated fixture expects a string property"
    );
    assert!(
        schema["required"]
            .as_array()
            .is_some_and(|required| required.iter().any(|item| item == name))
    );
    name.clone()
}

fn main() {
    println!("cargo:rerun-if-changed=capability.json");
    println!("cargo:rerun-if-changed=schemas");
    let descriptor: serde_json::Value = serde_json::from_str(
        &fs::read_to_string("capability.json").expect("read Capability Descriptor"),
    )
    .expect("parse Capability Descriptor");
    let id = descriptor["id"].as_str().expect("Descriptor id");
    let version = descriptor["version"].as_str().expect("Descriptor version");
    let operation = descriptor["operations"][0]["name"]
        .as_str()
        .expect("Operation name");
    assert_eq!(descriptor["operations"][0]["interaction"], "request");
    let operation_descriptor = &descriptor["operations"][0];
    for schema in ["request_schema", "response_schema", "domain_error_schema"] {
        let path = operation_descriptor[schema].as_str().expect("schema path");
        serde_json::from_str::<serde_json::Value>(&fs::read_to_string(path).expect("read Schema"))
            .expect("parse JSON Schema");
        println!("cargo:rerun-if-changed={path}");
    }
    let capability_name = pascal_case(id.split('@').next().unwrap().rsplit('.').next().unwrap());
    let operation_name = pascal_case(operation);
    let request_field =
        object_string_field(operation_descriptor["request_schema"].as_str().unwrap());
    let response_field =
        object_string_field(operation_descriptor["response_schema"].as_str().unwrap());
    let error_schema: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            operation_descriptor["domain_error_schema"]
                .as_str()
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let error_value = error_schema["oneOf"][0]["const"]
        .as_str()
        .expect("Domain Error const");
    let generated = include_str!("src/bindings.template.rs")
        .replace("__CAPABILITY_ID__", id)
        .replace("__DESCRIPTOR_VERSION__", version)
        .replace("__OPERATION__", operation)
        .replace("__OPERATION_FN__", operation)
        .replace("__CAPABILITY__", &capability_name)
        .replace("__OPERATION_TYPE__", &operation_name)
        .replace("__REQUEST_FIELD__", &request_field)
        .replace("__RESPONSE_FIELD__", &response_field)
        .replace("__DOMAIN_ERROR__", &pascal_case(error_value));
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs"),
        generated,
    )
    .expect("write generated Rust bindings");
}
