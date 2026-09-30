use std::path::PathBuf;
#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapHandle {
    pub deployment: String,
    pub bootstrap_subject: String,
    pub bootstrap_token_file: PathBuf,
    pub qualification: PathBuf,
    pub receipt: PathBuf,
    pub accounts: Vec<Account>,
}
#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub alias: String,
    pub identifier: String,
    pub password_file: PathBuf,
    pub roles: Vec<Role>,
}
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub kind: String,
    pub id: String,
    pub role_id: String,
    pub permissions: Vec<String>,
}
impl std::fmt::Debug for BootstrapHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BootstrapHandle")
            .field("deployment", &self.deployment)
            .field("accounts", &self.accounts.len())
            .finish()
    }
}
pub fn bootstrap(binding: &serde_json::Value) -> Result<BootstrapHandle, lenso::RuntimeFailure> {
    let selected: BootstrapHandle =
        serde_json::from_value(binding.clone()).map_err(|_| invalid())?;
    if selected.deployment.is_empty()
        || selected.bootstrap_subject.is_empty()
        || !selected.bootstrap_token_file.is_absolute()
        || !selected.qualification.is_absolute()
        || !selected.receipt.is_absolute()
        || selected.receipt.exists()
        || selected.accounts.is_empty()
        || selected.accounts.len() > 4
        || selected.accounts.iter().any(|account| {
            account.alias.is_empty()
                || account.identifier.is_empty()
                || !account.password_file.is_absolute()
                || account.roles.is_empty()
                || account.roles.len() > 4
                || account.roles.iter().any(|role| {
                    role.kind.is_empty()
                        || role.id.is_empty()
                        || role.role_id.is_empty()
                        || role.permissions.is_empty()
                        || role.permissions.len() > 16
                })
        })
    {
        return Err(invalid());
    }
    Ok(selected)
}
fn invalid() -> lenso::RuntimeFailure {
    lenso::RuntimeFailure::InvalidResolvedPlan {
        detail:
            "Human bootstrap requires an explicit bounded owner setup plan and a fresh receipt path"
                .into(),
    }
}
