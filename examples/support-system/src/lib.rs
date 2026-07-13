use anyhow::{Context, ensure};
use axum::{Router, routing::get};
use http::StatusCode;
use lenso_service::{
    DirectGrpcBindings, DirectGrpcCallError, DirectGrpcClient, DirectHttpBindings, DirectHttpCall,
    DirectHttpClient, DirectHttpResponse, DirectHttpServerBinding, Endpoint,
    EndpointResolutionError, EndpointResolver, EndpointState, LastValidEndpointResolver,
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
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncWriteExt;
use tonic::{Request, Response, Status};

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
        (_, WorkloadRole::Worker) => worker(&environment).await,
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
    let server = DirectHttpServerBinding::new(http_bindings()?, move |_| {
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

async fn worker(environment: &WorkloadEvidence) -> anyhow::Result<()> {
    verify_store_owner(environment).await?;
    write_json(
        Path::new(&environment.store_path).join("worker.json"),
        environment,
    )
    .await?;
    std::future::pending::<()>().await;
    Ok(())
}

async fn ticket_api(environment: WorkloadEvidence) -> anyhow::Result<()> {
    verify_store_owner(&environment).await?;
    let evidence_path = Path::new(&environment.store_path).join("operations.jsonl");
    let workload_identity = environment.workload_identity.clone();
    let grpc = std::sync::Arc::new(DirectGrpcClient::new(
        LastValidEndpointResolver::new(SandboxEndpointResolver::new(PathBuf::from(SANDBOX_STATE))),
        grpc_bindings()?,
    ));
    let binding = DirectHttpServerBinding::new(http_bindings()?, move |request| {
        let evidence_path = evidence_path.clone();
        let workload_identity = workload_identity.clone();
        let grpc = grpc.clone();
        async move {
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
}

#[tonic::async_trait]
impl SupportService for SlaApi {
    async fn get_sla(
        &self,
        request: Request<GetSlaRequest>,
    ) -> Result<Response<SlaResponse>, Status> {
        Ok(Response::new(SlaResponse {
            payload: request.into_inner().payload,
        }))
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
    let service = SlaApi {
        evidence_path: Path::new(&environment.store_path).join("operations.jsonl"),
        workload_identity: environment.workload_identity,
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
        LastValidEndpointResolver::new(SandboxEndpointResolver::new(PathBuf::from(SANDBOX_STATE))),
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

    let state_path = PathBuf::from(SANDBOX_STATE);
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

    let root = Path::new(".lenso/system-sandbox/support-platform/services");
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
