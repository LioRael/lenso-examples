use std::{env, fs, path::PathBuf};

fn main() {
    let descriptor_path =
        PathBuf::from("../../crates/lenso-capability-secure-greeting/capability.json");
    println!("cargo:rerun-if-changed={}", descriptor_path.display());
    let client = lenso_contract_codegen::generate_browser_request_client(&descriptor_path)
        .expect("generate Secure Greeting browser client");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("secure-greeting-client.js"),
        client,
    )
    .expect("write generated browser client");
}
