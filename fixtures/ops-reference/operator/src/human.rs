use super::{Input, Result, private_write};
use lenso_access_control_postgres_plugin::AccessControlConfig;
use lenso_audit_log_postgres_plugin::{AuditLogConfig, AuditReadScope};
use lenso_auth_account_plugin::{AccountAuthConfig, AccountAuthOperator};
use lenso_auth_api_token_plugin::{ApiTokenAuthConfig, assertion_public_key};
use lenso_auth_human_api_token_plugin::HumanApiTokenConfig;
use lenso_auth_password_plugin::{PasswordAuthConfig, PasswordAuthOperator};
use lenso_auth_sdk::{audience, credential::ManagementCredentialCeiling};
use lenso_business_approval_postgres_plugin::BusinessApprovalConfig;
use lenso_management_core::Management;
use serde_json::json;

const CORE: &str = "example.ops-management/default";
const TOKENS: &str = "example.ops-human-tokens/default";
const FACADE: &str = "lenso.auth.human-api-token/default";
const BOOTSTRAP: &str = "example.ops-human-bootstrap/default";
const PAT_MANAGEMENT: &str = "example.ops-management/pat";
const WEB: &str = "example.ops-management-web/default";
const AGENT_CONNECTION: &str = "lenso.agent.management-connection/default";

pub async fn upgrade(input: &Input, database: &str) -> Result<()> {
    let path = input.directory.join("human-setup.json");
    let mut prepared: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    let schema = prepared["schemas"]["account"]
        .as_str()
        .ok_or("missing prepared Account schema")?;
    AccountAuthOperator::upgrade(database, schema).await?;
    let account: AccountAuthConfig = serde_json::from_value(prepared["owners"]["account"].clone())?;
    prepared["owners"]["account"] = serde_json::to_value(
        account
            .with_delegation_callers(vec![WEB.into()])?
            .with_scoped_delegation_targets(vec![AGENT_CONNECTION.into()])?,
    )?;
    let audiences = prepared["owners"]["password"]["audience"]
        .as_array_mut()
        .ok_or("missing prepared Password audiences")?;
    for operation in ["grant_scoped", "scoped_receipt"] {
        let value = json!(audience("lenso.auth.delegation@1", operation));
        if !audiences.contains(&value) {
            audiences.push(value);
        }
    }
    let replacement = input.directory.join("human-setup-upgraded.json");
    private_write(&replacement, &serde_json::to_vec_pretty(&prepared)?)?;
    std::fs::rename(
        &path,
        input.directory.join("human-setup-before-upgrade.json"),
    )?;
    std::fs::rename(replacement, path)?;
    Ok(())
}

