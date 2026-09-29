use std::rc::Rc;

use futures::future::LocalBoxFuture;
use lenso::{Ctx, PluginError, PluginResult, RuntimeFailure};

use contract::*;
pub use example_settings_contract as contract;

#[cfg(not(target_arch = "wasm32"))]
pub mod postgres;

pub fn failure(detail: impl std::fmt::Display) -> RuntimeFailure {
    RuntimeFailure::PluginFailure {
        detail: detail.to_string(),
    }
}
pub type ReadResult = Result<Result<ReadResponse, ReadError>, RuntimeFailure>;
pub type ChangeResult = Result<Result<ChangeResponse, ChangeError>, RuntimeFailure>;

/// A private persistence resource, never an ambient or public Capability.
pub trait Persistence: std::fmt::Debug + 'static {
    fn read(&self, request: ReadRequest) -> LocalBoxFuture<'static, ReadResult>;
    fn change(&self, request: ChangeRequest) -> LocalBoxFuture<'static, ChangeResult>;
}

pub fn valid_principal(principal: &str) -> bool {
    !principal.is_empty()
        && principal.len() <= 64
        && principal
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub fn valid_change(request: &ChangeRequest) -> bool {
    valid_principal(&request.principal)
        && (0..=2_147_483_646).contains(&request.expected_revision)
        && !request.value.is_empty()
        && request.value.len() <= 256
        && !request.value.chars().any(char::is_control)
        && !request.idempotency_key.is_empty()
        && request.idempotency_key.len() <= 64
        && request
            .idempotency_key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Evaluated while holding the principal's database row lock. A replay returns
/// the original receipt even if later writes changed the current revision.
pub fn decide(
    current: &ReadResponse,
    receipt: Option<(&ChangeRequest, &ChangeResponse)>,
    request: &ChangeRequest,
) -> Result<ChangeResponse, ChangeError> {
    if !valid_change(request) {
        return Err(ChangeError::Invalid);
    }
    if let Some((original, response)) = receipt {
        return if original == request {
            Ok(response.clone())
        } else {
            Err(ChangeError::IdempotencyConflict)
        };
    }
    if current.revision != request.expected_revision {
        return Err(ChangeError::Conflict);
    }
    Ok(ChangeResponse {
        revision: current.revision + 1,
        value: request.value.clone(),
    })
}

#[lenso::plugin(lifecycle)]
#[derive(Clone, Debug)]
pub struct Store {
    pub persistence: Option<Rc<dyn Persistence>>,
}

impl lenso::Lifecycle for Store {
    async fn activate(&self, _: lenso::ActivateContext) -> Result<(), RuntimeFailure> {
        self.persistence
            .as_ref()
            .map(|_| ())
            .ok_or_else(|| failure("persistence was not attached by Host"))
    }
}

#[lenso::provides(contract::SettingsStore)]
impl Store {
    async fn read(&self, _: Ctx, request: ReadRequest) -> PluginResult<ReadResponse, ReadError> {
        if !valid_principal(&request.principal) {
            return Err(PluginError::Domain(ReadError::Invalid));
        }
        self.persistence
            .as_ref()
            .ok_or_else(|| PluginError::Runtime(failure("persistence was not attached by Host")))?
            .read(request)
            .await
            .map_err(PluginError::Runtime)?
            .map_err(PluginError::Domain)
    }

    async fn change(
        &self,
        _: Ctx,
        request: ChangeRequest,
    ) -> PluginResult<ChangeResponse, ChangeError> {
        if !valid_change(&request) {
            return Err(PluginError::Domain(ChangeError::Invalid));
        }
        self.persistence
            .as_ref()
            .ok_or_else(|| PluginError::Runtime(failure("persistence was not attached by Host")))?
            .change(request)
            .await
            .map_err(PluginError::Runtime)?
            .map_err(PluginError::Domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change() -> ChangeRequest {
        ChangeRequest {
            principal: "local-test".into(),
            expected_revision: 0,
            idempotency_key: "first".into(),
            value: "value".into(),
        }
    }

    #[test]
    fn original_receipt_wins_over_newer_current_revision() {
        let request = change();
        let original = ChangeResponse {
            revision: 1,
            value: "value".into(),
        };
        let current = ReadResponse {
            revision: 9,
            value: "later".into(),
        };
        assert_eq!(
            decide(&current, Some((&request, &original)), &request),
            Ok(original.clone())
        );
        assert_eq!(
            decide(
                &current,
                Some((&request, &original)),
                &ChangeRequest {
                    value: "different".into(),
                    ..request.clone()
                }
            ),
            Err(ChangeError::IdempotencyConflict),
        );
    }

    #[test]
    fn invalid_changes_cannot_reach_the_transaction() {
        for invalid in [
            ChangeRequest {
                principal: String::new(),
                ..change()
            },
            ChangeRequest {
                expected_revision: -1,
                ..change()
            },
            ChangeRequest {
                expected_revision: 2_147_483_647,
                ..change()
            },
            ChangeRequest {
                value: "x".repeat(257),
                ..change()
            },
            ChangeRequest {
                value: "\n".into(),
                ..change()
            },
            ChangeRequest {
                idempotency_key: "has spaces".into(),
                ..change()
            },
        ] {
            assert!(!valid_change(&invalid));
        }
    }

    #[test]
    fn unknown_domain_error_remains_distinct_from_runtime_failure() {
        let error = decode_change_error(r#""future_rejection""#).unwrap();
        assert!(matches!(error, ChangeError::Unknown(_)));
        assert_eq!(
            decode_change_error(&encode_change_error(&error).unwrap()).unwrap(),
            error
        );
    }
}
