use lenso_auth_sdk::realm::RealmAssertionVerifier;
use lenso_management_authority::{Clock, QualificationStore};
use std::{path::PathBuf, rc::Rc, time::Duration};
use time::OffsetDateTime;

#[derive(Clone, Debug)]
pub struct AuthorityHandle {
    pub deployment: String,
    pub journal: PathBuf,
    pub qualification: Rc<QualificationStore>,
    pub verifier: RealmAssertionVerifier,
    pub clock: Rc<dyn Clock>,
}

#[derive(Debug)]
struct HostClock(lenso_native_adapter::NativeHostClock);
impl Clock for HostClock {
    fn monotonic(&self) -> Duration {
        self.0.now()
    }
    fn wall(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

pub fn authority(
    binding: &serde_json::Value,
    clock: &lenso_native_adapter::NativeHostClock,
) -> Result<AuthorityHandle, lenso::RuntimeFailure> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Binding {
        deployment: String,
        journal: PathBuf,
        qualification: PathBuf,
        issuer: String,
        public_key: String,
        max_assertion_ttl_seconds: u32,
    }
    let invalid = || lenso::RuntimeFailure::InvalidResolvedPlan {
        detail: "Management requires an explicit prepared operators Host facility".into(),
    };
    let selected: Binding = serde_json::from_value(binding.clone()).map_err(|_| invalid())?;
    if selected.deployment.is_empty()
        || !selected.journal.is_absolute()
        || !selected.qualification.is_absolute()
    {
        return Err(invalid());
    }
    let verifier = RealmAssertionVerifier::new(
        "operators",
        &selected.issuer,
        &selected.public_key,
        selected.max_assertion_ttl_seconds,
        None,
    )
    .map_err(|_| invalid())?;
    let qualification = QualificationStore::open(&selected.qualification).map_err(|_| invalid())?;
    Ok(AuthorityHandle {
        deployment: selected.deployment,
        journal: selected.journal,
        qualification: Rc::new(qualification),
        verifier,
        clock: Rc::new(HostClock(clock.clone())),
    })
}
