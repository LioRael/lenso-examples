#[allow(dead_code)]
#[path = "src/contract.rs"]
mod contract;
fn main() {
    println!("cargo:rerun-if-changed=src/contract.rs");
    println!("cargo:rerun-if-changed=capability.json");
    println!("cargo:rerun-if-changed=schemas");
    println!("cargo:rerun-if-changed=src/generated.rs");
    lenso_contract_codegen::check_source_snapshot(
        &contract::__lenso_capability_snapshot(),
        std::path::Path::new("capability.json"),
    )
    .expect("run lenso app build/dev to synchronize the contract");
    lenso_contract_codegen::check_projection(
        std::path::Path::new("capability.json"),
        lenso_contract_codegen::ProjectionLanguage::RustRuntime,
        std::path::Path::new("src/generated.rs"),
    )
    .expect("stale generated projection");
}
