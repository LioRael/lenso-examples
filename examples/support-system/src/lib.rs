use anyhow::{Context, ensure};
use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use http::StatusCode;
use lenso_service::{
    CallPolicyDeclaration, CallPolicyRuntime, DirectGrpcBindings, DirectGrpcCallError,
    DirectGrpcClient, DirectHttpBindings, DirectHttpCall, DirectHttpClient, DirectHttpResponse,
    DirectHttpServerBinding, Endpoint, EndpointResolutionError, EndpointResolver, EndpointState,
    ExtractionBehaviorObservation, ExtractionDrainSnapshot, LastValidEndpointResolver,
    ServiceReference, StaticEndpointResolver, generate_direct_grpc_bindings,
    generate_direct_http_bindings,
    support_grpc_v1::{
        GetSlaRequest, ProbeSlaRequest, SlaResponse, UpdateSlaRequest,
        support_service_server::{SupportService, SupportServiceServer},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncWriteExt;
use tonic::{Request, Response, Status};

mod m2;
mod m3;
mod m4;
mod m5;
pub use m2::{
    M2ConsumerRequest, M2ProducerEvidence, M2ProducerRequest, M2SmokeEvidence, run_m2_consumer,
    run_m2_producer, run_m2_smoke,
};
pub use m3::{
    M3SmokeEvidence, M5DurableOperationProof, PreparedM5DurableOperation,
    prepare_m5_durable_operation, resume_m5_durable_operation, run_m3_smoke,
    run_m5_durable_operation_proof,
};
pub use m4::{M4SmokeEvidence, run_m4_smoke};
pub use m5::{M5SmokeEvidence, run_m5_smoke};

const TICKET_SERVICE: &str = "support-ticket-service";
const SLA_SERVICE: &str = "support-sla-service";
const TICKET_ENDPOINT: &str = "http://127.0.0.1:4210";
const SLA_ENDPOINT: &str = "http://127.0.0.1:4211";
const SLA_HEALTH_ENDPOINT: &str = "127.0.0.1:4212";
const CONTRACT_DEADLINE_MS: u64 = 30_000;
const IDEMPOTENCY_KEY: &str = "ticket-42:update";
const SANDBOX_STATE: &str = ".lenso/system-sandbox/support-platform/state.json";
const LOCAL_GRPC_DESCRIPTOR: &[u8] = tonic::include_file_descriptor_set!("support_descriptor");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Service {
    Ticket,
    Sla,
}

impl Service {
    fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            TICKET_SERVICE => Ok(Self::Ticket),
            SLA_SERVICE => Ok(Self::Sla),
            _ => anyhow::bail!("unknown Service `{value}`"),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Ticket => TICKET_SERVICE,
            Self::Sla => SLA_SERVICE,
        }
    }

    const fn store_id(self) -> &'static str {
        match self {
            Self::Ticket => "support-ticket-store",
            Self::Sla => "support-sla-store",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkloadRole {
    Api,
    Worker,
    Migration,
}

impl WorkloadRole {
    fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "api" => Ok(Self::Api),
            "worker" => Ok(Self::Worker),
            "migrate" => Ok(Self::Migration),
            _ => anyhow::bail!("unknown Workload role `{value}`"),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Worker => "worker",
            Self::Migration => "migrate",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmokeEvidence {
    pub ticket_service_reference: String,
    pub sla_service_reference: String,
    pub ticket_endpoint: String,
    pub sla_endpoint: String,
    pub deadline_unix_ms: u64,
    pub idempotency_key: String,
    pub http_decision: String,
    pub grpc_decision: String,
    pub unsafe_retry_decision: String,
    pub unsafe_retry_attempts: u32,
    pub calls_before_plane_withheld: u32,
    pub calls_after_plane_withheld: u32,
    pub system_plane_withheld: bool,
    pub runtime_console_withheld: bool,
    pub successful_story_segments: u32,
    pub ticket_workload_identity: String,
    pub sla_workload_identity: String,
    pub ticket_store_id: String,
    pub sla_store_id: String,
    pub stores_isolated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkloadEvidence {
    service_id: String,
    workload_id: String,
    workload_identity: String,
    store_id: String,
    store_path: String,
    kind: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkloadScenarioObservation {
    artifact_version: &'static str,
    outcome: &'static str,
    attempts: u32,
    retry_attempted: bool,
    retry_reason: &'static str,
    controlled_time_end_ms: u64,
    final_health: &'static str,
    health_reason: &'static str,
}

#[derive(Debug, Clone)]
struct SandboxEndpointResolver {
    state_path: PathBuf,
}

impl SandboxEndpointResolver {
    fn new(state_path: PathBuf) -> Self {
        Self { state_path }
    }
}

impl EndpointResolver for SandboxEndpointResolver {
    fn resolve(
        &self,
        service: &ServiceReference,
    ) -> Result<EndpointState, EndpointResolutionError> {
        let source = std::fs::read(&self.state_path).map_err(|error| {
            EndpointResolutionError::source_unavailable(
                service,
                format!("System Plane endpoint state is unavailable: {error}"),
            )
        })?;
        let state: Value = serde_json::from_slice(&source).map_err(|error| {
            EndpointResolutionError::source_unavailable(
                service,
                format!("System Plane endpoint state is invalid: {error}"),
            )
        })?;
        let endpoint = state["endpoints"]
            .as_array()
            .and_then(|endpoints| {
                endpoints
                    .iter()
                    .find(|endpoint| endpoint["serviceId"].as_str() == Some(service.as_str()))
            })
            .and_then(|endpoint| endpoint["endpoint"].as_str())
            .ok_or_else(|| {
                EndpointResolutionError::source_unavailable(
                    service,
                    "System Plane has no endpoint for the requested Service Reference",
                )
            })?;
        Ok(EndpointState::new(
            service.clone(),
            vec![Endpoint::new(endpoint)],
        ))
    }
}

pub async fn run_workload(service: &str, role: &str) -> anyhow::Result<()> {
    let service = Service::parse(service)?;
    let role = WorkloadRole::parse(role)?;
    let environment = workload_environment(service, role)?;
    match (service, role) {
        (_, WorkloadRole::Migration) => migrate(&environment).await,
        (_, WorkloadRole::Worker) => worker(service, &environment).await,
        (Service::Ticket, WorkloadRole::Api) => ticket_api(environment).await,
        (Service::Sla, WorkloadRole::Api) => sla_api(environment).await,
    }
}

pub async fn run_scenario() -> anyhow::Result<()> {
    let fault = std::env::var("LENSO_SANDBOX_FAULT_KIND")?;
    ensure!(fault == "timeout", "unsupported support scenario fault");
    let delay_ms = std::env::var("LENSO_SANDBOX_DELAY_MS")?.parse::<u64>()?;
    let deadline_ms = std::env::var("LENSO_SANDBOX_DEADLINE_MS")?.parse::<u64>()?;
    ensure!(delay_ms >= deadline_ms, "timeout must exhaust its Deadline");

    let attempts = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let observed_attempts = attempts.clone();
    let server =
        DirectHttpServerBinding::new_without_workload_identity(http_bindings()?, move |_| {
            let observed_attempts = observed_attempts.clone();
            async move {
                observed_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                DirectHttpResponse::json(StatusCode::OK, json!({"ok": true}))
            }
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, server.router()).await });
    let error = DirectHttpClient::new(resolver(TICKET_SERVICE, &endpoint)?, http_bindings()?)
        .call(
            &ServiceReference::new(TICKET_SERVICE),
            update_ticket_call(
                "deadline-proof",
                "Deadline proof",
                now_ms() + deadline_ms,
                "deadline-proof:update",
            ),
        )
        .await
        .expect_err("generated-client call must reach its Deadline");
    server.abort();
    ensure!(
        error.to_string().contains("transport_failure_no_retry"),
        "generated client did not report Deadline expiry"
    );
    let attempts = attempts.load(std::sync::atomic::Ordering::SeqCst);
    ensure!(
        attempts == 1,
        "Deadline proof must observe exactly one attempt"
    );
    println!(
        "{}",
        serde_json::to_string(&WorkloadScenarioObservation {
            artifact_version: "lenso.sandbox-workload-observation.v1",
            outcome: "deadline_exceeded",
            attempts,
            retry_attempted: false,
            retry_reason: "deadline_exhausted",
            controlled_time_end_ms: delay_ms,
            final_health: "ready",
            health_reason: "generated_client_deadline_expired",
        })?
    );
    Ok(())
}

fn workload_environment(service: Service, role: WorkloadRole) -> anyhow::Result<WorkloadEvidence> {
    let service_id = service.as_str();
    let workload_id = std::env::var("LENSO_WORKLOAD_ID").context("LENSO_WORKLOAD_ID")?;
    let expected_workload = format!("{service_id}-{}", role.as_str());
    ensure!(
        std::env::var("LENSO_SERVICE_ID").as_deref() == Ok(service_id),
        "Service identity mismatch"
    );
    ensure!(
        workload_id == expected_workload,
        "Workload identity mismatch"
    );
    let workload_identity =
        std::env::var("LENSO_WORKLOAD_IDENTITY").context("LENSO_WORKLOAD_IDENTITY")?;
    ensure!(
        workload_identity == format!("local-dev://support-platform/{service_id}/{workload_id}"),
        "unexpected development Workload Identity"
    );
    let store_id = std::env::var("LENSO_SERVICE_STORE_ID").context("LENSO_SERVICE_STORE_ID")?;
    ensure!(
        store_id == service.store_id(),
        "Service Store identity mismatch"
    );
    let store = std::env::var("LENSO_SERVICE_STORE_PATH").context("LENSO_SERVICE_STORE_PATH")?;
    Ok(WorkloadEvidence {
        service_id: service_id.to_owned(),
        workload_id,
        workload_identity,
        store_id,
        store_path: store,
        kind: role.as_str().to_owned(),
    })
}

async fn migrate(environment: &WorkloadEvidence) -> anyhow::Result<()> {
    let store = Path::new(&environment.store_path);
    tokio::fs::create_dir_all(store).await?;
    write_json(store.join("migration.json"), environment).await
}

async fn worker(service: Service, environment: &WorkloadEvidence) -> anyhow::Result<()> {
    verify_store_owner(environment).await?;
    write_json(
        Path::new(&environment.store_path).join("worker.json"),
        environment,
    )
    .await?;
    let evidence_path = PathBuf::from(&environment.store_path);
    let (address, app) = match service {
        Service::Ticket => (
            "127.0.0.1:4213",
            Router::new()
                .route("/health/ready", get(|| async { StatusCode::OK }))
                .route("/m2/events/produce", post(m2_produce))
                .with_state(evidence_path),
        ),
        Service::Sla => (
            "127.0.0.1:4214",
            Router::new()
                .route("/health/ready", get(|| async { StatusCode::OK }))
                .route("/m2/events/consume", post(m2_consume))
                .with_state(evidence_path),
        ),
    };
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn m2_produce(
    State(store_path): State<PathBuf>,
    Json(request): Json<M2ProducerRequest>,
) -> Result<Json<M2ProducerEvidence>, (StatusCode, String)> {
    let evidence = run_m2_producer(request).await.map_err(m2_handler_error)?;
    write_json(store_path.join("m2-event-producer.json"), &evidence)
        .await
        .map_err(m2_handler_error)?;
    Ok(Json(evidence))
}

async fn m2_consume(
    State(store_path): State<PathBuf>,
    Json(request): Json<M2ConsumerRequest>,
) -> Result<Json<m2::EventFlowEvidence>, (StatusCode, String)> {
    let evidence = run_m2_consumer(request).await.map_err(m2_handler_error)?;
    write_json(store_path.join("m2-event-consumer.json"), &evidence)
        .await
        .map_err(m2_handler_error)?;
    Ok(Json(evidence))
}

fn m2_handler_error(error: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

async fn ticket_api(environment: WorkloadEvidence) -> anyhow::Result<()> {
    verify_store_owner(&environment).await?;
    let evidence_path = Path::new(&environment.store_path).join("operations.jsonl");
    let mutation_gate = Path::new(&environment.store_path).join("mutations.paused");
    let workload_identity = environment.workload_identity.clone();
    let grpc = std::sync::Arc::new(DirectGrpcClient::new(
        LastValidEndpointResolver::new(SandboxEndpointResolver::new(fixture_path(SANDBOX_STATE))),
        grpc_bindings()?,
    ));
    let binding =
        DirectHttpServerBinding::new_without_workload_identity(http_bindings()?, move |request| {
            let evidence_path = evidence_path.clone();
            let workload_identity = workload_identity.clone();
            let grpc = grpc.clone();
            let mutation_gate = mutation_gate.clone();
            async move {
                if tokio::fs::try_exists(&mutation_gate).await.unwrap_or(true) {
                    return DirectHttpResponse::json(
                        StatusCode::CONFLICT,
                        json!({"error":"linked_mutations_paused"}),
                    );
                }
                let deadline = request.deadline_unix_ms.unwrap_or_default();
                let key = request.idempotency_key.clone().unwrap_or_default();
                let payload = request.body.to_vec();
                match grpc
                    .update_sla(&ServiceReference::new(SLA_SERVICE), payload, deadline, &key)
                    .await
                {
                    Ok(response) => {
                        let record = json!({
                            "artifactVersion": "lenso.story-segment.v1",
                            "storyId": format!("direct-call:{key}"),
                            "segmentId": format!("{TICKET_SERVICE}:updateTicket:{key}"),
                            "kind": "direct_service_call",
                            "outcome": "succeeded",
                            "operation": "updateTicket",
                            "serviceReference": SLA_SERVICE,
                            "resolvedEndpoint": SLA_ENDPOINT,
                            "deadlineUnixMs": deadline,
                            "idempotencyKey": key,
                            "workloadIdentity": workload_identity,
                            "grpcDecision": response.evidence.decision,
                            "grpcAttempts": response.evidence.attempts,
                        });
                        if let Err(error) = append_json(&evidence_path, &record).await {
                            return DirectHttpResponse::json(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                json!({"error": error.to_string()}),
                            );
                        }
                        let sla: Value = serde_json::from_slice(&response.payload)
                            .unwrap_or_else(|_| json!({"error":"invalid SLA response"}));
                        DirectHttpResponse::json(
                            StatusCode::OK,
                            json!({
                                "ticketId": ticket_id(&request.path),
                                "sla": sla,
                                "slaServiceReference": SLA_SERVICE,
                                "slaEndpoint": SLA_ENDPOINT,
                                "grpcDecision": response.evidence.decision,
                                "workloadIdentity": workload_identity,
                            }),
                        )
                    }
                    Err(error) => DirectHttpResponse::json(
                        StatusCode::BAD_GATEWAY,
                        json!({"error": error.to_string()}),
                    ),
                }
            }
        });
    let app = Router::new()
        .route("/health/ready", get(|| async { StatusCode::OK }))
        .merge(binding.router());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:4210").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn ticket_id(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or_default()
}

#[derive(Debug, Clone)]
struct SlaApi {
    evidence_path: PathBuf,
    workload_identity: String,
    call_policy_runtime: CallPolicyRuntime,
    get_sla_policy: CallPolicyDeclaration,
}

#[tonic::async_trait]
impl SupportService for SlaApi {
    async fn get_sla(
        &self,
        request: Request<GetSlaRequest>,
    ) -> Result<Response<SlaResponse>, Status> {
        let permit = match self
            .call_policy_runtime
            .admit("support-grpc:GetSla", &self.get_sla_policy)
        {
            Ok(permit) => permit,
            Err(event) => {
                append_json(
                    &self.evidence_path,
                    &json!({
                        "artifactVersion": "lenso.story-segment.v1",
                        "storyId": "call-policy:overload",
                        "segmentId": format!("{SLA_SERVICE}:GetSla:overload"),
                        "kind": "direct_service_call",
                        "operation": "GetSla",
                        "outcome": event.as_str(),
                    }),
                )
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
                return Err(Status::resource_exhausted(event.as_str()));
            }
        };
        let payload = request.into_inner().payload;
        let scenario = String::from_utf8_lossy(&payload);
        if scenario == "m2-call-policy-failure" {
            let _ = permit.failure();
            append_json(
                &self.evidence_path,
                &json!({
                    "artifactVersion": "lenso.story-segment.v1",
                    "storyId": "call-policy:circuit",
                    "segmentId": format!("{SLA_SERVICE}:GetSla:failure"),
                    "kind": "direct_service_call",
                    "operation": "GetSla",
                    "outcome": "unavailable",
                    "workloadIdentity": self.workload_identity,
                }),
            )
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
            return Err(Status::unavailable("controlled M2 Call Policy failure"));
        }
        if scenario == "m2-call-policy-deadline" {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            let _ = permit.failure();
            return Err(Status::deadline_exceeded(
                "controlled M2 Service Deadline expired",
            ));
        }
        if scenario == "m2-call-policy-slow" {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let events = permit.success();
        append_json(
            &self.evidence_path,
            &json!({
                "artifactVersion": "lenso.story-segment.v1",
                "storyId": "call-policy:actual-call",
                "segmentId": format!("{SLA_SERVICE}:GetSla:{}", now_ms()),
                "kind": "direct_service_call",
                "operation": "GetSla",
                "outcome": "succeeded",
                "workloadIdentity": self.workload_identity,
                "callPolicyEvents": events.iter().map(|event| event.as_str()).collect::<Vec<_>>(),
            }),
        )
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
        Ok(Response::new(SlaResponse { payload }))
    }

    async fn update_sla(
        &self,
        request: Request<UpdateSlaRequest>,
    ) -> Result<Response<SlaResponse>, Status> {
        let deadline = metadata(&request, "x-lenso-deadline-unix-ms")?;
        let key = metadata(&request, "idempotency-key")?;
        let record = json!({
            "artifactVersion": "lenso.story-segment.v1",
            "storyId": format!("direct-call:{key}"),
            "segmentId": format!("{SLA_SERVICE}:UpdateSla:{key}"),
            "kind": "direct_service_call",
            "outcome": "succeeded",
            "operation": "UpdateSla",
            "deadlineUnixMs": deadline.parse::<u64>().unwrap_or_default(),
            "idempotencyKey": key,
            "workloadIdentity": self.workload_identity,
        });
        append_json(&self.evidence_path, &record)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        let payload: Value = serde_json::from_slice(&request.into_inner().payload)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        Ok(Response::new(SlaResponse {
            payload: serde_json::to_vec(&json!({"priority":"urgent","title":payload["title"]}))
                .map_err(|error| Status::internal(error.to_string()))?,
        }))
    }

    async fn probe_sla(
        &self,
        _request: Request<ProbeSlaRequest>,
    ) -> Result<Response<SlaResponse>, Status> {
        append_json(
            &self.evidence_path,
            &json!({
                "artifactVersion": "lenso.story-segment.v1",
                "storyId": "direct-call:unsafe-probe",
                "segmentId": format!("{SLA_SERVICE}:ProbeSla"),
                "kind": "direct_service_call",
                "operation": "ProbeSla",
                "workloadIdentity": self.workload_identity,
                "outcome": "unavailable",
            }),
        )
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
        Err(Status::unavailable("probe unavailable"))
    }
}

fn metadata<T>(request: &Request<T>, key: &'static str) -> Result<String, Status> {
    request
        .metadata()
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| Status::invalid_argument(format!("missing {key}")))
}

async fn sla_api(environment: WorkloadEvidence) -> anyhow::Result<()> {
    verify_store_owner(&environment).await?;
    let bindings = grpc_bindings()?;
    let get_sla_policy = bindings
        .operation("GetSla")
        .context("generated GetSla binding")?
        .call_policy
        .clone();
    let service = SlaApi {
        evidence_path: Path::new(&environment.store_path).join("operations.jsonl"),
        workload_identity: environment.workload_identity,
        call_policy_runtime: CallPolicyRuntime::default(),
        get_sla_policy,
    };
    let grpc_listener = tokio::net::TcpListener::bind("127.0.0.1:4211").await?;
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(grpc_listener);
    let grpc = tonic::transport::Server::builder()
        .add_service(SupportServiceServer::new(service))
        .serve_with_incoming(incoming);
    let health_listener = tokio::net::TcpListener::bind(SLA_HEALTH_ENDPOINT).await?;
    let health = axum::serve(
        health_listener,
        Router::new().route("/health/ready", get(|| async { StatusCode::OK })),
    );
    let (grpc_result, health_result) = tokio::join!(grpc, health);
    grpc_result?;
    health_result?;
    Ok(())
}

pub async fn run_smoke() -> anyhow::Result<SmokeEvidence> {
    let sandbox_state: Value = serde_json::from_slice(&tokio::fs::read(SANDBOX_STATE).await?)?;
    let workloads = sandbox_state["workloads"]
        .as_array()
        .context("Sandbox Workload state")?;
    ensure!(
        workloads.len() == 6,
        "Sandbox started an unexpected process"
    );
    ensure!(
        workloads.iter().all(|workload| matches!(
            workload["serviceId"].as_str(),
            Some(TICKET_SERVICE | SLA_SERVICE)
        )),
        "Host, Provider, Runtime Console, or another control-plane process entered the Data Plane"
    );
    let runtime_console_withheld = sandbox_state.get("runtimeConsole").is_none()
        && workloads.iter().all(|workload| {
            !workload["workloadId"]
                .as_str()
                .is_some_and(|id| id.contains("console"))
        });
    ensure!(
        runtime_console_withheld,
        "Runtime Console was not withheld from the acceptance proof"
    );
    let deadline = now_ms() + CONTRACT_DEADLINE_MS;
    let client = DirectHttpClient::new(
        LastValidEndpointResolver::new(SandboxEndpointResolver::new(fixture_path(SANDBOX_STATE))),
        http_bindings()?,
    );
    let service = ServiceReference::new(TICKET_SERVICE);
    let response = client
        .call(
            &service,
            update_ticket_call("42", "SLA breach", deadline, IDEMPOTENCY_KEY),
        )
        .await?;
    ensure!(response.status == StatusCode::OK, "ticket operation failed");
    let body: Value = serde_json::from_slice(&response.body)?;
    ensure!(body["ticketId"] == "42", "ticket identity changed");
    ensure!(body["sla"]["priority"] == "urgent", "SLA outcome missing");

    let state_path = fixture_path(SANDBOX_STATE);
    let withheld_path = state_path.with_extension("withheld");
    tokio::fs::rename(&state_path, &withheld_path).await?;
    let system_plane_withheld = !tokio::fs::try_exists(&state_path).await?;
    ensure!(
        system_plane_withheld,
        "System Plane endpoint source remained available"
    );
    let plane_independent_call = client
        .call(
            &service,
            update_ticket_call(
                "43",
                "Plane-independent SLA breach",
                now_ms() + CONTRACT_DEADLINE_MS,
                "ticket-43:update",
            ),
        )
        .await;
    tokio::fs::rename(&withheld_path, &state_path).await?;
    let plane_independent_call = plane_independent_call?;
    ensure!(
        plane_independent_call.status == StatusCode::OK,
        "direct call failed while System Plane state was withheld"
    );
    let plane_independent_body: Value = serde_json::from_slice(&plane_independent_call.body)?;
    ensure!(
        plane_independent_body["ticketId"] == "43"
            && plane_independent_body["sla"]["priority"] == "urgent",
        "plane-independent business outcome missing"
    );

    let probe = DirectGrpcClient::new(resolver(SLA_SERVICE, SLA_ENDPOINT)?, grpc_bindings()?)
        .probe_sla(&ServiceReference::new(SLA_SERVICE), vec![], deadline)
        .await
        .expect_err("unsafe probe must fail without retry");
    let DirectGrpcCallError::Status {
        evidence: unsafe_evidence,
        ..
    } = probe
    else {
        anyhow::bail!("unsafe probe did not preserve native gRPC status");
    };

    let root = fixture_path(".lenso/system-sandbox/support-platform/services");
    let ticket_store = root.join(TICKET_SERVICE).join("store");
    let sla_store = root.join(SLA_SERVICE).join("store");
    ensure!(ticket_store != sla_store, "Service Stores are shared");
    let ticket_migration: WorkloadEvidence = read_json(ticket_store.join("migration.json")).await?;
    let sla_migration: WorkloadEvidence = read_json(sla_store.join("migration.json")).await?;
    let ticket_operations =
        tokio::fs::read_to_string(ticket_store.join("operations.jsonl")).await?;
    let sla_operations = tokio::fs::read_to_string(sla_store.join("operations.jsonl")).await?;
    let ticket_segments = json_lines(&ticket_operations);
    let sla_segments = json_lines(&sla_operations);
    let successful_story_segments = ticket_segments
        .iter()
        .chain(&sla_segments)
        .filter(|segment| {
            segment["artifactVersion"] == "lenso.story-segment.v1"
                && segment["outcome"] == "succeeded"
        })
        .count() as u32;
    ensure!(
        successful_story_segments >= 4,
        "successful direct calls did not persist local Story Segments"
    );
    ensure!(
        ticket_operations.contains("updateTicket") && ticket_operations.contains(SLA_SERVICE),
        "ticket local evidence missing"
    );
    ensure!(
        sla_operations.contains("UpdateSla"),
        "SLA business evidence missing"
    );
    ensure!(
        sla_operations.contains("ProbeSla"),
        "no-unsafe-retry evidence missing"
    );
    ensure!(
        ticket_migration.store_id == Service::Ticket.store_id()
            && sla_migration.store_id == Service::Sla.store_id(),
        "declared Service Store identities were not applied"
    );
    let sla_api_identity = sla_segments
        .iter()
        .find(|value| value["operation"] == "UpdateSla")
        .and_then(|value| value["workloadIdentity"].as_str().map(str::to_owned))
        .context("SLA API Workload Identity evidence")?;

    Ok(SmokeEvidence {
        ticket_service_reference: TICKET_SERVICE.to_owned(),
        sla_service_reference: SLA_SERVICE.to_owned(),
        ticket_endpoint: TICKET_ENDPOINT.to_owned(),
        sla_endpoint: SLA_ENDPOINT.to_owned(),
        deadline_unix_ms: deadline,
        idempotency_key: IDEMPOTENCY_KEY.to_owned(),
        http_decision: response.evidence.context("HTTP evidence")?.decision,
        grpc_decision: body["grpcDecision"].as_str().unwrap_or_default().to_owned(),
        unsafe_retry_decision: unsafe_evidence.decision,
        unsafe_retry_attempts: unsafe_evidence.attempts,
        calls_before_plane_withheld: 1,
        calls_after_plane_withheld: 1,
        system_plane_withheld,
        runtime_console_withheld,
        successful_story_segments,
        ticket_workload_identity: body["workloadIdentity"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        sla_workload_identity: sla_api_identity,
        ticket_store_id: ticket_migration.store_id.clone(),
        sla_store_id: sla_migration.store_id.clone(),
        stores_isolated: ticket_migration.store_path != sla_migration.store_path,
    })
}

pub async fn observe_linked_extraction_behavior(
    source_pool: &sqlx::PgPool,
) -> anyhow::Result<ExtractionBehaviorObservation> {
    let response =
        DirectHttpClient::new(resolver(TICKET_SERVICE, TICKET_ENDPOINT)?, http_bindings()?)
            .call(
                &ServiceReference::new(TICKET_SERVICE),
                update_ticket_call(
                    "ticket-003",
                    "Extraction behavior proof",
                    now_ms() + CONTRACT_DEADLINE_MS,
                    "ticket-003:extraction-linked",
                ),
            )
            .await?;
    ensure!(response.status == StatusCode::OK);
    sqlx::query(
        "update support.tickets set title = 'Extraction behavior proof' where id = 'ticket-003'",
    )
    .execute(source_pool)
    .await?;
    extraction_observation("linked", source_pool, response).await
}

pub async fn pause_linked_extraction_mutations() -> anyhow::Result<()> {
    tokio::fs::write(linked_mutation_gate_path(), b"m4-quiescence").await?;
    Ok(())
}

pub async fn resume_linked_extraction_mutations() -> anyhow::Result<()> {
    match tokio::fs::remove_file(linked_mutation_gate_path()).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub async fn probe_linked_extraction_route() -> anyhow::Result<String> {
    let response = reqwest::get(format!("{TICKET_ENDPOINT}/health/ready")).await?;
    ensure!(response.status() == StatusCode::OK);
    Ok("linked-health-ready".to_owned())
}

pub async fn linked_extraction_drain_snapshot(
    source_pool: &sqlx::PgPool,
) -> anyhow::Result<ExtractionDrainSnapshot> {
    ensure!(
        tokio::fs::try_exists(linked_mutation_gate_path()).await?,
        "linked mutation gate is not paused"
    );
    sqlx::query(
        "create table if not exists support.extraction_pending_work (work_id text primary key, kind text not null)",
    )
    .execute(source_pool)
    .await?;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "select work_id, kind from support.extraction_pending_work order by work_id",
    )
    .fetch_all(source_pool)
    .await?;
    let count = |kind: &str| rows.iter().filter(|(_, value)| value == kind).count() as u64;
    Ok(ExtractionDrainSnapshot {
        in_flight_requests: count("in_flight_request"),
        outbox_messages: count("outbox_message"),
        inbox_messages: count("inbox_message"),
        scheduled_functions: count("scheduled_function"),
        timers: count("timer"),
        durable_workflows: count("durable_workflow"),
        unresolved: rows.into_iter().map(|(id, _)| id).collect(),
        timed_out: false,
    })
}

fn linked_mutation_gate_path() -> PathBuf {
    fixture_path(".lenso/system-sandbox/support-platform/services")
        .join(TICKET_SERVICE)
        .join("store")
        .join("mutations.paused")
}

fn fixture_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

pub struct AutonomousExtractionService {
    endpoint: String,
    pool: sqlx::PgPool,
    server: tokio::task::JoinHandle<Result<(), std::io::Error>>,
    fail_next_request: Arc<AtomicBool>,
}

impl AutonomousExtractionService {
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub async fn observe(&self) -> anyhow::Result<ExtractionBehaviorObservation> {
        let response = self
            .update_ticket(
                "ticket-003",
                "Extraction behavior proof",
                "ticket-003:extraction-autonomous",
            )
            .await?;
        extraction_observation("autonomous", &self.pool, response).await
    }

    pub async fn update_ticket(
        &self,
        ticket_id: &str,
        title: &str,
        idempotency_key: &str,
    ) -> anyhow::Result<DirectHttpResponse> {
        let response =
            DirectHttpClient::new(resolver(TICKET_SERVICE, &self.endpoint)?, http_bindings()?)
                .call(
                    &ServiceReference::new(TICKET_SERVICE),
                    update_ticket_call(
                        ticket_id,
                        title,
                        now_ms() + CONTRACT_DEADLINE_MS,
                        idempotency_key,
                    ),
                )
                .await?;
        ensure!(response.status == StatusCode::OK);
        Ok(response)
    }

    pub async fn update_ticket_through_committed_topology(
        &self,
        topology_pool: &sqlx::PgPool,
        ticket_id: &str,
        title: &str,
        idempotency_key: &str,
    ) -> anyhow::Result<DirectHttpResponse> {
        let topology: (String, String) = sqlx::query_as(
            "select authority_kind, owner_id from lenso_extraction.authority_states where state_id = 'system'",
        )
        .fetch_one(topology_pool)
        .await?;
        ensure!(
            topology == ("autonomous".to_owned(), TICKET_SERVICE.to_owned()),
            "committed System topology does not resolve the candidate Service"
        );
        self.update_ticket(ticket_id, title, idempotency_key).await
    }

    pub async fn probe_health(&self) -> anyhow::Result<()> {
        let response = reqwest::get(format!("{}/health/ready", self.endpoint)).await?;
        ensure!(response.status() == StatusCode::OK);
        Ok(())
    }

    pub async fn probe_store_complete(&self) -> anyhow::Result<()> {
        let ticket_count: i64 = sqlx::query_scalar("select count(*) from support.tickets")
            .fetch_one(&self.pool)
            .await?;
        ensure!(ticket_count == 3, "candidate Store is incomplete");
        Ok(())
    }

    pub fn inject_next_request_failure(&self) {
        self.fail_next_request.store(true, Ordering::SeqCst);
    }

    pub fn clear_injected_failure(&self) {
        self.fail_next_request.store(false, Ordering::SeqCst);
    }
}

impl Drop for AutonomousExtractionService {
    fn drop(&mut self) {
        self.server.abort();
    }
}

pub async fn start_autonomous_extraction_service(
    destination_pool: &sqlx::PgPool,
) -> anyhow::Result<AutonomousExtractionService> {
    let grpc = std::sync::Arc::new(DirectGrpcClient::new(
        resolver(SLA_SERVICE, SLA_ENDPOINT)?,
        grpc_bindings()?,
    ));
    let pool = destination_pool.clone();
    let fail_next_request = Arc::new(AtomicBool::new(false));
    let handler_failure = fail_next_request.clone();
    let binding = DirectHttpServerBinding::new_without_workload_identity(
        http_bindings()?,
        move |request| {
            let grpc = grpc.clone();
            let pool = pool.clone();
            let fail_next_request = handler_failure.clone();
            async move {
                if fail_next_request.load(Ordering::SeqCst) {
                    return DirectHttpResponse::json(
                        StatusCode::SERVICE_UNAVAILABLE,
                        json!({"error":"injected provisional candidate failure"}),
                    );
                }
                let deadline = request.deadline_unix_ms.unwrap_or_default();
                let key = request.idempotency_key.clone().unwrap_or_default();
                let title = serde_json::from_slice::<Value>(&request.body)
                    .ok()
                    .and_then(|value| value["title"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                let ticket_id = ticket_id(&request.path).to_owned();
                match grpc
                    .update_sla(
                        &ServiceReference::new(SLA_SERVICE),
                        request.body.to_vec(),
                        deadline,
                        &key,
                    )
                    .await
                {
                    Ok(response) => {
                        if let Err(error) =
                            sqlx::query("update support.tickets set title = $2 where id = $1")
                                .bind(&ticket_id)
                                .bind(&title)
                                .execute(&pool)
                                .await
                        {
                            return DirectHttpResponse::json(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                json!({"error":error.to_string()}),
                            );
                        }
                        let sla: Value = serde_json::from_slice(&response.payload)
                            .unwrap_or_else(|_| json!({"error":"invalid SLA response"}));
                        DirectHttpResponse::json(
                            StatusCode::OK,
                            json!({
                                "ticketId":ticket_id,
                                "sla":sla,
                                "slaServiceReference":SLA_SERVICE,
                                "slaEndpoint":SLA_ENDPOINT,
                                "grpcDecision":response.evidence.decision,
                                "workloadIdentity":"local-dev://support-platform/support-ticket-service/support-ticket-service-api"
                            }),
                        )
                    }
                    Err(error) => DirectHttpResponse::json(
                        StatusCode::BAD_GATEWAY,
                        json!({"error":error.to_string()}),
                    ),
                }
            }
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let health_pool = destination_pool.clone();
    let router = binding.router().route(
        "/health/ready",
        get(move || {
            let pool = health_pool.clone();
            async move {
                let ready = sqlx::query_scalar::<_, bool>(
                    "select to_regclass('support.tickets') is not null",
                )
                .fetch_one(&pool)
                .await;
                match ready {
                    Ok(true) => StatusCode::OK,
                    _ => StatusCode::SERVICE_UNAVAILABLE,
                }
            }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let service = AutonomousExtractionService {
        endpoint,
        pool: destination_pool.clone(),
        server,
        fail_next_request,
    };
    service.probe_health().await?;
    Ok(service)
}

async fn extraction_observation(
    implementation: &str,
    pool: &sqlx::PgPool,
    response: DirectHttpResponse,
) -> anyhow::Result<ExtractionBehaviorObservation> {
    let response: Value = serde_json::from_slice(&response.body)?;
    let durable_state: Value = sqlx::query_scalar(
        "select to_jsonb(ticket_row) from support.tickets ticket_row where id = 'ticket-003'",
    )
    .fetch_one(pool)
    .await?;
    Ok(ExtractionBehaviorObservation {
        implementation: implementation.to_owned(),
        module_id: "support-ticket".to_owned(),
        operation_id: "updateTicket".to_owned(),
        tenant_id: "tenant-acme".to_owned(),
        actor_id: "user-42".to_owned(),
        response,
        durable_state,
        event_effects: Vec::new(),
        workflow_outcomes: vec!["support-sla:urgent".to_owned()],
        story_evidence: vec!["support-ticket:updateTicket:succeeded".to_owned()],
    })
}

fn http_bindings() -> anyhow::Result<DirectHttpBindings> {
    let source: Value = serde_yaml::from_str(include_str!("../contracts/support-http.v1.yaml"))?;
    generate_direct_http_bindings("support-http", "v1", &source).map_err(anyhow::Error::msg)
}

fn grpc_bindings() -> anyhow::Result<DirectGrpcBindings> {
    generate_direct_grpc_bindings(
        "support-grpc",
        "v1",
        include_str!("../contracts/support-grpc.v1.proto"),
        LOCAL_GRPC_DESCRIPTOR,
    )
    .map_err(anyhow::Error::msg)
}

fn resolver(service: &str, endpoint: &str) -> anyhow::Result<StaticEndpointResolver> {
    StaticEndpointResolver::new([EndpointState::new(
        ServiceReference::new(service),
        vec![Endpoint::new(endpoint)],
    )])
    .map_err(anyhow::Error::msg)
}

fn update_ticket_call(
    ticket_id: &str,
    title: &str,
    deadline: u64,
    idempotency_key: &str,
) -> DirectHttpCall {
    DirectHttpCall::new("updateTicket")
        .with_path_parameter("ticket_id", ticket_id)
        .with_json(json!({"title": title}))
        .with_deadline(deadline)
        .with_idempotency_key(idempotency_key)
}

fn json_lines(source: &str) -> Vec<Value> {
    source
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

async fn append_json(path: &Path, value: &Value) -> anyhow::Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(serde_json::to_string(value)?.as_bytes())
        .await?;
    file.write_all(b"\n").await?;
    Ok(())
}

async fn write_json(path: PathBuf, value: &impl Serialize) -> anyhow::Result<()> {
    tokio::fs::write(path, serde_json::to_vec_pretty(value)?).await?;
    Ok(())
}

async fn verify_store_owner(environment: &WorkloadEvidence) -> anyhow::Result<()> {
    let owner: WorkloadEvidence =
        read_json(Path::new(&environment.store_path).join("migration.json")).await?;
    ensure!(
        owner.service_id == environment.service_id && owner.store_id == environment.store_id,
        "Service Store is owned by {}/{} instead of {}/{}",
        owner.service_id,
        owner.store_id,
        environment.service_id,
        environment.store_id
    );
    Ok(())
}

async fn read_json<T: for<'de> Deserialize<'de>>(path: PathBuf) -> anyhow::Result<T> {
    Ok(serde_json::from_slice(&tokio::fs::read(path).await?)?)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use lenso_service::{
        ContractSemanticKind, EndpointResolver, LastValidEndpointResolver,
        check_contract_artifact_value,
    };

    #[test]
    fn service_definitions_are_valid_autonomous_contracts() {
        for source in [
            include_str!("../services/support-ticket/lenso.service.json"),
            include_str!("../services/support-sla/lenso.service.json"),
        ] {
            let value: Value = serde_json::from_str(source).unwrap();
            let check = check_contract_artifact_value(&value).unwrap();
            assert_eq!(check.semantic_kind, ContractSemanticKind::AutonomousService);
            assert!(check.provider_semantics.is_none());
        }
    }

    #[test]
    fn system_definition_is_a_valid_mixed_topology_contract() {
        let value: Value = serde_json::from_str(include_str!("../lenso.system.json")).unwrap();
        let check = check_contract_artifact_value(&value).unwrap();
        assert_eq!(check.semantic_kind, ContractSemanticKind::MixedSystem);
    }

    #[test]
    fn service_client_retains_endpoint_state_after_system_plane_is_withheld() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "lenso-support-system-resolver-{}-{nonce}",
            std::process::id(),
        ));
        std::fs::create_dir_all(&root).unwrap();
        let state = root.join("state.json");
        std::fs::write(
            &state,
            serde_json::to_vec(&json!({
                "endpoints": [{
                    "serviceId": SLA_SERVICE,
                    "workloadId": "support-sla-service-api",
                    "endpoint": SLA_ENDPOINT
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let resolver = LastValidEndpointResolver::new(SandboxEndpointResolver::new(state.clone()));
        let service = ServiceReference::new(SLA_SERVICE);

        let available = resolver.resolve(&service).unwrap();
        std::fs::remove_file(&state).unwrap();
        let withheld = resolver.resolve(&service).unwrap();

        assert_eq!(available, withheld);
        assert_eq!(withheld.endpoints[0].address, SLA_ENDPOINT);
        std::fs::remove_dir_all(root).unwrap();
    }
}
