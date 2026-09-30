//! Removable browser PAT guard. The Auth facade owns credentials; this wrapper owns qualification and audit.
use lenso_capability_access_control as access;
use lenso_capability_audit_log as audit;
use lenso_capability_business_approval as approval;
use lenso_capability_credential_state as credentials;
use lenso_capability_human_api_token as pat;
use lenso_kernel::{InvocationContext, NativeRequestFuture};
use lenso_management_authority::{HumanPatServiceProvider, OperatorsAuthority, OwnerPorts};
use lenso_management_core::Management;
use lenso_ops_reference_management::{host_facilities::AuthorityHandle, operation_policies};
use pat::HumanApiTokenProvider as _;
use std::{cell::RefCell, rc::Rc};

pub mod host_facilities {
    pub fn authority(
        binding: &serde_json::Value,
        clock: &lenso_native_adapter::NativeHostClock,
    ) -> Result<super::AuthorityHandle, lenso::RuntimeFailure> {
        lenso_ops_reference_management::host_facilities::authority(binding, clock)
    }
}
#[derive(Clone, Debug, serde::Deserialize, lenso::PluginConfig)]
#[serde(deny_unknown_fields)]
struct Config {
    deployment: String,
}
#[lenso::plugin]
#[derive(Clone, Debug)]
struct HumanTokens {
    #[config]
    config: Config,
    #[facility(id = "authority")]
    host: AuthorityHandle,
    #[dependency(id = "credential_state")]
    credential_state: credentials::CredentialStateClient,
    #[dependency(id = "access")]
    access: access::AccessControlClient,
    #[dependency(id = "approval")]
    approval: approval::BusinessApprovalClient,
    #[dependency(id = "audit")]
    audit: audit::AuditLogClient,
    #[dependency(id = "human_token_owner")]
    owner: pat::HumanApiTokenClient,
    provider: Rc<RefCell<Option<HumanPatServiceProvider>>>,
}
fn unavailable() -> lenso::RuntimeFailure {
    lenso::RuntimeFailure::PluginFailure {
        detail: "Human token profile is unavailable".into(),
    }
}
#[lenso::plugin_impl]
impl HumanTokens {
    #[create]
    fn create(
        config: Config,
        host: AuthorityHandle,
        credential_state: credentials::CredentialStateClient,
        access: access::AccessControlClient,
        approval: approval::BusinessApprovalClient,
        audit: audit::AuditLogClient,
        owner: pat::HumanApiTokenClient,
    ) -> Result<Self, &'static str> {
        let wrapper = Self {
            config,
            host,
            credential_state,
            access,
            approval,
            audit,
            owner,
            provider: Rc::new(RefCell::new(None)),
        };
        wrapper
            .initialize()
            .map_err(|_| "Human token complete-object construction was rejected")?;
        Ok(wrapper)
    }

    #[stop]
    fn stop(&self) {
        self.provider.replace(None);
    }
}
impl HumanTokens {
    fn initialize(&self) -> Result<(), lenso::RuntimeFailure> {
        if self.config.deployment != self.host.deployment || self.provider.borrow().is_some() {
            return Err(unavailable());
        }
        let authority = Rc::new(
            OperatorsAuthority::new(
                self.host.verifier.clone(),
                self.host.qualification.clone(),
                operation_policies(),
                OwnerPorts {
                    credential_state: self.credential_state.clone(),
                    access: self.access.clone(),
                    approval: self.approval.clone(),
                    audit: self.audit.clone(),
                },
                self.host.clock.clone(),
            )
            .map_err(|_| unavailable())?,
        );
        let journal = Rc::new(
            Management::open(
                &self.host.journal,
                self.config.deployment.clone(),
                "human-tokens-v1".into(),
                Vec::new(),
                authority.clone(),
            )
            .map_err(|_| unavailable())?,
        );
        let provider = HumanPatServiceProvider::new(
            authority,
            self.config.deployment.clone(),
            self.owner.clone(),
            journal,
        )
        .map_err(|_| unavailable())?;
        self.provider.replace(Some(provider));
        Ok(())
    }
}
#[lenso::provides(pat::HumanApiToken)]
impl HumanTokens {
    fn issue(
        &self,
        context: InvocationContext,
        request: pat::IssueRequest,
    ) -> NativeRequestFuture<pat::HumanApiTokenIssue> {
        self.provider.borrow().as_ref().cloned().map_or_else(
            || {
                Box::pin(std::future::ready(Err(unavailable())))
                    as NativeRequestFuture<pat::HumanApiTokenIssue>
            },
            |provider| provider.issue(context, request),
        )
    }
    fn list(
        &self,
        context: InvocationContext,
        request: pat::ListRequest,
    ) -> NativeRequestFuture<pat::HumanApiTokenList> {
        self.provider.borrow().as_ref().cloned().map_or_else(
            || {
                Box::pin(std::future::ready(Err(unavailable())))
                    as NativeRequestFuture<pat::HumanApiTokenList>
            },
            |provider| provider.list(context, request),
        )
    }
    fn receipt(
        &self,
        context: InvocationContext,
        request: pat::ReceiptRequest,
    ) -> NativeRequestFuture<pat::HumanApiTokenReceipt> {
        self.provider.borrow().as_ref().cloned().map_or_else(
            || {
                Box::pin(std::future::ready(Err(unavailable())))
                    as NativeRequestFuture<pat::HumanApiTokenReceipt>
            },
            |provider| provider.receipt(context, request),
        )
    }
    fn revoke(
        &self,
        context: InvocationContext,
        request: pat::RevokeRequest,
    ) -> NativeRequestFuture<pat::HumanApiTokenRevoke> {
        self.provider.borrow().as_ref().cloned().map_or_else(
            || {
                Box::pin(std::future::ready(Err(unavailable())))
                    as NativeRequestFuture<pat::HumanApiTokenRevoke>
            },
            |provider| provider.revoke(context, request),
        )
    }
}
