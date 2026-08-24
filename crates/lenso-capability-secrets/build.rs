use std::path::Path;

use lenso_contract_codegen::generate;

fn main() {
    println!("cargo:rerun-if-changed=capability.json");
    println!("cargo:rerun-if-changed=schemas");
    println!("cargo:rerun-if-changed=src/generated.rs");

    let expected = generate(Path::new("capability.json"))
        .unwrap_or_else(|error| panic!("Capability generation failed: {error}"));
    let actual = std::fs::read_to_string("src/generated.rs")
        .unwrap_or_else(|error| panic!("read Rust projection: {error}"));
    assert_eq!(actual, expected.rust, "Rust projection is stale");
}
