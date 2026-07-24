use std::path::Path;

fn main() {
    const CONTRACT: &str = "contracts/support-grpc.v1.proto";
    println!("cargo:rerun-if-changed={CONTRACT}");

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
