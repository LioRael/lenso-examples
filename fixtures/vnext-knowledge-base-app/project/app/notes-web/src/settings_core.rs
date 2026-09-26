use lenso_capability_http_endpoint::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const DEFAULT_EXCERPT_LIMIT: i64 = 96;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessSettings {
    pub excerpt_limit: i64,
    pub revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateBusinessSettings {
    pub excerpt_limit: i64,
    pub predecessor_revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsCommand {
    pub excerpt_limit: i64,
    pub predecessor_revision: i64,
    pub idempotency_key: Option<String>,
    pub payload_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsCommandError {
    InvalidLimit,
    InvalidIdempotencyKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsFailure {
    InvalidLimit,
    InvalidIdempotencyKey,
    StaleRevision,
    IdempotencyConflict,
    Unauthorized,
    StorageUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsProblemSpec {
    pub status: u16,
    pub title: &'static str,
    pub code: &'static str,
    pub detail: &'static str,
}

pub fn problem_spec(failure: SettingsFailure) -> SettingsProblemSpec {
    match failure {
        SettingsFailure::InvalidLimit => SettingsProblemSpec {
            status: 400,
            title: "Bad Request",
            code: "invalid_excerpt_limit",
            detail: "excerpt_limit must be from 16 through 512",
        },
        SettingsFailure::InvalidIdempotencyKey => SettingsProblemSpec {
            status: 400,
            title: "Bad Request",
            code: "invalid_idempotency_key",
            detail: "Idempotency-Key must be 1 through 128 printable ASCII bytes without a comma",
        },
        SettingsFailure::StaleRevision => SettingsProblemSpec {
            status: 409,
            title: "Conflict",
            code: "stale_settings_revision",
            detail: "the settings predecessor revision is stale",
        },
        SettingsFailure::IdempotencyConflict => SettingsProblemSpec {
            status: 409,
            title: "Conflict",
            code: "idempotency_conflict",
            detail: "the Idempotency-Key was already used with a different settings update",
        },
        SettingsFailure::Unauthorized => SettingsProblemSpec {
            status: 401,
            title: "Unauthorized",
            code: "authentication_required",
            detail: "Provide a valid Bearer credential.",
        },
        SettingsFailure::StorageUnavailable => SettingsProblemSpec {
            status: 503,
            title: "Service Unavailable",
            code: "knowledge_storage_unavailable",
            detail: "knowledge storage is temporarily unavailable",
        },
    }
}

impl From<SettingsCommandError> for SettingsFailure {
    fn from(value: SettingsCommandError) -> Self {
        match value {
            SettingsCommandError::InvalidLimit => Self::InvalidLimit,
            SettingsCommandError::InvalidIdempotencyKey => Self::InvalidIdempotencyKey,
        }
    }
}

pub fn prepare_settings_command(
    input: UpdateBusinessSettings,
    idempotency_key: Option<&str>,
) -> Result<SettingsCommand, SettingsCommandError> {
    if !(16..=512).contains(&input.excerpt_limit) {
        return Err(SettingsCommandError::InvalidLimit);
    }
    if let Some(key) = idempotency_key {
        if key.is_empty()
            || key.len() > 128
            || !key
                .bytes()
                .all(|byte| (0x21..=0x7e).contains(&byte) && byte != b',')
        {
            return Err(SettingsCommandError::InvalidIdempotencyKey);
        }
    }
    let canonical = format!(
        "lenso.knowledge-settings-command.v1\n{}\n{}",
        input.predecessor_revision, input.excerpt_limit
    );
    let digest = Sha256::digest(canonical.as_bytes());
    let mut payload_sha256 = String::with_capacity(71);
    payload_sha256.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut payload_sha256, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(SettingsCommand {
        excerpt_limit: input.excerpt_limit,
        predecessor_revision: input.predecessor_revision,
        idempotency_key: idempotency_key.map(ToOwned::to_owned),
        payload_sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_validation_and_fingerprint_are_canonical() {
        let first = prepare_settings_command(
            UpdateBusinessSettings {
                excerpt_limit: 48,
                predecessor_revision: 2,
            },
            Some("request-001"),
        )
        .unwrap();
        let repeated = prepare_settings_command(
            UpdateBusinessSettings {
                excerpt_limit: 48,
                predecessor_revision: 2,
            },
            Some("request-002"),
        )
        .unwrap();
        assert_eq!(first.payload_sha256, repeated.payload_sha256);
        assert!(first.payload_sha256.starts_with("sha256:"));
        assert_eq!(first.payload_sha256.len(), 71);
        assert_eq!(
            prepare_settings_command(
                UpdateBusinessSettings {
                    excerpt_limit: 15,
                    predecessor_revision: 2,
                },
                None,
            ),
            Err(SettingsCommandError::InvalidLimit)
        );
        assert_eq!(
            prepare_settings_command(
                UpdateBusinessSettings {
                    excerpt_limit: 48,
                    predecessor_revision: 0,
                },
                None,
            )
            .unwrap()
            .predecessor_revision,
            0
        );
        assert_eq!(
            prepare_settings_command(
                UpdateBusinessSettings {
                    excerpt_limit: 48,
                    predecessor_revision: 2,
                },
                Some("key,other"),
            ),
            Err(SettingsCommandError::InvalidIdempotencyKey)
        );
    }
}
