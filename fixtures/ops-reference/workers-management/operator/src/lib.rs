//! Private explicit qualification operator; it does not supply an App Capability.
#[cfg(target_arch = "wasm32")]
mod access;
#[cfg(target_arch = "wasm32")]
mod audit;
#[cfg(any(target_arch = "wasm32", test))]
mod audit_proof;
#[cfg(target_arch = "wasm32")]
mod auth;

#[cfg(target_arch = "wasm32")]
fn unavailable() -> wasm_bindgen::JsValue {
    // Owner exceptions can contain backend details. Only this fixed classification leaves Wasm.
    wasm_bindgen::JsValue::from_str("owner_operator_unavailable_or_rejected")
}

#[cfg(any(target_arch = "wasm32", test))]
fn label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':' | b'/'))
}

#[cfg(test)]
mod tests {
    #[test]
    fn bounded_operator_references_reject_delimiters() {
        assert!(super::label("operator-subject-a"));
        for invalid in ["", "subject\nheader", "subject?query", &"x".repeat(257)] {
            assert!(!super::label(invalid));
        }
    }
}