pub async fn setup(input: &Input, database: &str) -> Result<()> {
    let account_schema = format!("{}_account", input.schemas.auth);
    let password_schema = format!("{}_password", input.schemas.auth);
    let account_issuer = format!("{}.account", input.issuer);
    let account_signing = std::fs::read_to_string(input.directory.join("account-signing.secret"))?;
    let account_public_key = assertion_public_key(&account_signing);
    let api_signing = std::fs::read_to_string(&input.signing_file)?;
    let api_public_key = assertion_public_key(&api_signing);
    AccountAuthOperator::setup(database, &account_schema).await?;
    PasswordAuthOperator::setup(database, &password_schema).await?;
    Management::initialize_journal(&input.directory.join("human-tokens.sqlite"))
        .map_err(|_| "human token journal setup failed")?;
    Management::initialize_journal(&input.directory.join("pat-management.sqlite"))
        .map_err(|_| "PAT management journal setup failed")?;
    let mut ceiling: ManagementCredentialCeiling = super::ceiling(&input.deployment);
    ceiling
        .permissions
        .extend(["auth.pat.issue", "auth.pat.list", "auth.pat.revoke"].map(str::to_owned));
    let account = AccountAuthConfig::new(
        &account_schema,
        &account_issuer,
        &account_public_key,
        "account/database",
        "account/signing",
        "account/pepper",
        60,
    )?
    .with_management_session_ceiling(ceiling)?
    .with_credential_state_callers(vec![CORE.into(), TOKENS.into(), FACADE.into()])?
    .with_delegation_callers(vec![WEB.into()])?
    .with_scoped_delegation_targets(vec![AGENT_CONNECTION.into()])?;
    let password_audiences = [
        ("lenso.management@1", "catalog"),
        ("lenso.management@1", "invoke"),
        ("lenso.management@1", "status"),
        ("lenso.agent.tool-provider@2", "catalog"),
        ("lenso.agent.tool-provider@2", "execute"),
        ("lenso.management-human@1", "read_intent"),
        ("lenso.management-human@1", "decide"),
        ("lenso.business-approval@1", "decide"),
        ("lenso.auth.human-api-token@1", "issue"),
        ("lenso.auth.human-api-token@1", "list"),
        ("lenso.auth.human-api-token@1", "receipt"),
        ("lenso.auth.human-api-token@1", "revoke"),
        ("lenso.auth.credential-issuer@1", "revoke_credential"),
        ("lenso.auth.delegation@1", "grant_scoped"),
        ("lenso.auth.delegation@1", "scoped_receipt"),
    ]
    .map(|(capability, operation)| audience(capability, operation))
    .to_vec();
    let password = PasswordAuthConfig::new(
        &password_schema,
        "password/database",
        password_audiences,
        3600,
        5,
        60,
    )?;
    let api = ApiTokenAuthConfig::new(
        &input.schemas.auth,
        &input.issuer,
        &api_public_key,
        "auth/database",
        "auth/signing",
        "auth/pepper",
        60,
    )?
    .with_credential_state_callers(vec![CORE.into(), PAT_MANAGEMENT.into()])?
    .with_management_callers(vec![FACADE.into()])?;
    let api_access = AccessControlConfig::new(
        &input.schemas.access,
        "access/database",
        &input.issuer,
        &api_public_key,
        vec![BOOTSTRAP.into()],
    )?;
    let human_access = AccessControlConfig::new(
        &input.schemas.access,
        "access/database",
        &account_issuer,
        &account_public_key,
        vec![BOOTSTRAP.into()],
    )?;
    let mut token_audiences = ["catalog", "invoke", "status"]
        .map(|operation| audience("lenso.management@1", operation))
        .to_vec();
    token_audiences.extend(
        ["catalog", "execute"].map(|operation| audience("lenso.agent.tool-provider@2", operation)),
    );
    let facade = HumanApiTokenConfig::new(
        "operators",
        &account_issuer,
        &account_public_key,
        60,
        3600,
        &input.deployment,
        "management-deployment",
        &input.deployment,
        token_audiences,
    )
    .map_err(|_| "human token config invalid")?;
    let approval = BusinessApprovalConfig::new(
        &input.schemas.approval,
        "approval/database",
        vec![CORE.into(), PAT_MANAGEMENT.into()],
        vec![CORE.into(), PAT_MANAGEMENT.into()],
        vec![CORE.into(), PAT_MANAGEMENT.into()],
    )?;
    let audit = AuditLogConfig::new(
        "audit/database",
        vec![CORE.into(), TOKENS.into(), PAT_MANAGEMENT.into()],
        vec![CORE.into(), TOKENS.into(), PAT_MANAGEMENT.into()],
    )?
    .with_reader_scopes(
        CORE,
        vec![AuditReadScope {
            kind: "deployment".into(),
            id: input.deployment.clone(),
        }],
    )?;
    private_write(
        &input.directory.join("human-setup.json"),
        &serde_json::to_vec_pretty(&json!({
            "schema":"lenso.ops-human-operator-setup.v1", "account_issuer":account_issuer,
            "account_public_key":account_public_key, "schemas":{"account":account_schema,"password":password_schema},
            "owners":{"account":account,"password":password,"auth":api,"access":api_access,
                "human_access":human_access,"human_facade":facade,"approval":approval,"audit":audit}
        }))?,
    )?;
    Ok(())
}
