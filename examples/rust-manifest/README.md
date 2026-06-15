# Rust Manifest Example

This example shows the published `lenso` Rust facade from a standalone module
authoring crate.

Run it from the repository root:

```sh
cargo run --manifest-path examples/rust-manifest/Cargo.toml
```

It builds a schema-admin manifest, validates it with `lint_module_manifest`,
and prints the manifest JSON that a host can inspect.
