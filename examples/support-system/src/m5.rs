use std::collections::BTreeMap;

use lenso_service::{
    CanaryDecision, CanaryPlanInput, CanaryState, ConfigActivationReceipt, ConfigField,
    ConfigFieldActivation, ConfigFieldScope, ConfigFieldSensitivity, ConfigOperation, ConfigState,
    ConfigValueType, CoordinationOutageClaims, CoordinationOutageEvidence, CoordinationOutageInput,
    CoordinationOutageObservation, DataPlaneOperation, DeliveryDecision, DeliveryEvidenceReference,
    DeliveryPolicyInputs, DeliveryReliabilityContract, DependencyCriticality,
    DependencyReliability, DependencyReliabilityObservation, DeploymentAdapterKind,
    DeploymentEnvironmentBinding, DeploymentPlan, DeploymentState, DeploymentWorkloadSettings,
    DeterministicCoordinationAuthorityProvider, DeterministicReliabilityObservationProvider,
    DeterministicRollbackSafetyProvider, DeterministicTrustProvider,
    Ed25519GatewayObservationProvider, Ed25519OperatorObservationAuthorityProvider,
    EdgeAuthentication, EdgeOperationVisibility, EdgeRoute, EdgeServiceOperation,
    EnvironmentVerification, EnvironmentVerificationInput, GatewayEnvironmentBinding,
    OperatorObservationAttestation, OperatorObservationClaims, PolicyEvaluationSurface,
    PolicyEvidence, ProductionEligibilityInput, PromotionPlan, PromotionPlanInput,
    PromotionReceipt, PromotionState, RateIntent, ReleaseContractVersion, ReleaseMigration,
    ReleaseModule, ReleaseProvenance, ReleaseRetention, ReleaseRollbackConstraints,
    ReleaseRolloutGate, ReleaseTrustEvidence, ReleaseWorkloadRole, ReliabilityObservation,
    RollbackCompatibilityInput, RollbackReceipt, RollbackSafetyInput, RollbackState,
    SecretReferenceObservation, SecretReferenceStatus, SecurityContinuity, ServiceRelease,
    ServiceReleaseInput, WorkflowCompatibilityInput, WorkloadArtifact, apply_config_activation,
    apply_deployment, apply_promotion, apply_rollback, approve_promotion, assemble_service_release,
    attach_service_release_signature, attest_coordination_outage, attest_operator_observation,
    attest_production_eligibility_input, build_config_contract, build_config_revision,
    build_edge_contract, evaluate_canary, evaluate_delivery_policy,
    evaluate_production_eligibility, extraction_input_digest, observe_deployment,
    observe_deployment_adapter, observe_gateway, observe_rollback_convergence,
    observe_secret_reference, plan_canary, plan_config_activation, plan_deployment,
    plan_gateway_configuration, plan_promotion, plan_rollback, production_policy_pack,
    prove_system_plane_outage, reliability_contract_digest, seal_reliability_observation,
    seal_rollback_safety_evidence, verify_service_release_trust, verify_staging_environment,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActualOperatorWorkloadObservation {
    workload_id: String,
    observed_digest: Option<String>,
    ready: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActualOperatorObservation {
    observation_id: String,
    observation_digest: String,
    authority_id: String,
    authority_proof: String,
    claims: OperatorObservationClaims,
    observed_release_id: String,
    observed_release_digest: String,
    config_revision_id: String,
    workloads: Vec<ActualOperatorWorkloadObservation>,
    fresh: bool,
    drifted: bool,
    decision: DeliveryDecision,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActualGatewayObservation {
    protocol: String,
    observation_id: String,
    plan_id: String,
    plan_digest: String,
    environment: String,
    release_id: String,
    release_digest: String,
    resource_uid: String,
    resource_version: String,
    authority_context: String,
    configuration_identity: String,
    revision: u64,
    observed_after: String,
    fresh: bool,
    provider_id: String,
    provider_proof: String,
}

type ActualOutageObservation = CoordinationOutageObservation;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActualSupplyChainEvidence {
    candidate: ActualReleaseSupplyChainEvidence,
    previous: ActualReleaseSupplyChainEvidence,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActualReleaseSupplyChainEvidence {
    sbom_reference: String,
    sbom_digest: String,
    provenance_reference: String,
    provenance_digest: String,
    source: String,
    builder: String,
    input_digests: Vec<String>,
    subject_digest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct M5PolicyEvidence {
    pub evaluation_input: serde_json::Value,
    pub local: PolicyEvidence,
    pub ci_equivalent: PolicyEvidence,
    pub system_plane: PolicyEvidence,
    pub byte_equivalent: bool,
    pub blocked_issue_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct M5SmokeEvidence {
    pub artifact_version: String,
    pub outcome: String,
    pub public_seam: String,
    pub service_release: ServiceRelease,
    pub trust: ReleaseTrustEvidence,
    pub tampered_issue_codes: Vec<String>,
    pub untrusted_issue_codes: Vec<String>,
    pub revoked_issue_codes: Vec<String>,
    pub policy: M5PolicyEvidence,
    pub config_revision: lenso_service::ConfigRevision,
    pub previous_config_revision: lenso_service::ConfigRevision,
    pub config_stage: ConfigActivationReceipt,
    pub config_rollback: Option<ConfigActivationReceipt>,
    pub redaction_proven: bool,
    pub staging_deployment_plan: DeploymentPlan,
    pub staging_deployment_receipt: lenso_service::DeploymentReceipt,
    pub staging_deployment_observation: lenso_service::DeploymentObservation,
    pub production_deployment_plan: DeploymentPlan,
    pub previous_deployment_plan: DeploymentPlan,
    pub previous_deployment_receipt: lenso_service::DeploymentReceipt,
    pub previous_deployment_observation: lenso_service::DeploymentObservation,
    pub staging_edge_contract: lenso_service::EdgeContract,
    pub production_edge_contract: lenso_service::EdgeContract,
    pub previous_edge_contract: lenso_service::EdgeContract,
    pub staging_gateway_plan: lenso_service::GatewayConfigurationPlan,
    pub staging_gateway_observation: lenso_service::GatewayObservation,
    pub production_gateway_plan: lenso_service::GatewayConfigurationPlan,
    pub previous_gateway_plan: lenso_service::GatewayConfigurationPlan,
    pub previous_gateway_observation: lenso_service::GatewayObservation,
    pub environment_verification: EnvironmentVerification,
    pub promotion: PromotionPlan,
    pub promotion_approval: lenso_service::PromotionApproval,
    pub promotion_protected_evidence: lenso_service::PromotionProtectedEvidence,
    pub promotion_receipt: PromotionReceipt,
    pub canary_plan: lenso_service::CanaryPlan,
    pub production_deployment_observation: lenso_service::DeploymentObservation,
    pub canary_history: Vec<CanaryDecision>,
    pub canary_observations: Vec<ReliabilityObservation>,
    pub rollback_plan: lenso_service::RollbackPlan,
    pub rollback: Option<RollbackReceipt>,
    pub rollback_deployment_observation: Option<lenso_service::DeploymentObservation>,
    pub rollback_gateway_observation: Option<lenso_service::GatewayObservation>,
    pub outage: CoordinationOutageEvidence,
    pub migration_first_required: bool,
    pub public_edge_paths: Vec<String>,
    pub internal_operations_private: bool,
    pub prior_guarantees: String,
    pub provider_compatibility: String,
    pub local_requirements: Vec<String>,
}

pub fn run_m5_smoke() -> anyhow::Result<M5SmokeEvidence> {
    let actual_operator =
        read_optional_json::<ActualOperatorObservation>("M5_STAGING_OPERATOR_OBSERVATION")?;
    let actual_gateway =
        read_optional_json::<ActualGatewayObservation>("M5_STAGING_GATEWAY_OBSERVATION")?;
    let actual_baseline_operator =
        read_optional_json::<ActualOperatorObservation>("M5_BASELINE_OPERATOR_OBSERVATION")?;
    let actual_baseline_gateway =
        read_optional_json::<ActualGatewayObservation>("M5_BASELINE_GATEWAY_OBSERVATION")?;
    let actual_promoted_operator =
        read_optional_json::<ActualOperatorObservation>("M5_PROMOTED_OPERATOR_OBSERVATION")?;
    let actual_reliability = if let Some(observations) =
        read_optional_json::<Vec<ReliabilityObservation>>("M5_CANARY_RELIABILITY_OBSERVATIONS")?
    {
        Some(observations)
    } else {
        read_optional_json::<ReliabilityObservation>("M5_CANARY_RELIABILITY_OBSERVATION")?
            .map(|observation| vec![observation])
    };
    let actual_rollback_operator =
        read_optional_json::<ActualOperatorObservation>("M5_ROLLBACK_OPERATOR_OBSERVATION")?;
    let actual_rollback_gateway =
        read_optional_json::<ActualGatewayObservation>("M5_ROLLBACK_GATEWAY_OBSERVATION")?;
    let actual_outage = read_optional_json::<ActualOutageObservation>("M5_OUTAGE_OBSERVATION")?;
    let actual_supply_chain = std::env::var("M5_SUPPLY_CHAIN_EVIDENCE")
        .ok()
        .map(|value| serde_json::from_str::<ActualSupplyChainEvidence>(&value))
        .transpose()?;
    let config_contract = build_config_contract(
        "config-contract:support:v1",
        vec![
            ConfigField {
                path: "MAX_CONCURRENCY".to_owned(),
                value_type: ConfigValueType::Integer,
                required: true,
                sensitivity: ConfigFieldSensitivity::Public,
                scope: ConfigFieldScope::Service,
                activation: ConfigFieldActivation::Hot,
                mutable: true,
            },
            ConfigField {
                path: "DB_PASSWORD".to_owned(),
                value_type: ConfigValueType::String,
                required: true,
                sensitivity: ConfigFieldSensitivity::Sensitive,
                scope: ConfigFieldScope::Service,
                activation: ConfigFieldActivation::Restart,
                mutable: true,
            },
        ],
    )
    .map_err(delivery_error)?;
    let secret_provider = lenso_service::DeterministicSecretProvider::new(
        "acceptance-local",
        [
            (
                "secret:support:database:v5".to_owned(),
                SecretReferenceObservation {
                    status: SecretReferenceStatus::Resolved,
                    metadata: BTreeMap::from([("rotationRevision".to_owned(), "7".to_owned())]),
                },
            ),
            (
                "secret:support:database:v4".to_owned(),
                SecretReferenceObservation {
                    status: SecretReferenceStatus::Resolved,
                    metadata: BTreeMap::from([("rotationRevision".to_owned(), "6".to_owned())]),
                },
            ),
        ],
    );
    let secret_reference = observe_secret_reference(
        &secret_provider,
        "secret:support:database:v5",
        "DB_PASSWORD",
        "service:support",
    );
    let config = build_config_revision(
        "service:support",
        &config_contract,
        BTreeMap::from([("MAX_CONCURRENCY".to_owned(), serde_json::json!(32))]),
        vec![secret_reference],
        &secret_provider,
    )
    .map_err(delivery_error)?;
    let previous_config = build_config_revision(
        "service:support",
        &config_contract,
        BTreeMap::from([("MAX_CONCURRENCY".to_owned(), serde_json::json!(16))]),
        vec![observe_secret_reference(
            &secret_provider,
            "secret:support:database:v4",
            "DB_PASSWORD",
            "service:support",
        )],
        &secret_provider,
    )
    .map_err(delivery_error)?;

    let provider = DeterministicTrustProvider::new([("ci:m5-acceptance", "ephemeral-m5-key")]);
    let mut release = assemble_service_release(release_input(
        "5.0.0",
        &config_contract,
        actual_supply_chain
            .as_ref()
            .map(|evidence| &evidence.candidate),
    ))
    .map_err(delivery_error)?;
    attach_service_release_signature(&mut release, &provider, "ci:m5-acceptance")
        .map_err(|issue| delivery_error(vec![issue]))?;
    let trust = verify_service_release_trust(&release, &provider);
    anyhow::ensure!(
        trust.decision == DeliveryDecision::Passed,
        "release trust must pass"
    );
    let mut tampered = release.clone();
    tampered.service_version = "5.0.1-tampered".to_owned();
    let tampered = verify_service_release_trust(&tampered, &provider);
    anyhow::ensure!(
        tampered.decision == DeliveryDecision::Blocked,
        "tampering must fail closed"
    );
    let untrusted = verify_service_release_trust(
        &release,
        &DeterministicTrustProvider::new(std::iter::empty::<(String, String)>()),
    );
    anyhow::ensure!(
        untrusted.decision == DeliveryDecision::Blocked,
        "untrusted candidate must fail closed"
    );
    let revoked = verify_service_release_trust(
        &release,
        &DeterministicTrustProvider::new([("ci:m5-acceptance", "ephemeral-m5-key")])
            .with_revoked("ci:m5-acceptance"),
    );
    anyhow::ensure!(
        revoked.decision == DeliveryDecision::Blocked,
        "revoked candidate must fail closed"
    );

    let mut previous_release = assemble_service_release(release_input(
        "4.9.0",
        &config_contract,
        actual_supply_chain
            .as_ref()
            .map(|evidence| &evidence.previous),
    ))
    .map_err(delivery_error)?;
    attach_service_release_signature(&mut previous_release, &provider, "ci:m5-acceptance")
        .map_err(|issue| delivery_error(vec![issue]))?;
    let previous_trust = verify_service_release_trust(&previous_release, &provider);
    let previous_eligibility_input = safe_eligibility(
        &previous_release,
        &provider,
        RollbackTargetEvidence::synthetic_prior(),
    )?;
    let pack = production_policy_pack();
    let previous_policy_inputs = DeliveryPolicyInputs {
        release: previous_release.clone(),
        trust: previous_trust,
        config_contract: config_contract.clone(),
        config: previous_config.clone(),
        eligibility: evaluate_production_eligibility(
            &previous_eligibility_input,
            &previous_release,
            &provider,
        ),
        eligibility_input: previous_eligibility_input,
    };
    let previous_policy = evaluate_delivery_policy(
        &pack,
        &previous_policy_inputs,
        &provider,
        &secret_provider,
        PolicyEvaluationSurface::Local,
    );
    anyhow::ensure!(
        previous_policy.decision == DeliveryDecision::Passed,
        "previous known-good release must have passing policy evidence"
    );
    let (previous_edge, previous_gateway) =
        gateway_plan(&previous_release, &provider, "production", 8)?;
    let mut previous_binding = deployment_binding(
        "production",
        31,
        &previous_config,
        &previous_gateway.plan_digest,
        &previous_policy,
    );
    previous_binding
        .adapter_inputs
        .insert("rollbackReleaseId".to_owned(), release.release_id.clone());
    let previous_plan = plan_deployment(
        &previous_release,
        &config_contract,
        &previous_config,
        &secret_provider,
        &previous_binding,
        DeploymentAdapterKind::Kubernetes,
    )
    .map_err(delivery_error)?;

    let candidate_rollback_target = RollbackTargetEvidence::from_actual(
        &previous_release,
        &previous_plan,
        &previous_config,
        &previous_gateway,
    );
    let eligibility_input =
        safe_eligibility(&release, &provider, candidate_rollback_target.clone())?;
    let eligibility = evaluate_production_eligibility(&eligibility_input, &release, &provider);
    let policy_evaluation_input = serde_json::json!({
        "protocol": "lenso.policy-evaluation-input.v1",
        "release": release,
        "trust": trust,
        "configContract": config_contract,
        "config": config,
        "eligibilityInput": eligibility_input,
        "eligibility": eligibility,
    });
    let policy_inputs = DeliveryPolicyInputs {
        release: release.clone(),
        trust: trust.clone(),
        config_contract: config_contract.clone(),
        config: config.clone(),
        eligibility: eligibility.clone(),
        eligibility_input: eligibility_input.clone(),
    };
    let local = evaluate_delivery_policy(
        &pack,
        &policy_inputs,
        &provider,
        &secret_provider,
        PolicyEvaluationSurface::Local,
    );
    let ci_equivalent = evaluate_delivery_policy(
        &pack,
        &policy_inputs,
        &provider,
        &secret_provider,
        PolicyEvaluationSurface::Ci,
    );
    let system_plane = evaluate_delivery_policy(
        &pack,
        &policy_inputs,
        &provider,
        &secret_provider,
        PolicyEvaluationSurface::SystemPlane,
    );
    let byte_equivalent = serde_json::to_vec(&local)? == serde_json::to_vec(&ci_equivalent)?
        && serde_json::to_vec(&local)? == serde_json::to_vec(&system_plane)?;
    anyhow::ensure!(byte_equivalent, "Policy Evidence must be byte-equivalent");
    let mut unsafe_eligibility = safe_eligibility(&release, &provider, candidate_rollback_target)?;
    unsafe_eligibility.workload_identity_production = None;
    let unsafe_policy = evaluate_delivery_policy(
        &pack,
        &DeliveryPolicyInputs {
            release: release.clone(),
            trust: trust.clone(),
            config_contract: config_contract.clone(),
            config: config.clone(),
            eligibility: evaluate_production_eligibility(&unsafe_eligibility, &release, &provider),
            eligibility_input: unsafe_eligibility,
        },
        &provider,
        &secret_provider,
        PolicyEvaluationSurface::Local,
    );
    anyhow::ensure!(
        unsafe_policy.decision == DeliveryDecision::Blocked,
        "unsafe candidate must be blocked"
    );

    let (staging_edge, staging_gateway) = gateway_plan(&release, &provider, "staging", 5)?;
    let mut staging_config_state = ConfigState::new("staging", 17);
    let stage_plan = plan_config_activation(
        &staging_config_state,
        &config_contract,
        &config,
        &secret_provider,
        ConfigOperation::Stage,
    )
    .map_err(delivery_error)?;
    let config_stage = apply_config_activation(&mut staging_config_state, &stage_plan)
        .map_err(|error| delivery_error(error.issues))?;
    let staging_plan = plan_deployment(
        &release,
        &config_contract,
        &config,
        &secret_provider,
        &deployment_binding(
            "staging",
            staging_config_state.environment_revision,
            &config,
            &staging_gateway.plan_digest,
            &local,
        ),
        DeploymentAdapterKind::Kubernetes,
    )
    .map_err(delivery_error)?;
    let mut staging_state =
        DeploymentState::new("staging", staging_config_state.environment_revision);
    let staging_receipt = apply_deployment(&mut staging_state, &staging_plan)
        .map_err(|error| delivery_error(error.issues))?;
    let provisional_staging_observation = observe_deployment_adapter(
        &staging_plan,
        &staging_receipt.receipt_id,
        "operator-observation:pending",
        &staging_receipt.release_id,
        &staging_receipt.release_digest,
        &staging_receipt.workload_digests,
        &staging_receipt.config_revision_id,
        true,
    );
    let operator_observation = actual_operator.as_ref().map_or_else(
        || {
            attest_operator_observation(
                lenso_service::operator_observation_claims_from_deployment(
                    &staging_plan,
                    &provisional_staging_observation,
                    workload_health(),
                ),
                "kubernetes-api:lenso-m5-kind",
                &operator_observation_provider(),
            )
            .map_err(|issue| delivery_error(vec![issue]))
        },
        |actual| {
            Ok(lenso_service::OperatorObservationAttestation {
                observation_id: actual.observation_id.clone(),
                observation_digest: actual.observation_digest.clone(),
                authority_id: actual.authority_id.clone(),
                authority_proof: actual.authority_proof.clone(),
                claims: actual.claims.clone(),
            })
        },
    )?;
    let staging_observation = actual_operator.as_ref().map_or_else(
        || {
            observe_deployment_adapter(
                &staging_plan,
                &staging_receipt.receipt_id,
                &operator_observation.observation_id,
                &staging_receipt.release_id,
                &staging_receipt.release_digest,
                &staging_receipt.workload_digests,
                &staging_receipt.config_revision_id,
                true,
            )
        },
        |actual| {
            let observed_workload_digests = actual
                .workloads
                .iter()
                .filter_map(|workload| {
                    workload
                        .observed_digest
                        .clone()
                        .map(|digest| (workload.workload_id.clone(), digest))
                })
                .collect();
            observe_deployment_adapter(
                &staging_plan,
                &staging_receipt.receipt_id,
                &actual.observation_id,
                &actual.observed_release_id,
                &actual.observed_release_digest,
                &observed_workload_digests,
                &actual.config_revision_id,
                actual.fresh && !actual.drifted && actual.decision == DeliveryDecision::Passed,
            )
        },
    );
    let actual_workload_health = actual_operator
        .as_ref()
        .map_or_else(workload_health, |actual| {
            actual
                .workloads
                .iter()
                .map(|workload| (workload.workload_id.clone(), workload.ready))
                .collect()
        });
    let gateway_observation = actual_gateway.as_ref().map_or_else(
        || {
            observe_gateway(
                &staging_gateway,
                staging_gateway.expected_gateway_revision,
                staging_observation.source_observation_id.clone(),
                true,
                &gateway_observation_provider(),
            )
            .map_err(|issue| delivery_error(vec![issue]))
        },
        |actual| {
            Ok(lenso_service::GatewayObservation {
                protocol: actual.protocol.clone(),
                observation_id: actual.observation_id.clone(),
                plan_id: actual.plan_id.clone(),
                plan_digest: actual.plan_digest.clone(),
                environment: actual.environment.clone(),
                release_id: actual.release_id.clone(),
                release_digest: actual.release_digest.clone(),
                resource_uid: actual.resource_uid.clone(),
                resource_version: actual.resource_version.clone(),
                authority_context: actual.authority_context.clone(),
                configuration_identity: actual.configuration_identity.clone(),
                revision: actual.revision,
                observed_after: actual.observed_after.clone(),
                fresh: actual.fresh
                    && actual.protocol == "lenso.gateway-observation.v1"
                    && actual.observed_after == operator_observation.observation_id,
                provider_id: actual.provider_id.clone(),
                provider_proof: actual.provider_proof.clone(),
            })
        },
    )?;
    let mut staging_evidence_references = vec![
        operator_observation.observation_id.clone(),
        operator_observation.observation_digest.clone(),
        format!(
            "operator-observation-authority:{}",
            operator_observation.authority_id
        ),
        format!(
            "operator-observation-proof:{}",
            operator_observation.authority_proof
        ),
    ];
    if let Some(actual) = &actual_gateway {
        staging_evidence_references.push(actual.observation_id.clone());
    }
    let verification = verify_staging_environment(
        EnvironmentVerificationInput {
            release: release.clone(),
            trust: trust.clone(),
            policy: local.clone(),
            policy_inputs: policy_inputs.clone(),
            config: config.clone(),
            deployment_plan: staging_plan.clone(),
            deployment: staging_receipt.clone(),
            deployment_observation: staging_observation.clone(),
            operator_observation: operator_observation.clone(),
            gateway_plan: staging_gateway.clone(),
            gateway_observation: gateway_observation.clone(),
            topology_digest: actual_operator.as_ref().map_or_else(
                || digest("staging-topology:r19"),
                |actual| digest(&actual.observation_digest),
            ),
            workload_health: actual_workload_health,
            evidence_references: staging_evidence_references,
            freshness_horizon_revision: 24,
        },
        &provider,
        &secret_provider,
        &operator_observation_provider(),
        &gateway_observation_provider(),
    );
    anyhow::ensure!(
        verification.decision == DeliveryDecision::Passed,
        "staging verification must pass"
    );

    let (production_edge, production_gateway) = gateway_plan(&release, &provider, "production", 9)?;
    let mut production_binding = deployment_binding(
        "production",
        31,
        &config,
        &production_gateway.plan_digest,
        &local,
    );
    production_binding.adapter_inputs.insert(
        "rollbackReleaseId".to_owned(),
        previous_release.release_id.clone(),
    );
    production_binding.adapter_inputs.insert(
        "resourceName".to_owned(),
        "service-support-production-canary".to_owned(),
    );
    let production_plan = plan_deployment(
        &release,
        &config_contract,
        &config,
        &secret_provider,
        &production_binding,
        DeploymentAdapterKind::Kubernetes,
    )
    .map_err(delivery_error)?;
    let promotion = plan_promotion(
        PromotionPlanInput {
            source: verification.clone(),
            target_deployment: production_plan.clone(),
            target_gateway: production_gateway.clone(),
            policy: local.clone(),
            policy_inputs: policy_inputs.clone(),
            source_environment_revision: staging_state.environment_revision,
            target_environment_revision: 31,
            target_topology_digest: digest("production-topology:r31"),
            secret_reference_ids: vec!["secret:support:database:v5".to_owned()],
            evidence_references: vec!["approval-input:m5-local".to_owned()],
        },
        &provider,
        &secret_provider,
        &operator_observation_provider(),
        &gateway_observation_provider(),
    )
    .map_err(delivery_error)?;
    let approval_authority = lenso_service::DeterministicPromotionApprovalAuthority::new(
        "production-release-operator",
        ["user:m5-release-engineer"],
        "ephemeral-m5-approval-key",
    );
    let protected_evidence = lenso_service::PromotionProtectedEvidence::from_plan(&promotion);
    let approval = approve_promotion(&promotion, "user:m5-release-engineer", &approval_authority)
        .map_err(|issue| delivery_error(vec![issue]))?;
    let mut promotion_state = PromotionState::new("production", 31);
    let mut production_state = DeploymentState::new("production", 31);
    let promotion_receipt = apply_promotion(
        &mut promotion_state,
        &mut production_state,
        &promotion,
        &approval,
        &protected_evidence,
        &approval_authority,
    )
    .map_err(|error| delivery_error(error.issues))?;

    let mut previous_state = DeploymentState::new("production", 31);
    let previous_receipt = apply_deployment(&mut previous_state, &previous_plan)
        .map_err(|error| delivery_error(error.issues))?;
    let previous_observation = actual_baseline_operator.as_ref().map_or_else(
        || observe_deployment(&previous_plan, &previous_receipt, true),
        |actual| {
            let workloads = actual
                .workloads
                .iter()
                .filter_map(|workload| {
                    workload
                        .observed_digest
                        .clone()
                        .map(|digest| (workload.workload_id.clone(), digest))
                })
                .collect();
            observe_deployment_adapter(
                &previous_plan,
                &previous_receipt.receipt_id,
                &actual.observation_id,
                &actual.observed_release_id,
                &actual.observed_release_digest,
                &workloads,
                &actual.config_revision_id,
                actual.fresh && !actual.drifted && actual.decision == DeliveryDecision::Passed,
            )
        },
    );
    let production_observation = actual_promoted_operator.as_ref().map_or_else(
        || {
            observe_deployment(
                &production_plan,
                &promotion_receipt.deployment_receipt,
                true,
            )
        },
        |actual| {
            let workloads = actual
                .workloads
                .iter()
                .filter_map(|workload| {
                    workload
                        .observed_digest
                        .clone()
                        .map(|digest| (workload.workload_id.clone(), digest))
                })
                .collect();
            observe_deployment_adapter(
                &production_plan,
                &promotion_receipt.deployment_receipt.receipt_id,
                &actual.observation_id,
                &actual.observed_release_id,
                &actual.observed_release_digest,
                &workloads,
                &actual.config_revision_id,
                actual.fresh && !actual.drifted && actual.decision == DeliveryDecision::Passed,
            )
        },
    );
    let previous_gateway_observation = actual_baseline_gateway.as_ref().map_or_else(
        || {
            observe_gateway(
                &previous_gateway,
                previous_gateway.expected_gateway_revision,
                previous_observation.source_observation_id.clone(),
                true,
                &gateway_observation_provider(),
            )
            .map_err(|issue| delivery_error(vec![issue]))
        },
        |actual| {
            Ok(lenso_service::GatewayObservation {
                protocol: actual.protocol.clone(),
                observation_id: actual.observation_id.clone(),
                plan_id: actual.plan_id.clone(),
                plan_digest: actual.plan_digest.clone(),
                environment: actual.environment.clone(),
                release_id: actual.release_id.clone(),
                release_digest: actual.release_digest.clone(),
                resource_uid: actual.resource_uid.clone(),
                resource_version: actual.resource_version.clone(),
                authority_context: actual.authority_context.clone(),
                configuration_identity: actual.configuration_identity.clone(),
                revision: actual.revision,
                observed_after: actual.observed_after.clone(),
                fresh: actual.fresh
                    && actual.protocol == "lenso.gateway-observation.v1"
                    && actual.observed_after == previous_observation.source_observation_id,
                provider_id: actual.provider_id.clone(),
                provider_proof: actual.provider_proof.clone(),
            })
        },
    )?;
    let canary_plan = plan_canary(
        CanaryPlanInput {
            release: release.clone(),
            production_deployment: production_plan.clone(),
            production_deployment_receipt: promotion_receipt.deployment_receipt.clone(),
            production_deployment_observation: production_observation.clone(),
            reliability_contract: reliability_contract(),
            policy: local.clone(),
            policy_inputs: policy_inputs.clone(),
            environment_verification: verification.clone(),
            previous_known_good_deployment: previous_plan.clone(),
            previous_known_good_receipt: previous_receipt.clone(),
            previous_known_good_observation: previous_observation.clone(),
            previous_known_good_release: previous_release.clone(),
            previous_known_good_policy: previous_policy.clone(),
            previous_known_good_policy_inputs: previous_policy_inputs.clone(),
            previous_known_good_gateway: previous_gateway.clone(),
            previous_known_good_gateway_observation: previous_gateway_observation.clone(),
            initial_percent: 10,
            maximum_percent: 50,
        },
        &provider,
        &secret_provider,
        &operator_observation_provider(),
        &gateway_observation_provider(),
    )
    .map_err(delivery_error)?;
    let mut canary_state = CanaryState::new(canary_plan.plan_id.clone());
    let reliability_provider = DeterministicReliabilityObservationProvider::new([
        (
            "support-runtime-reliability-adapter",
            "ephemeral-m5-reliability-key",
        ),
        (
            "kubernetes-http-reliability-adapter",
            "ephemeral-m5-reliability-key",
        ),
    ]);
    let mut raw_observations = actual_reliability.unwrap_or_else(|| {
        let mut failed = reliability_observation();
        failed.latency_p99_ms = Some(900);
        vec![failed]
    });
    anyhow::ensure!(
        !raw_observations.is_empty(),
        "actual canary requires at least one Reliability observation"
    );
    let mut sealed_observations = Vec::new();
    let mut decisions = Vec::new();
    for raw_observation in raw_observations.drain(..) {
        let sealed = seal_reliability_observation(
            &canary_plan,
            &production_observation,
            &reliability_provider,
            raw_observation,
        )
        .map_err(|issue| delivery_error(vec![issue]))?;
        let decision = evaluate_canary(
            &mut canary_state,
            &canary_plan,
            sealed.clone(),
            &reliability_provider,
        );
        sealed_observations.push(sealed);
        decisions.push(decision);
    }
    let breach_observation = sealed_observations
        .pop()
        .expect("at least one observation was sealed");
    let breach = decisions.pop().expect("at least one decision was recorded");
    anyhow::ensure!(
        breach.decision == DeliveryDecision::Blocked
            && breach.outcome == lenso_service::CanaryOutcome::Rollback,
        "final canary observation must require rollback"
    );
    let rollback_safety_provider = DeterministicRollbackSafetyProvider::new(
        "support-runtime-rollback-safety-adapter",
        "ephemeral-m5-rollback-safety-key",
    );
    let eligibility_input = &policy_inputs.eligibility_input;
    let rollback_safety = seal_rollback_safety_evidence(
        &canary_plan,
        &production_plan,
        &previous_plan,
        production_state.environment_revision,
        &rollback_safety_provider,
        RollbackSafetyInput {
            migrations_reversible: release
                .migrations
                .iter()
                .all(|migration| migration.reversible),
            destructive_changes_absent: release
                .migrations
                .iter()
                .all(|migration| !matches!(migration.phase.as_str(), "contract" | "irreversible")),
            workflows_downgrade_safe: eligibility_input.workflows.downgrade_safe == Some(true)
                && eligibility_input.rollback.workflow_compatible == Some(true),
            config_revision_compatible: eligibility_input.rollback.config_compatible == Some(true),
            secret_references_resolvable: eligibility_input.rollback.secret_references_compatible
                == Some(true)
                && previous_config
                    .secret_references
                    .iter()
                    .all(|reference| reference.status == SecretReferenceStatus::Resolved),
            edge_configuration_compatible: eligibility_input.edge_contract_valid == Some(true)
                && eligibility_input.rollback.edge_compatible == Some(true),
            adapter_recovery_complete: eligibility_input.rollback.adapter_capable == Some(true)
                && production_plan.rollback_capable
                && previous_plan.rollback_capable,
            policy_approved: local.decision == DeliveryDecision::Passed,
            evidence_references: vec![
                local.evidence_id.clone(),
                verification.verification_id.clone(),
                previous_observation.observation_id.clone(),
                breach_observation.observation_id.clone(),
            ],
        },
    )
    .map_err(|issue| delivery_error(vec![issue]))?;
    let rollback_plan = plan_rollback(
        &canary_plan,
        &breach,
        &breach_observation,
        &reliability_provider,
        &production_plan,
        &promotion.target_gateway,
        &previous_plan,
        &previous_gateway,
        production_state.environment_revision,
        rollback_safety,
        &rollback_safety_provider,
        &provider,
    )
    .map_err(delivery_error)?;
    let rollback_result = actual_rollback_operator
        .as_ref()
        .zip(actual_rollback_gateway.as_ref())
        .map(|(actual_operator, actual_gateway)| {
            let observed_workloads = actual_operator
                .workloads
                .iter()
                .filter_map(|workload| {
                    workload
                        .observed_digest
                        .clone()
                        .map(|digest| (workload.workload_id.clone(), digest))
                })
                .collect();
            let deployment_observation = observe_deployment_adapter(
                &previous_plan,
                &previous_receipt.receipt_id,
                &actual_operator.observation_id,
                &actual_operator.observed_release_id,
                &actual_operator.observed_release_digest,
                &observed_workloads,
                &actual_operator.config_revision_id,
                actual_operator.fresh
                    && !actual_operator.drifted
                    && actual_operator.decision == DeliveryDecision::Passed,
            );
            let gateway_observation = lenso_service::GatewayObservation {
                protocol: actual_gateway.protocol.clone(),
                observation_id: actual_gateway.observation_id.clone(),
                plan_id: actual_gateway.plan_id.clone(),
                plan_digest: actual_gateway.plan_digest.clone(),
                environment: actual_gateway.environment.clone(),
                release_id: actual_gateway.release_id.clone(),
                release_digest: actual_gateway.release_digest.clone(),
                resource_uid: actual_gateway.resource_uid.clone(),
                resource_version: actual_gateway.resource_version.clone(),
                authority_context: actual_gateway.authority_context.clone(),
                configuration_identity: actual_gateway.configuration_identity.clone(),
                revision: actual_gateway.revision,
                observed_after: actual_gateway.observed_after.clone(),
                fresh: actual_gateway.fresh
                    && actual_gateway.protocol == "lenso.gateway-observation.v1"
                    && actual_gateway.observed_after == actual_operator.observation_id,
                provider_id: actual_gateway.provider_id.clone(),
                provider_proof: actual_gateway.provider_proof.clone(),
            };
            let convergence = observe_rollback_convergence(
                &rollback_plan,
                &previous_plan,
                &previous_receipt,
                &deployment_observation,
                &previous_gateway,
                &gateway_observation,
                &provider,
                &gateway_observation_provider(),
                &rollback_safety_provider,
                vec![
                    actual_operator.observation_id.clone(),
                    actual_operator.observation_digest.clone(),
                    actual_gateway.observation_id.clone(),
                ],
            )
            .map_err(|issue| delivery_error(vec![issue]))?;
            let mut rollback_state = RollbackState::new(
                "production",
                production_plan.release_id.clone(),
                production_plan.config_revision_id.clone(),
                production_state.environment_revision,
                10,
            );
            let receipt = apply_rollback(
                &mut rollback_state,
                &rollback_plan,
                Some(&convergence),
                &rollback_safety_provider,
                "automation:m5-canary-controller",
            )
            .map_err(delivery_error)?;
            Ok::<_, anyhow::Error>((receipt, deployment_observation, gateway_observation))
        })
        .transpose()?;
    let (rollback, rollback_deployment_observation, rollback_gateway_observation) = rollback_result
        .map_or_else(
            || (None, None, None),
            |(receipt, deployment, gateway)| (Some(receipt), Some(deployment), Some(gateway)),
        );
    let config_rollback = rollback
        .as_ref()
        .map(|rollback_receipt| {
            let mut state =
                ConfigState::new("production", rollback_receipt.environment_revision_before);
            state.active_revision_id = Some(config.revision_id.clone());
            state.previous_revision_id = Some(previous_config.revision_id.clone());
            let plan = plan_config_activation(
                &state,
                &config_contract,
                &previous_config,
                &secret_provider,
                ConfigOperation::Rollback,
            )
            .map_err(delivery_error)?;
            apply_config_activation(&mut state, &plan).map_err(|error| delivery_error(error.issues))
        })
        .transpose()?;

    let observed_outage = actual_outage.as_ref();
    let (
        outage_deployment_plan,
        outage_deployment,
        mut outage_deployment_observation,
        actual_outage_operator,
    ) = if let Some(observation) = rollback_deployment_observation.clone() {
        (
            previous_plan.clone(),
            previous_receipt.clone(),
            observation,
            actual_rollback_operator.as_ref(),
        )
    } else {
        (
            production_plan.clone(),
            promotion_receipt.deployment_receipt.clone(),
            production_observation.clone(),
            actual_promoted_operator.as_ref(),
        )
    };
    let outage_operator_observation = if let Some(actual) = actual_outage_operator {
        operator_attestation(actual)
    } else {
        let provisional = observe_deployment_adapter(
            &outage_deployment_plan,
            &outage_deployment.receipt_id,
            "operator-observation:pending",
            &outage_deployment.release_id,
            &outage_deployment.release_digest,
            &outage_deployment.workload_digests,
            &outage_deployment.config_revision_id,
            true,
        );
        let attestation = attest_operator_observation(
            lenso_service::operator_observation_claims_from_deployment(
                &outage_deployment_plan,
                &provisional,
                workload_health(),
            ),
            "kubernetes-api:lenso-m5-kind",
            &operator_observation_provider(),
        )
        .map_err(|issue| delivery_error(vec![issue]))?;
        outage_deployment_observation = observe_deployment_adapter(
            &outage_deployment_plan,
            &outage_deployment.receipt_id,
            &attestation.observation_id,
            &outage_deployment.release_id,
            &outage_deployment.release_digest,
            &outage_deployment.workload_digests,
            &outage_deployment.config_revision_id,
            true,
        );
        attestation
    };
    let outage_observation = if let Some(observation) = observed_outage {
        observation.clone()
    } else {
        attest_coordination_outage(
            CoordinationOutageClaims {
                protocol: lenso_service::COORDINATION_OUTAGE_OBSERVATION_PROTOCOL.to_owned(),
                deployment_plan_id: outage_deployment_plan.plan_id.clone(),
                deployment_plan_digest: outage_deployment_plan.plan_digest.clone(),
                deployment_receipt_id: outage_deployment.receipt_id.clone(),
                deployment_observation_id: outage_deployment_observation.observation_id.clone(),
                operator_observation_id: outage_operator_observation.observation_id.clone(),
                operator_observation_digest: outage_operator_observation.observation_digest.clone(),
                environment_revision_after: outage_deployment.environment_revision_after,
                release_id: outage_deployment.release_id.clone(),
                release_digest: outage_deployment.release_digest.clone(),
                config_revision_id: outage_deployment.config_revision_id.clone(),
                system_plane_available: false,
                runtime_console_available: false,
                autonomous_service_running: true,
                selected_gateway_running: true,
                selected_transport_running: true,
                gateway_is_data_plane: true,
                gateway_requires_live_policy: false,
                gateway_requires_live_release_metadata: false,
                last_valid_config_revision_available: false,
                secret_provider_lease_valid: false,
                secret_rotation_policy_preserved: false,
                operation_results: [
                    DataPlaneOperation::DirectRequest,
                    DataPlaneOperation::Event,
                    DataPlaneOperation::DurableWorkflow,
                    DataPlaneOperation::Inbox,
                    DataPlaneOperation::Outbox,
                    DataPlaneOperation::Timer,
                    DataPlaneOperation::Retry,
                    DataPlaneOperation::Compensation,
                    DataPlaneOperation::RuntimeStory,
                ]
                .into_iter()
                .map(|operation| (operation, false))
                .collect(),
                security: SecurityContinuity {
                    workload_identity_enforced: false,
                    tenant_context_enforced: false,
                    call_policy_enforced: false,
                    service_authorization_enforced: false,
                },
                durable_checkpoint_id: String::new(),
                evidence_references: Vec::new(),
            },
            "data-plane-probe:lenso-m5-kind",
            &coordination_outage_provider(),
        )
        .map_err(|issue| delivery_error(vec![issue]))?
    };
    let outage = prove_system_plane_outage(
        CoordinationOutageInput {
            deployment_plan: outage_deployment_plan,
            deployment: outage_deployment,
            deployment_observation: outage_deployment_observation,
            operator_observation: outage_operator_observation,
            outage_observation,
        },
        &coordination_outage_provider(),
        &operator_observation_provider(),
    );
    if actual_outage.is_some() {
        anyhow::ensure!(
            observed_outage.is_some_and(|observation| {
                observation.protocol == lenso_service::COORDINATION_OUTAGE_OBSERVATION_PROTOCOL
                    && observation.claims.release_id == outage.release_id
                    && observation.claims.config_revision_id == outage.config_revision_id
            }) && rollback.is_some()
                && outage.decision == DeliveryDecision::Passed,
            "actual outage proof must pass"
        );
    }

    let rendered = serde_json::to_string(&(
        &release,
        &config,
        &promotion,
        &rollback_plan,
        &rollback,
        &outage,
    ))?;
    let redaction_proven = !rendered.contains("m5-plaintext-should-never-appear")
        && !rendered.contains("secretValue")
        && !rendered.contains("ephemeral-m5-key");
    anyhow::ensure!(
        redaction_proven,
        "acceptance evidence leaked sensitive material"
    );

    Ok(M5SmokeEvidence {
        artifact_version: "lenso.m5-production-delivery-core.v1".to_owned(),
        outcome: if actual_outage.is_some() && rollback.is_some() {
            "passed".to_owned()
        } else {
            "planning".to_owned()
        },
        public_seam: "support-system".to_owned(),
        service_release: release,
        trust,
        tampered_issue_codes: issue_codes(&tampered.issues),
        untrusted_issue_codes: issue_codes(&untrusted.issues),
        revoked_issue_codes: issue_codes(&revoked.issues),
        policy: M5PolicyEvidence {
            evaluation_input: policy_evaluation_input,
            local,
            ci_equivalent,
            system_plane,
            byte_equivalent,
            blocked_issue_codes: issue_codes(&unsafe_policy.issues),
        },
        config_revision: config,
        previous_config_revision: previous_config,
        config_stage,
        config_rollback,
        redaction_proven,
        staging_deployment_plan: staging_plan,
        staging_deployment_receipt: staging_receipt,
        staging_deployment_observation: staging_observation,
        production_deployment_plan: production_plan,
        previous_deployment_plan: previous_plan,
        previous_deployment_receipt: previous_receipt,
        previous_deployment_observation: previous_observation,
        staging_edge_contract: staging_edge,
        production_edge_contract: production_edge,
        previous_edge_contract: previous_edge,
        staging_gateway_plan: staging_gateway,
        staging_gateway_observation: gateway_observation,
        production_gateway_plan: promotion.target_gateway.clone(),
        previous_gateway_plan: previous_gateway,
        previous_gateway_observation,
        environment_verification: verification,
        promotion,
        promotion_approval: approval,
        promotion_protected_evidence: protected_evidence,
        promotion_receipt,
        canary_plan,
        production_deployment_observation: production_observation,
        canary_observations: canary_state.observations,
        canary_history: canary_state.decisions,
        rollback_plan,
        rollback,
        rollback_deployment_observation,
        rollback_gateway_observation,
        outage,
        migration_first_required: true,
        public_edge_paths: vec!["/v1/tickets/{ticketId}".to_owned()],
        internal_operations_private: true,
        prior_guarantees: "m4_acceptance".to_owned(),
        provider_compatibility: "independent_host_managed_smoke".to_owned(),
        local_requirements: vec![
            "docker".to_owned(),
            "kind".to_owned(),
            "kubectl".to_owned(),
            "cargo".to_owned(),
            "lenso-cli-m5".to_owned(),
        ],
    })
}

fn release_input(
    version: &str,
    config_contract: &lenso_service::ConfigContractDefinition,
    supply_chain: Option<&ActualReleaseSupplyChainEvidence>,
) -> ServiceReleaseInput {
    ServiceReleaseInput {
        service_id: "service:support".to_owned(),
        service_version: version.to_owned(),
        modules: vec![
            ReleaseModule {
                module_id: "support-ticket".to_owned(),
                module_version: "4.0.0".to_owned(),
            },
            ReleaseModule {
                module_id: "support-sla".to_owned(),
                module_version: "2.0.0".to_owned(),
            },
        ],
        workloads: vec![
            workload(
                version,
                "support-api",
                ReleaseWorkloadRole::Api,
                supply_chain,
            ),
            workload(
                version,
                "support-worker",
                ReleaseWorkloadRole::Worker,
                supply_chain,
            ),
            workload(
                version,
                "support-migration",
                ReleaseWorkloadRole::Migration,
                supply_chain,
            ),
        ],
        contract_versions: vec![ReleaseContractVersion {
            contract_id: "support-http".to_owned(),
            version: "v1".to_owned(),
            kind: "request_response".to_owned(),
            artifact: evidence("contracts/openapi/support.v1.yaml"),
        }],
        config_contract: DeliveryEvidenceReference {
            reference: config_contract.reference.clone(),
            digest: config_contract.digest.clone(),
        },
        reliability_contract: DeliveryEvidenceReference {
            reference: "contracts/reliability/support.v1.schema.json".to_owned(),
            digest: reliability_contract_digest(&reliability_contract()),
        },
        migrations: vec![ReleaseMigration {
            migration_id: "support-0001".to_owned(),
            phase: "expand".to_owned(),
            artifact: evidence("migration:support-0001"),
            reversible: true,
        }],
        workflow_compatibility: vec![evidence("workflow:support:v1")],
        verification_evidence: vec![evidence("verification:m4-support")],
        rollout_gates: vec![ReleaseRolloutGate {
            gate_id: "service-reliability".to_owned(),
            evidence_kind: "service_reliability".to_owned(),
            required: true,
        }],
        rollback: ReleaseRollbackConstraints {
            previous_release_required: true,
            automatic_allowed: true,
            blocked_by_irreversible_migration: true,
        },
        retention: ReleaseRetention {
            evidence_days: 90,
            artifact_days: 365,
        },
    }
}

fn workload(
    version: &str,
    workload_id: &str,
    role: ReleaseWorkloadRole,
    supply_chain: Option<&ActualReleaseSupplyChainEvidence>,
) -> WorkloadArtifact {
    let digest_key = if version == "5.0.0" {
        "M5_CANDIDATE_IMAGE_DIGEST"
    } else {
        "M5_PREVIOUS_IMAGE_DIGEST"
    };
    let artifact_digest =
        std::env::var(digest_key).unwrap_or_else(|_| digest(&format!("{workload_id}:{version}")));
    let artifact_reference = format!("registry.example.test/lenso/{workload_id}");
    WorkloadArtifact {
        workload_id: workload_id.to_owned(),
        role,
        artifact_reference,
        artifact_digest: artifact_digest.clone(),
        media_type: "application/vnd.oci.image.manifest.v1+json".to_owned(),
        display_tag: Some(version.to_owned()),
        sbom: supply_chain.map_or_else(
            || evidence(&format!("sbom:{workload_id}")),
            |evidence| DeliveryEvidenceReference {
                reference: evidence.sbom_reference.clone(),
                digest: evidence.sbom_digest.clone(),
            },
        ),
        provenance: ReleaseProvenance {
            reference: supply_chain.map_or_else(
                || format!("provenance:{workload_id}"),
                |evidence| evidence.provenance_reference.clone(),
            ),
            digest: supply_chain.map_or_else(
                || digest(&format!("provenance:{workload_id}")),
                |evidence| evidence.provenance_digest.clone(),
            ),
            source: supply_chain.map_or_else(
                || "https://github.com/LioRael/lenso-examples".to_owned(),
                |evidence| evidence.source.clone(),
            ),
            builder: supply_chain.map_or_else(
                || "https://github.com/LioRael/lenso-examples/actions".to_owned(),
                |evidence| evidence.builder.clone(),
            ),
            input_digests: supply_chain.map_or_else(
                || vec![digest("support-system-source")],
                |evidence| evidence.input_digests.clone(),
            ),
            subject_digests: vec![supply_chain.map_or_else(
                || artifact_digest.clone(),
                |evidence| evidence.subject_digest.clone(),
            )],
        },
        signature_subject: format!("workload:{workload_id}"),
    }
}

fn deployment_binding(
    environment: &str,
    revision: u64,
    config: &lenso_service::ConfigRevision,
    gateway_plan_digest: &str,
    policy: &PolicyEvidence,
) -> DeploymentEnvironmentBinding {
    DeploymentEnvironmentBinding {
        environment: environment.to_owned(),
        expected_environment_revision: revision,
        config_revision_id: config.revision_id.clone(),
        secret_reference_ids: config
            .secret_references
            .iter()
            .map(|reference| reference.reference_id.clone())
            .collect(),
        endpoints: BTreeMap::from([(
            "public".to_owned(),
            format!("https://{environment}.support.example.test"),
        )]),
        placement: BTreeMap::from([(
            "topology.kubernetes.io/zone".to_owned(),
            "acceptance".to_owned(),
        )]),
        workloads: vec![
            DeploymentWorkloadSettings {
                workload_id: "support-api".to_owned(),
                replicas: 1,
                port: Some(8080),
                command: m5_data_plane_command(),
                health_path: Some("/health".to_owned()),
                disruption_min_available: Some(1),
            },
            DeploymentWorkloadSettings {
                workload_id: "support-worker".to_owned(),
                replicas: 1,
                port: None,
                command: vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    "while true; do sleep 30; done".to_owned(),
                ],
                health_path: None,
                disruption_min_available: Some(1),
            },
            DeploymentWorkloadSettings {
                workload_id: "support-migration".to_owned(),
                replicas: 1,
                port: None,
                command: vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    "sleep 4; echo migration-complete".to_owned(),
                ],
                health_path: None,
                disruption_min_available: None,
            },
        ],
        adapter_inputs: BTreeMap::new(),
        gateway_plan_digest: gateway_plan_digest.to_owned(),
        policy_evidence_references: vec![
            policy.evidence_id.clone(),
            policy.evidence_digest.clone(),
        ],
    }
}

fn m5_data_plane_command() -> Vec<String> {
    vec!["/usr/local/bin/support-system-m5-data-plane".to_owned()]
}

fn gateway_plan(
    release: &ServiceRelease,
    provider: &DeterministicTrustProvider,
    environment: &str,
    revision: u64,
) -> anyhow::Result<(
    lenso_service::EdgeContract,
    lenso_service::GatewayConfigurationPlan,
)> {
    let contract_digest = release
        .contract_versions
        .iter()
        .find(|contract| contract.contract_id == "support-http" && contract.version == "v1")
        .map(|contract| contract.artifact.digest.clone())
        .ok_or_else(|| anyhow::anyhow!("authoritative support-http contract is missing"))?;
    let operations = [
        EdgeServiceOperation {
            contract_id: "support-http".to_owned(),
            contract_version: "v1".to_owned(),
            contract_digest: contract_digest.clone(),
            operation_id: "getTicket".to_owned(),
            visibility: EdgeOperationVisibility::PublicEligible,
            request_schema_reference: "schema:support:getTicket:request".to_owned(),
            response_schema_reference: "schema:support:getTicket:response".to_owned(),
        },
        EdgeServiceOperation {
            contract_id: "support-http".to_owned(),
            contract_version: "v1".to_owned(),
            contract_digest,
            operation_id: "adminRepair".to_owned(),
            visibility: EdgeOperationVisibility::Internal,
            request_schema_reference: "schema:support:adminRepair:request".to_owned(),
            response_schema_reference: "schema:support:adminRepair:response".to_owned(),
        },
    ];
    let edge = build_edge_contract(
        release,
        &operations,
        "ci:m5-acceptance",
        provider,
        vec![EdgeRoute {
            contract_id: "support-http".to_owned(),
            contract_version: "v1".to_owned(),
            operation_id: "getTicket".to_owned(),
            public_path: "/v1/tickets/{ticketId}".to_owned(),
            authentication: EdgeAuthentication::WorkloadOrUser,
            cors: lenso_service::CorsIntent {
                allowed_origins: vec![format!("https://{environment}.support.example.test")],
                allowed_methods: vec!["GET".to_owned()],
            },
            rate: RateIntent {
                requests: 100,
                window_seconds: 60,
            },
            deprecated: false,
        }],
    )
    .map_err(delivery_error)?;
    let gateway = plan_gateway_configuration(
        &edge,
        provider,
        &GatewayEnvironmentBinding {
            environment: environment.to_owned(),
            gateway_adapter: "acceptance-nginx".to_owned(),
            public_origin: format!("https://{environment}.support.example.test"),
            expected_gateway_revision: revision,
        },
        None,
        &gateway_observation_provider(),
    )
    .map_err(delivery_error)?;
    Ok((edge, gateway))
}

#[derive(Clone)]
struct RollbackTargetEvidence {
    release_id: String,
    release_digest: String,
    deployment_plan_id: String,
    deployment_plan_digest: String,
    config_revision_id: String,
    config_revision_digest: String,
    secret_reference_ids: Vec<String>,
    gateway_plan_id: String,
    gateway_plan_digest: String,
    gateway_configuration_identity: String,
    adapter: String,
}

impl RollbackTargetEvidence {
    fn synthetic_prior() -> Self {
        Self {
            release_id: "service-release:prior-4-8".to_owned(),
            release_digest: digest("release:4.8.0"),
            deployment_plan_id: "deployment-plan:prior-4-8".to_owned(),
            deployment_plan_digest: digest("deployment:4.8.0"),
            config_revision_id: "config-revision:prior-4-8".to_owned(),
            config_revision_digest: digest("config:4.8.0"),
            secret_reference_ids: vec!["secret:support:database:v3".to_owned()],
            gateway_plan_id: "gateway-plan:prior-4-8".to_owned(),
            gateway_plan_digest: digest("gateway:4.8.0"),
            gateway_configuration_identity: digest("gateway-configuration:4.8.0"),
            adapter: "kubernetes".to_owned(),
        }
    }

    fn from_actual(
        release: &ServiceRelease,
        deployment: &DeploymentPlan,
        config: &lenso_service::ConfigRevision,
        gateway: &lenso_service::GatewayConfigurationPlan,
    ) -> Self {
        Self {
            release_id: release.release_id.clone(),
            release_digest: release.release_digest.clone(),
            deployment_plan_id: deployment.plan_id.clone(),
            deployment_plan_digest: deployment.plan_digest.clone(),
            config_revision_id: config.revision_id.clone(),
            config_revision_digest: config.revision_digest.clone(),
            secret_reference_ids: config
                .secret_references
                .iter()
                .map(|reference| reference.reference_id.clone())
                .collect(),
            gateway_plan_id: gateway.plan_id.clone(),
            gateway_plan_digest: gateway.plan_digest.clone(),
            gateway_configuration_identity: gateway.configuration_identity.clone(),
            adapter: "kubernetes".to_owned(),
        }
    }
}

fn safe_eligibility(
    release: &ServiceRelease,
    provider: &DeterministicTrustProvider,
    rollback_target: RollbackTargetEvidence,
) -> anyhow::Result<ProductionEligibilityInput> {
    let input = ProductionEligibilityInput {
        release_id: String::new(),
        release_digest: String::new(),
        provider_id: String::new(),
        provider_proof: String::new(),
        system_graph_digest: digest("support-system:m5"),
        contracts: release
            .contract_versions
            .iter()
            .map(|contract| lenso_service::ContractCompatibilityInput {
                contract_id: contract.contract_id.clone(),
                current_major: 1,
                candidate_major: contract
                    .version
                    .trim_start_matches('v')
                    .split('.')
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or_default(),
                compatible: Some(true),
                active_consumers: vec!["service:portal".to_owned()],
                consumer_migration_evidence: true,
                retiring: false,
                deprecation_window_complete: false,
            })
            .collect(),
        migrations: release
            .migrations
            .iter()
            .enumerate()
            .map(
                |(index, migration)| lenso_service::MigrationCompatibilityInput {
                    migration_id: migration.migration_id.clone(),
                    lineage_id: migration.migration_id.clone(),
                    sequence: u32::try_from(index + 1).expect("migration sequence fits u32"),
                    phase: match migration.phase.as_str() {
                        "expand" => lenso_service::MigrationPhase::Expand,
                        "backfill" => lenso_service::MigrationPhase::Backfill,
                        "verify" => lenso_service::MigrationPhase::Verify,
                        "contract" => lenso_service::MigrationPhase::Contract,
                        _ => lenso_service::MigrationPhase::Irreversible,
                    },
                    verified: true,
                },
            )
            .collect(),
        workflows: WorkflowCompatibilityInput {
            new_starts_compatible: Some(true),
            in_flight_compatible: Some(true),
            downgrade_safe: Some(true),
        },
        rollback: RollbackCompatibilityInput {
            prior_release_compatible: Some(true),
            schema_compatible: Some(true),
            workflow_compatible: Some(true),
            config_compatible: Some(true),
            secret_references_compatible: Some(true),
            edge_compatible: Some(true),
            adapter_capable: Some(true),
            previous_release_id: rollback_target.release_id,
            previous_release_digest: rollback_target.release_digest,
            previous_deployment_plan_id: rollback_target.deployment_plan_id,
            previous_deployment_plan_digest: rollback_target.deployment_plan_digest,
            previous_config_revision_id: rollback_target.config_revision_id,
            previous_config_revision_digest: rollback_target.config_revision_digest,
            previous_secret_reference_ids: rollback_target.secret_reference_ids,
            previous_gateway_plan_id: rollback_target.gateway_plan_id,
            previous_gateway_plan_digest: rollback_target.gateway_plan_digest,
            previous_gateway_configuration_identity: rollback_target.gateway_configuration_identity,
            previous_adapter: rollback_target.adapter,
        },
        provider_compatibility_verified: Some(true),
        workload_identity_production: Some(true),
        tenancy_mode_production: Some(true),
        tenant_context_enforced: Some(true),
        call_policies_declared: Some(true),
        dependencies_ready: Some(true),
        resilience_declared: Some(true),
        reliability_contract_complete: Some(true),
        edge_contract_valid: Some(true),
        environment_verification_fresh: Some(true),
    };
    attest_production_eligibility_input(release, provider, "ci:m5-acceptance", input)
        .map_err(|issue| delivery_error(vec![issue]))
}

fn reliability_contract() -> DeliveryReliabilityContract {
    DeliveryReliabilityContract {
        protocol: "lenso.reliability-contract.v1".to_owned(),
        contract_id: "reliability:support:v1".to_owned(),
        minimum_observation_seconds: 5,
        minimum_sample_count: 20,
        minimum_availability_basis_points: 9_950,
        maximum_latency_p99_ms: 500,
        maximum_error_budget_used_basis_points: 500,
        maximum_queue_backlog: 100,
        maximum_workflow_backlog: 50,
        maximum_timer_lag_ms: 2_000,
        maximum_retry_exhaustion: 2,
        maximum_compensation_pressure: 2,
        minimum_healthy_failure_domains: 1,
        dependencies: vec![DependencyReliability {
            dependency_id: "database".to_owned(),
            criticality: DependencyCriticality::Critical,
            allowed_degraded_modes: Vec::new(),
        }],
    }
}

fn operator_observation_provider() -> Ed25519OperatorObservationAuthorityProvider {
    Ed25519OperatorObservationAuthorityProvider::from_base64_private_keys([(
        "kubernetes-api:lenso-m5-kind",
        std::env::var("M5_OPERATOR_OBSERVATION_PRIVATE_KEY")
            .expect("M5 Operator observation private key must be provided to the evidence runner"),
    )])
    .expect("M5 Operator observation private key must be valid")
}

fn gateway_observation_provider() -> Ed25519GatewayObservationProvider {
    Ed25519GatewayObservationProvider::from_base64_private_key(
        "gateway-api:lenso-m5-kind",
        &std::env::var("M5_GATEWAY_OBSERVATION_PRIVATE_KEY")
            .expect("M5 Gateway observation private key must be provided to the evidence runner"),
    )
    .expect("M5 Gateway observation private key must be valid")
}

fn coordination_outage_provider() -> DeterministicCoordinationAuthorityProvider {
    DeterministicCoordinationAuthorityProvider::new([(
        "data-plane-probe:lenso-m5-kind",
        "ephemeral-m5-outage-observation-key",
    )])
}

fn operator_attestation(actual: &ActualOperatorObservation) -> OperatorObservationAttestation {
    OperatorObservationAttestation {
        observation_id: actual.observation_id.clone(),
        observation_digest: actual.observation_digest.clone(),
        authority_id: actual.authority_id.clone(),
        authority_proof: actual.authority_proof.clone(),
        claims: actual.claims.clone(),
    }
}

fn reliability_observation() -> ReliabilityObservation {
    ReliabilityObservation {
        protocol: String::new(),
        observation_id: String::new(),
        canary_plan_id: String::new(),
        canary_plan_digest: String::new(),
        release_id: String::new(),
        release_digest: String::new(),
        environment: String::new(),
        deployment_plan_id: String::new(),
        deployment_plan_digest: String::new(),
        deployment_observation_id: String::new(),
        collector_id: "support-runtime-reliability-adapter".to_owned(),
        collector_proof: String::new(),
        observed_revision: 32,
        freshness_horizon_revision: 40,
        fresh: true,
        observation_window_seconds: 5,
        sample_count: 20,
        generic_process_healthy: true,
        workload_readiness: workload_health(),
        workload_liveness: workload_health(),
        availability_basis_points: Some(9_999),
        latency_p99_ms: Some(120),
        error_budget_used_basis_points: Some(40),
        queue_backlog: Some(4),
        workflow_backlog: Some(2),
        timer_lag_ms: Some(100),
        retry_exhaustion: Some(0),
        compensation_pressure: Some(0),
        dependencies: vec![DependencyReliabilityObservation {
            dependency_id: "database".to_owned(),
            available: true,
            active_degraded_mode: None,
        }],
        failure_domains: BTreeMap::from([("acceptance".to_owned(), true)]),
        scaling_check_passed: Some(true),
        disruption_check_passed: Some(true),
        availability_check_passed: Some(true),
        evidence_references: vec!["runtime-story:canary:m5".to_owned()],
    }
}

fn workload_health() -> BTreeMap<String, bool> {
    BTreeMap::from([
        ("support-api".to_owned(), true),
        ("support-worker".to_owned(), true),
        ("support-migration".to_owned(), true),
    ])
}

fn evidence(reference: &str) -> DeliveryEvidenceReference {
    DeliveryEvidenceReference {
        reference: reference.to_owned(),
        digest: digest(reference),
    }
}

fn digest(value: &str) -> String {
    extraction_input_digest(value.as_bytes())
}

fn read_optional_json<T: for<'de> Deserialize<'de>>(
    environment_key: &str,
) -> anyhow::Result<Option<T>> {
    let Some(path) = std::env::var_os(environment_key) else {
        return Ok(None);
    };
    let bytes = std::fs::read(&path)?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}

fn issue_codes(issues: &[lenso_service::DeliveryIssue]) -> Vec<String> {
    issues
        .iter()
        .filter_map(|issue| serde_json::to_value(issue.code).ok())
        .filter_map(|code| code.as_str().map(str::to_owned))
        .collect()
}

fn delivery_error(issues: Vec<lenso_service::DeliveryIssue>) -> anyhow::Error {
    anyhow::anyhow!(
        serde_json::to_string_pretty(&issues).unwrap_or_else(|_| "delivery error".to_owned())
    )
}
