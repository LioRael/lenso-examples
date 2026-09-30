//! Explicit disposable qualification setup; the running App never migrates owner stores.
use lenso_access_control_postgres_plugin::{AccessControlConfig, AccessControlOperator};
use lenso_audit_log_postgres_plugin::{AuditLogConfig, AuditLogOperator, AuditReadScope};
use lenso_auth_api_token_plugin::{ApiTokenAuthConfig, ApiTokenAuthOperator, assertion_public_key};
use lenso_auth_sdk::credential::{ManagementCredentialCeiling, ManagementResourceScope};
use lenso_business_approval_postgres_plugin::{BusinessApprovalConfig, BusinessApprovalOperator};
use lenso_management_authority::QualificationStore;
use lenso_management_core::Management;
use serde::Deserialize;
use serde_json::json;
use std::{
    error::Error,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
mod human;
mod tokens;
const CALLER: &str = "example.ops-management/default";
const BOOTSTRAP: &str = "example.ops-bootstrap/default";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Schemas {
    auth: String,
    access: String,
    approval: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    database_uri_file: PathBuf,
    audit_uri_file: PathBuf,
    directory: PathBuf,
    schemas: Schemas,
    deployment: String,
    issuer: String,
    signing_file: PathBuf,
    pepper_file: PathBuf,
    resource_uri: String,
}
fn private_write(path: &Path, value: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(value)?;
    file.sync_all()?;
    std::fs::File::open(path.parent().ok_or("private file has no parent")?)?.sync_all()?;
    Ok(())
}
fn ceiling(deployment: &str) -> ManagementCredentialCeiling {
    ManagementCredentialCeiling {
        deployment: deployment.into(),
        permissions: vec![
            "ops.state.read".into(),
            "ops.state.update".into(),
            "management.approval.read".into(),
            "management.approval.decide".into(),
        ],
        resource_scopes: vec![
            ManagementResourceScope {
                kind: "ops-state".into(),
                id: "primary-state".into(),
            },
            ManagementResourceScope {
                kind: "management-deployment".into(),
                id: deployment.into(),
            },
        ],
    }
}
async fn setup(input: &Input, database: &str) -> Result<()> {
    let audit_database = std::fs::read_to_string(&input.audit_uri_file)?;
    let signing = std::fs::read_to_string(&input.signing_file)?;
    ApiTokenAuthOperator::setup(database, &input.schemas.auth).await?;
    AccessControlOperator::setup(database, &input.schemas.access).await?;
    BusinessApprovalOperator::setup(database, &input.schemas.approval).await?;
    AuditLogOperator::setup(audit_database.trim()).await?;
    Management::initialize_journal(&input.directory.join("management.sqlite"))
        .map_err(|_| "journal setup failed")?;
    QualificationStore::initialize(&input.directory.join("qualification.sqlite"))
        .map_err(|_| "qualification setup failed")?;
    let qualification = QualificationStore::open(&input.directory.join("qualification.sqlite"))
        .map_err(|_| "qualification unavailable")?;
    for subject in ["alice", "bob"] {
        qualification
            .grant(&input.deployment, subject)
            .map_err(|_| "qualification grant failed")?;
    }
    let tokens = tokens::issue(input, database, &input.directory.join("tokens")).await?;
    let public_key = assertion_public_key(&signing);
    let auth = ApiTokenAuthConfig::new(
        &input.schemas.auth,
        &input.issuer,
        &public_key,
        "auth/database",
        "auth/signing",
        "auth/pepper",
        60,
    )?
    .with_credential_state_callers(vec![CALLER.into()])?;
    let access = AccessControlConfig::new(
        &input.schemas.access,
        "access/database",
        &input.issuer,
        &public_key,
        vec![BOOTSTRAP.into()],
    )?;
    let approval = BusinessApprovalConfig::new(
        &input.schemas.approval,
        "approval/database",
        vec![CALLER.into()],
        vec![CALLER.into()],
        vec![CALLER.into()],
    )?;
    let audit = AuditLogConfig::new("audit/database", vec![CALLER.into()], vec![CALLER.into()])?
        .with_reader_scopes(
            CALLER,
            vec![AuditReadScope {
                kind: "deployment".into(),
                id: input.deployment.clone(),
            }],
        )?;
    private_write(
        &input.directory.join("setup.json"),
        &serde_json::to_vec_pretty(&json!({
            "schema":"lenso.ops-operator-setup.v1", "public_key":public_key, "max_assertion_ttl_seconds":60,
            "owners":{"auth":auth,"access":access,"approval":approval,"audit":audit}, "tokens":tokens,
        }))?,
    )?;
    Ok(())
}
async fn run() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let action = arguments.next().ok_or("missing action")?;
    let input: Input = serde_json::from_slice(&std::fs::read(
        arguments.next().ok_or("missing input file")?,
    )?)?;
    for path in [
        &input.database_uri_file,
        &input.audit_uri_file,
        &input.directory,
        &input.signing_file,
        &input.pepper_file,
    ] {
        if !path.is_absolute() {
            return Err("setup paths must be absolute".into());
        }
    }
    let database = std::fs::read_to_string(&input.database_uri_file)?;
    match action.as_str() {
        "setup" => setup(&input, database.trim()).await?,
        "human-setup" => human::setup(&input, database.trim()).await?,
        "refresh" => tokens::refresh(&input, database.trim()).await?,
        "qualification-read-token" => {
            tokens::qualification_read_token(&input, database.trim()).await?
        }
        "upgrade" => {
            private_write(
                &input.directory.join("owners-upgrade-started.json"),
                b"{\"state\":\"started\"}\n",
            )?;
            ApiTokenAuthOperator::upgrade(database.trim(), &input.schemas.auth).await?;
            if input.directory.join("human-setup.json").is_file() {
                human::upgrade(&input, database.trim()).await?;
            }
            private_write(
                &input.directory.join("owners-upgrade-completed.json"),
                b"{\"state\":\"completed\"}\n",
            )?;
        }
        "revoke" => {
            let id = arguments.next().ok_or("missing token ID")?;
            let operator =
                ApiTokenAuthOperator::connect(database.trim(), &input.schemas.auth).await?;
            if !operator.revoke_token(&id).await? {
                return Err("token not found".into());
            }
        }
        "qualify" | "unqualify" => {
            let store = QualificationStore::open(&input.directory.join("qualification.sqlite"))
                .map_err(|_| "qualification unavailable")?;
            let subject = arguments.next().ok_or("missing subject")?;
            if action == "qualify" {
                store
                    .grant(&input.deployment, &subject)
                    .map_err(|_| "qualification grant failed")?;
            } else {
                store
                    .revoke(&input.deployment, &subject)
                    .map_err(|_| "qualification revoke failed")?;
            }
        }
        _ => return Err("unknown action".into()),
    }
    if arguments.next().is_some() {
        return Err("unexpected argument".into());
    }
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if run().await.is_err() {
        eprintln!("Explicit Ops operator action failed");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_receipt_creation_never_overwrites_a_completed_phase() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ops-receipt-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("completed.json");
        private_write(&path, b"original").unwrap();
        assert!(private_write(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
