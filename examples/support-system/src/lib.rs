use anyhow::{Context, ensure};
use axum::{Router, routing::get};
use http::StatusCode;
use lenso_service::{
    DirectGrpcBindings, DirectGrpcCallError, DirectGrpcClient, DirectHttpBindings, DirectHttpCall,
    DirectHttpClient, DirectHttpResponse, DirectHttpServerBinding, Endpoint, EndpointState,
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
    let binding = DirectHttpServerBinding::new(http_bindings()?, move |request| {
        let evidence_path = evidence_path.clone();
        let workload_identity = workload_identity.clone();
        async move {
            let deadline = request.deadline_unix_ms.unwrap_or_default();
            let key = request.idempotency_key.clone().unwrap_or_default();
            let payload = request.body.to_vec();
            let grpc = match resolver(SLA_SERVICE, SLA_ENDPOINT)
                .and_then(|resolver| Ok(DirectGrpcClient::new(resolver, grpc_bindings()?)))
            {
                Ok(client) => client,
                Err(error) => {
                    return DirectHttpResponse::json(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        json!({"error": error.to_string()}),
                    );
                }
            };
            match grpc
                .update_sla(&ServiceReference::new(SLA_SERVICE), payload, deadline, &key)
                .await
            {
                Ok(response) => {
                    let record = json!({
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
    let deadline = now_ms() + CONTRACT_DEADLINE_MS;
    let response =
        DirectHttpClient::new(resolver(TICKET_SERVICE, TICKET_ENDPOINT)?, http_bindings()?)
            .call(
                &ServiceReference::new(TICKET_SERVICE),
                DirectHttpCall::new("updateTicket")
                    .with_path_parameter("ticket_id", "42")
                    .with_json(json!({"title":"SLA breach"}))
                    .with_deadline(deadline)
                    .with_idempotency_key(IDEMPOTENCY_KEY),
            )
            .await?;
    ensure!(response.status == StatusCode::OK, "ticket operation failed");
    let body: Value = serde_json::from_slice(&response.body)?;
    ensure!(body["ticketId"] == "42", "ticket identity changed");
    ensure!(body["sla"]["priority"] == "urgent", "SLA outcome missing");

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
    let sla_api_identity = sla_operations
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
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
    use lenso_service::{ContractSemanticKind, check_contract_artifact_value};

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
}
