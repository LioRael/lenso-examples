use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    path: PathBuf,
    callers: Vec<String>,
}
#[derive(Clone)]
pub struct SecretHandle(BTreeMap<String, Reference>);
impl std::fmt::Debug for SecretHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SecretHandle").field(&self.0.keys()).finish()
    }
}
pub fn secrets(binding: &serde_json::Value) -> Result<SecretHandle, lenso::RuntimeFailure> {
    let references: BTreeMap<String, Reference> =
        serde_json::from_value(binding.clone()).map_err(|_| invalid())?;
    if references.is_empty()
        || references.len() > 32
        || references.values().any(|reference| {
            !reference.path.is_absolute()
                || reference.callers.is_empty()
                || reference.callers.len() > 16
                || reference.callers.iter().any(|caller| {
                    caller.split('/').count() != 2 || caller.split('/').any(str::is_empty)
                })
        })
    {
        return Err(invalid());
    }
    Ok(SecretHandle(references))
}
fn invalid() -> lenso::RuntimeFailure {
    lenso::RuntimeFailure::InvalidResolvedPlan {
        detail: "Secrets require explicit local files and exact owner callers".into(),
    }
}
impl SecretHandle {
    pub fn resolve(&self, reference: &str, caller: Option<&str>) -> Option<String> {
        let selected = self.0.get(reference)?;
        if !selected
            .callers
            .iter()
            .any(|allowed| Some(allowed.as_str()) == caller)
        {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if std::fs::metadata(&selected.path).ok()?.permissions().mode() & 0o077 != 0 {
                return None;
            }
        }
        let value = std::fs::read_to_string(&selected.path).ok()?;
        if value.is_empty() || value.len() > 8192 {
            return None;
        }
        Some(value)
    }
}
