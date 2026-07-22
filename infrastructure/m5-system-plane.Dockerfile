FROM rust:1.88-bookworm AS build
WORKDIR /workspace
COPY --from=lenso . .
RUN cargo build --release --locked -p lenso-api -p lenso-migrate

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /workspace/target/release/lenso-api /usr/local/bin/lenso-api
COPY --from=build /workspace/target/release/lenso-migrate /usr/local/bin/lenso-migrate
ENTRYPOINT ["/usr/local/bin/lenso-api"]
