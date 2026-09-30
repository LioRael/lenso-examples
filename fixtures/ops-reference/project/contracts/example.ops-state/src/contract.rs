use lenso_contract_authoring as lenso;

#[derive(lenso::JsonSchema)]
#[schemars(deny_unknown_fields)]
struct ReadRequest {}

#[derive(lenso::JsonSchema)]
#[schemars(deny_unknown_fields)]
struct ReadResponse {
    label: String,
    value: i64,
    revision: i64,
}

#[derive(lenso::JsonSchema)]
#[schemars(deny_unknown_fields)]
struct UpdateRequest {
    value: i64,
    expected_revision: i64,
    idempotency_key: String,
}

#[derive(lenso::JsonSchema)]
#[schemars(deny_unknown_fields)]
struct UpdateResponse {
    label: String,
    value: i64,
    revision: i64,
    receipt_id: String,
}

#[derive(lenso::JsonSchema)]
#[schemars(deny_unknown_fields)]
struct ReceiptRequest {
    idempotency_key: String,
}

#[derive(lenso::JsonSchema)]
#[schemars(deny_unknown_fields)]
struct ReceiptResponse {
    found: bool,
    label: Option<String>,
    value: Option<i64>,
    revision: Option<i64>,
    receipt_id: Option<String>,
}

#[derive(lenso::DomainError)]
enum StateError {
    InvalidInput,
    StaleRevision,
    IdempotencyConflict,
    Unavailable,
    UnknownCommit,
}

#[lenso::capability(
    id = "example.ops-state",
    major = 1,
    version = "1.1.1",
    portable = true,
    cross_lane_transfer = false,
    request_queue_capacity = 16,
    request_max_concurrency = 2
)]
trait OpsState {
    async fn read(
        &self,
        context: lenso::Ctx<'_>,
        request: ReadRequest,
    ) -> Result<ReadResponse, StateError>;
    async fn update(
        &self,
        context: lenso::Ctx<'_>,
        request: UpdateRequest,
    ) -> Result<UpdateResponse, StateError>;
    async fn receipt(
        &self,
        context: lenso::Ctx<'_>,
        request: ReceiptRequest,
    ) -> Result<ReceiptResponse, StateError>;
}
