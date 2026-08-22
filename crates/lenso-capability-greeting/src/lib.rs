//! Generated Rust bindings for the portable Greeting Capability Descriptor.

include!("generated.rs");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_values_round_trip_through_rust_wire_codecs() {
        let request = GreetRequest {
            name: "Ada".to_owned(),
        };
        let wire = encode_greet_request(&request).expect("request should encode");
        assert_eq!(
            decode_greet_request(&wire).expect("request should decode"),
            request
        );

        let error =
            decode_greet_error(r#"{"code":"future_variant","payload":{"retry_after_ms":2500}}"#)
                .expect("unknown Domain Error should decode");
        assert_eq!(
            error,
            GreetError::Unknown(UnknownDomainError {
                code: "future_variant".to_owned(),
                payload: Some(serde_json::json!({"retry_after_ms": 2500})),
                extra: std::collections::BTreeMap::new(),
            })
        );
        assert_eq!(
            encode_greet_error(&error).expect("unknown Domain Error should encode"),
            r#"{"code":"future_variant","payload":{"retry_after_ms":2500}}"#
        );
    }
}
