use example_ops_state_contract::{
    self as state, ReadRequest, ReadResponse, ReceiptRequest, ReceiptResponse, UpdateRequest,
    UpdateResponse,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

pub mod host_facilities;
use host_facilities::StateHandle;

#[derive(Clone, Debug, Deserialize, lenso::PluginConfig)]
#[serde(deny_unknown_fields)]
struct StateConfig {
    label: String,
    initial_value: i64,
    profile: String,
}

fn validate(config: &StateConfig) -> Result<(), lenso::RuntimeFailure> {
    if !matches!(
        config.profile.as_str(),
        "simulated" | "native-pg" | "workers-d1"
    ) || config.label.is_empty()
        || config.initial_value.unsigned_abs() > 9_007_199_254_740_991
    {
        return Err(lenso::RuntimeFailure::InvalidResolvedPlan {
            detail: "this provider requires an explicit supported profile and nonempty label"
                .into(),
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
struct MemoryState {
    value: Option<i64>,
    revision: i64,
    receipts: BTreeMap<String, (String, UpdateResponse)>,
}

#[lenso::plugin(validate = validate, lifecycle)]
#[derive(Clone, Debug)]
struct OpsState {
    #[config]
    config: StateConfig,
    #[facility(id = "state")]
    backend: StateHandle,
    state: Rc<RefCell<MemoryState>>,
}

#[cfg(test)]
mod simulated_tests;

impl lenso::Lifecycle for OpsState {
    async fn prepare(&self, _context: lenso::PrepareContext) -> Result<(), lenso::RuntimeFailure> {
        if self.config.profile != self.backend.profile() {
            return Err(lenso::RuntimeFailure::InvalidResolvedPlan {
                detail: "Ops State configuration and Host facility profiles differ".into(),
            });
        }
        self.backend.readiness().await?;
        if self.config.profile != "simulated"
            && self
                .backend
                .read()
                .await
                .map_err(|_| lenso::RuntimeFailure::PluginFailure {
                    detail: "Ops State setup required".into(),
                })?
                .label
                != self.config.label
        {
            return Err(lenso::RuntimeFailure::InvalidResolvedPlan {
                detail: "Ops State facility points at a different configured state".into(),
            });
        }
        Ok(())
    }

    async fn deactivate(
        &self,
        _context: lenso::DeactivateContext,
    ) -> Result<(), lenso::RuntimeFailure> {
        self.backend.close().await;
        Ok(())
    }
}

#[lenso::provides(state::OpsState)]
impl OpsState {
    async fn receipt(
        &self,
        _context: lenso::Ctx,
        request: ReceiptRequest,
    ) -> Result<ReceiptResponse, state::ReceiptError> {
        if self.config.profile != "simulated" {
            return self.backend.receipt(request).await;
        }
        if request.idempotency_key.is_empty()
            || request.idempotency_key.len() > 128
            || !request
                .idempotency_key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(state::ReceiptError::InvalidInput);
        }
        Ok(
            match self.state.borrow().receipts.get(&request.idempotency_key) {
                Some((_, response)) => ReceiptResponse {
                    found: true,
                    label: Some(Some(response.label.clone())),
                    value: Some(Some(response.value)),
                    revision: Some(Some(response.revision)),
                    receipt_id: Some(Some(response.receipt_id.clone())),
                },
                None => ReceiptResponse {
                    found: false,
                    label: None,
                    value: None,
                    revision: None,
                    receipt_id: None,
                },
            },
        )
    }

    async fn read(
        &self,
        _context: lenso::Ctx,
        _request: ReadRequest,
    ) -> Result<ReadResponse, state::ReadError> {
        if self.config.profile != "simulated" {
            return self.backend.read().await;
        }
        let state = self.state.borrow();
        Ok(ReadResponse {
            label: self.config.label.clone(),
            value: state.value.unwrap_or(self.config.initial_value),
            revision: state.revision,
        })
    }

    async fn update(
        &self,
        _context: lenso::Ctx,
        request: UpdateRequest,
    ) -> Result<UpdateResponse, state::UpdateError> {
        if self.config.profile != "simulated" {
            return self.backend.update(request).await;
        }
        if request.value.unsigned_abs() > 9_007_199_254_740_991
            || !(0..9_007_199_254_740_991).contains(&request.expected_revision)
            || request.idempotency_key.is_empty()
            || request.idempotency_key.len() > 128
            || !request
                .idempotency_key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(state::UpdateError::InvalidInput);
        }
        let intent = format!(
            "ops-state.v1\n{}\n{}",
            request.expected_revision, request.value
        );
        let digest = format!("{:x}", Sha256::digest(intent.as_bytes()));
        let mut state = self.state.borrow_mut();
        if let Some((prior_digest, receipt)) = state.receipts.get(&request.idempotency_key) {
            return if *prior_digest == digest {
                Ok(receipt.clone())
            } else {
                Err(state::UpdateError::IdempotencyConflict)
            };
        }
        if request.expected_revision != state.revision {
            return Err(state::UpdateError::StaleRevision);
        }
        let revision = state
            .revision
            .checked_add(1)
            .ok_or(state::UpdateError::Unavailable)?;
        let receipt = UpdateResponse {
            label: self.config.label.clone(),
            value: request.value,
            revision,
            receipt_id: format!("{}:{revision}:{digest}", self.config.label),
        };
        state.value = Some(request.value);
        state.revision = revision;
        state
            .receipts
            .insert(request.idempotency_key, (digest, receipt.clone()));
        Ok(receipt)
    }
}
