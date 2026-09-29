use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=contract");
    println!("cargo:rerun-if-changed=src/generated.rs");
    lenso_contract_codegen::check_projection(
        Path::new("contract/capability.json"),
        lenso_contract_codegen::ProjectionLanguage::Rust,
        Path::new("src/generated.rs"),
    )
    .expect("run the fixture's verify.py generate command");
}
