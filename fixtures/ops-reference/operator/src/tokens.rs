use super::{Input, Result, ceiling, private_write};
use lenso_auth_api_token_plugin::{ApiTokenAuthOperator, IssueApiToken};
use lenso_auth_sdk::{audience, credential::MANAGEMENT_CEILING_CLAIM};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
use time::OffsetDateTime;

fn audiences(input: &Input) -> Vec<String> {
    let mut audiences = [
        ("lenso.management@1", "catalog"),
        ("lenso.management@1", "invoke"),
        ("lenso.management@1", "status"),
        ("lenso.agent.tool-provider@2", "catalog"),
        ("lenso.agent.tool-provider@2", "execute"),
        ("lenso.management-human@1", "read_intent"),
        ("lenso.management-human@1", "decide"),
        ("lenso.business-approval@1", "decide"),
        ("lenso.access-control-admin@1", "create_role"),
        ("lenso.access-control-admin@1", "set_role_permissions"),
        ("lenso.access-control-admin@1", "assign_role"),
        ("lenso.access-control-admin@1", "revoke_role"),
    ]
    .map(|(capability, operation)| audience(capability, operation))
    .to_vec();
    audiences.push(input.resource_uri.clone());
    audiences
}

pub async fn issue(
    input: &Input,
    database: &str,
    directory: &Path,
) -> Result<BTreeMap<String, Value>> {
    let operator = ApiTokenAuthOperator::connect(database, &input.schemas.auth).await?;
    let pepper = std::fs::read_to_string(&input.pepper_file)?;
    let audiences = audiences(input);
    std::fs::create_dir(directory)?;
    let mut tokens = BTreeMap::<String, Value>::new();
    for (name, subject, kind, deployment) in [
        ("alice", "alice", "user", input.deployment.as_str()),
        ("bob", "bob", "user", input.deployment.as_str()),
        (
            "bootstrap",
            "bootstrap-human",
            "user",
            input.deployment.as_str(),
        ),
        (
            "machine",
            "alice",
            "service-account",
            input.deployment.as_str(),
        ),
        ("wrong-deployment", "alice", "user", "other-deployment"),
    ] {
        let token = operator
            .issue(
                pepper.as_bytes(),
                IssueApiToken {
                    subject: subject.into(),
                    actor_kind: kind.into(),
                    assurance: "api-token".into(),
                    audience: audiences.clone(),
                    claims: BTreeMap::from([(
                        MANAGEMENT_CEILING_CLAIM.into(),
                        json!(ceiling(deployment)),
                    )]),
                    expires_at: OffsetDateTime::now_utc() + time::Duration::hours(1),
                },
            )
            .await?;
        private_write(
            &directory.join(format!("{name}.secret")),
            token.expose_secret().as_bytes(),
        )?;
        tokens.insert(
            name.into(),
            json!({"token_id":token.token_id(), "session_id":token.session_id()}),
        );
    }
    Ok(tokens)
}

pub async fn qualification_read_token(input: &Input, database: &str) -> Result<()> {
    let phase = input.directory.join("qualification-read-token");
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(&phase)?;
    private_write(&phase.join("started.json"), b"{\"state\":\"started\"}\n")?;
    let operator = ApiTokenAuthOperator::connect(database, &input.schemas.auth).await?;
    let pepper = std::fs::read_to_string(&input.pepper_file)?;
    let expires_at = OffsetDateTime::now_utc() + time::Duration::hours(1);
    let token = operator
        .issue(
            pepper.as_bytes(),
            IssueApiToken {
                subject: "bob".into(),
                actor_kind: "user".into(),
                assurance: "api-token".into(),
                audience: audiences(input),
                claims: BTreeMap::from([(
                    MANAGEMENT_CEILING_CLAIM.into(),
                    json!(ceiling(&input.deployment)),
                )]),
                expires_at,
            },
        )
        .await?;
    private_write(
        &phase.join("c5-bob.secret"),
        token.expose_secret().as_bytes(),
    )?;
    private_write(
        &phase.join("completed.json"),
        &serde_json::to_vec_pretty(&json!({
            "schema":"lenso.ops-qualification-read-token.v1", "state":"completed",
            "owner_source":"5fb16bbcbff51cce394fd6e29df9079485f7f37e",
            "deployment":input.deployment, "subject":"bob", "actor_kind":"user",
            "token_id":token.token_id(), "session_id":token.session_id(),
            "expires_at_unix":expires_at.unix_timestamp(), "ttl_seconds":3600,
            "original_credentials_changed":false, "schema_or_rbac_changed":false,
        }))?,
    )?;
    Ok(())
}

pub async fn refresh(input: &Input, database: &str) -> Result<()> {
    let root = &input.directory;
    let setup_path = root.join("setup.json");
    let mut setup: Value = serde_json::from_slice(&std::fs::read(&setup_path)?)?;
    let old = setup["tokens"]
        .as_object()
        .ok_or("missing prepared credentials")?;
    private_write(
        &root.join("tokens-refresh-started.json"),
        b"{\"state\":\"started\"}\n",
    )?;
    let operator = ApiTokenAuthOperator::connect(database, &input.schemas.auth).await?;
    for token in old.values() {
        let id = token["token_id"]
            .as_str()
            .ok_or("missing prepared token ID")?;
        operator.revoke_token(id).await?;
    }
    let fresh = root.join("tokens-refreshed");
    setup["tokens"] = serde_json::to_value(issue(input, database, &fresh).await?)?;
    let fresh_setup = root.join("setup-refreshed.json");
    private_write(&fresh_setup, &serde_json::to_vec_pretty(&setup)?)?;
    std::fs::rename(root.join("tokens"), root.join("tokens-before-refresh"))?;
    std::fs::rename(fresh, root.join("tokens"))?;
    std::fs::rename(&setup_path, root.join("setup-before-refresh.json"))?;
    std::fs::rename(fresh_setup, setup_path)?;
    private_write(
        &root.join("tokens-refresh-completed.json"),
        b"{\"state\":\"completed\"}\n",
    )?;
    Ok(())
}
