# Operations state contract

`src/contract.rs` owns the portable read/update contract. Normal App build/dev
regenerates the Descriptor, schemas and runtime projection. `build.rs` checks
freshness in direct Cargo consumers. The state provider owns CAS, idempotency,
receipts and storage; the contract does not choose its provider or database.
