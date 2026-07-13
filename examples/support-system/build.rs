use std::{fs, path::Path};

fn main() {
    const CONTRACT: &str = "contracts/support-grpc.v1.proto";
    const GENERATED_CLIENT_SOURCE: &str =
        "../../../lenso/crates/lenso-service/fixtures/contracts/v2/support-grpc.v1.proto";
    println!("cargo:rerun-if-changed={CONTRACT}");
    println!("cargo:rerun-if-changed={GENERATED_CLIENT_SOURCE}");

    let authoritative = fs::read_to_string(CONTRACT)
        .expect("authoritative support gRPC Service Contract must be readable");
    let generated_source = fs::read_to_string(GENERATED_CLIENT_SOURCE)
        .expect("generated support client source Contract must be readable");
    assert_eq!(
        normalize(&authoritative),
        normalize(&generated_source),
        "published support client drifted from the authoritative example Contract"
    );

    let mut prost = prost_build::Config::new();
    prost.protoc_executable(
        protoc_bin_vendored::protoc_bin_path().expect("vendored protoc must be available"),
    );
    tonic_prost_build::configure()
        .build_server(false)
        .build_client(false)
        .file_descriptor_set_path(
            Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("support_descriptor.bin"),
        )
        .compile_with_config(prost, &[CONTRACT], &["contracts"])
        .expect("authoritative support gRPC Service Contract must compile");
}

fn normalize(source: &str) -> String {
    source
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}
