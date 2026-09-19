use lenso_contract_authoring as lenso;

#[derive(lenso::JsonSchema)]
#[schemars(deny_unknown_fields)]
struct UppercaseRequest {
    text: String,
}

#[derive(lenso::JsonSchema)]
#[schemars(deny_unknown_fields)]
struct UppercaseResponse {
    text: String,
}

#[derive(lenso::DomainError)]
enum TextError {
    Unavailable,
}

#[lenso::capability(
    id = "example.text",
    major = 1,
    version = "1.0.0",
    portable = true,
    cross_lane_transfer = false
)]
trait Text {
    async fn uppercase(
        &self,
        context: lenso::Ctx<'_>,
        request: UppercaseRequest,
    ) -> Result<UppercaseResponse, TextError>;
}
