ARG RUST_BUILDER_IMAGE=rust:bookworm
ARG RUNTIME_IMAGE=debian:bookworm-slim
FROM ${RUST_BUILDER_IMAGE} AS builder
ARG RELEASE_VERSION=development
ENV M5_RELEASE_VERSION=$RELEASE_VERSION
WORKDIR /workspace
COPY --from=lenso . /workspace/lenso
COPY examples/support-system /workspace/examples/examples/support-system
RUN cargo build --locked --release \
    --manifest-path /workspace/examples/examples/support-system/Cargo.toml \
    --bin support-system-m5-data-plane

FROM ${RUNTIME_IMAGE}
ARG RELEASE_VERSION=development
ENV M5_RELEASE_VERSION=$RELEASE_VERSION
LABEL org.opencontainers.image.version=$RELEASE_VERSION
COPY --from=builder \
    /workspace/examples/examples/support-system/target/release/support-system-m5-data-plane \
    /usr/local/bin/support-system-m5-data-plane
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/support-system-m5-data-plane"]
