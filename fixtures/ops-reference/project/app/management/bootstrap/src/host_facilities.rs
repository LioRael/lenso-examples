use std::path::PathBuf;

#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapHandle {
    pub subject: String,
    pub bootstrap_token_file: PathBuf,
    pub scopes: Vec<Scope>,
}
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub kind: String,
    pub id: String,
    pub roles: Vec<Role>,
}
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub id: String,
    pub permissions: Vec<String>,
    pub subjects: Vec<String>,
}
impl std::fmt::Debug for BootstrapHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BootstrapHandle")
            .field("subject", &self.subject)
            .field("scopes", &self.scopes)
            .finish()
    }
}
pub fn bootstrap(binding: &serde_json::Value) -> Result<BootstrapHandle, lenso::RuntimeFailure> {
    let selected: BootstrapHandle =
        serde_json::from_value(binding.clone()).map_err(|_| invalid())?;
    if selected.subject.is_empty()
        || !selected.bootstrap_token_file.is_absolute()
        || selected.scopes.is_empty()
        || selected.scopes.len() > 4
        || selected.scopes.iter().any(|scope| {
            scope.kind.is_empty()
                || scope.id.is_empty()
                || scope.roles.is_empty()
                || scope.roles.len() > 8
                || scope.roles.iter().any(|role| {
                    role.id.is_empty() || role.permissions.is_empty() || role.subjects.is_empty()
                })
        })
    {
        return Err(invalid());
    }
    Ok(selected)
}
fn invalid() -> lenso::RuntimeFailure {
    lenso::RuntimeFailure::InvalidResolvedPlan {
        detail: "Explicit bootstrap requires a bounded fixed owner setup plan".into(),
    }
}
