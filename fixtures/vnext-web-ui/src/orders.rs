use std::rc::Rc;

use futures::future::LocalBoxFuture;
use lenso_auth_sdk::{ActorAssertionVerifier, ActorProjectionError, TypedActor};
use lenso_capability_secure_greeting::{
    ActorBoundSecureGreetingEndpoint, GreetError, GreetRequest, GreetResponse,
    SecureGreetingClient, SecureGreetingHandler, UnknownDomainError,
};
use lenso_capability_ui_contribution::{
    ContributionEndpoint, ContributionInvocationError, ContributionProvider, DescribeRequest,
    DescribeResponse, DescribeResponseAssetsItem, DescribeResponseRequirementsItem,
};
use lenso_kernel::{ActivateContext, InvocationContext, ModuleLifecycle, RuntimeFailure};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};
use serde::{Deserialize, Serialize};

use crate::{MetadataScenario, auth::fixed_clock};

pub const ORDERS_PACKAGE_ID: &str = "fixture.orders";
const WORKER_PACKAGE_ID: &str = "fixture.orders-worker";

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ContributionConfiguration {
    contribution_id: String,
    route: String,
}

pub fn contribution_configuration(scenario: MetadataScenario, copy: bool) -> String {
    let route = if scenario == MetadataScenario::MissingRoute {
        "orders"
    } else {
        "/orders"
    };
    serde_json::to_string(&ContributionConfiguration {
        contribution_id: if copy { "orders-copy" } else { "orders" }.to_owned(),
        route: route.to_owned(),
    })
    .expect("fixture contribution configuration serializes")
}

#[derive(Debug)]
struct UserActor(String);

impl TypedActor for UserActor {
    fn from_assertion(
        assertion: &lenso_auth_sdk::ActorAssertion,
    ) -> Result<Self, ActorProjectionError> {
        if assertion.actor_kind() != "user" {
            return Err(ActorProjectionError::UnexpectedActorKind {
                expected: "user".to_owned(),
                actual: assertion.actor_kind().to_owned(),
            });
        }
        Ok(Self(assertion.subject().to_owned()))
    }
}

#[derive(Debug)]
struct OrdersHandler;

impl SecureGreetingHandler<UserActor> for OrdersHandler {
    fn greet(
        &self,
        actor: UserActor,
        request: GreetRequest,
    ) -> LocalBoxFuture<'static, Result<GreetResponse, GreetError>> {
        Box::pin(async move {
            if request.name.trim().is_empty() {
                return Err(GreetError::EmptyName);
            }
            if actor.0 == "forbidden" {
                return Err(GreetError::NotAllowed);
            }
            if request.name == "Future" {
                return Err(GreetError::Unknown(UnknownDomainError {
                    code: "future_rule".to_owned(),
                    payload: Some(serde_json::json!({ "retry": false })),
                    extra: [("source".to_owned(), serde_json::json!("orders"))]
                        .into_iter()
                        .collect(),
                }));
            }
            Ok(GreetResponse {
                message: format!("Hello, {} ({})!", request.name, actor.0),
            })
        })
    }
}

#[derive(Clone, Debug)]
struct OrdersContribution {
    metadata: DescribeResponse,
}

impl OrdersContribution {
    fn from_configuration(configuration: &str) -> Result<Self, RuntimeFailure> {
        let configuration: ContributionConfiguration = serde_json::from_str(configuration)
            .map_err(|error| RuntimeFailure::InvalidResolvedPlan {
                detail: format!("orders UI configuration is invalid: {error}"),
            })?;
        Ok(Self {
            metadata: DescribeResponse {
                contribution_id: configuration.contribution_id,
                route: configuration.route,
                navigation_label: "Orders".to_owned(),
                body: "<main><h1>Orders</h1><form id=\"greet\"><input id=\"token\" value=\"good-token\"><input id=\"name\" value=\"Ada\"><button>Greet</button></form><output id=\"result\"></output></main>".to_owned(),
                assets: vec![
                    DescribeResponseAssetsItem {
                        path: "/assets/generated/secure-greeting.js".to_owned(),
                        content_type: "text/javascript; charset=utf-8".to_owned(),
                        content: include_str!(concat!(env!("OUT_DIR"), "/secure-greeting-client.js"))
                            .to_owned(),
                    },
                    DescribeResponseAssetsItem {
                        path: "/assets/orders.js".to_owned(),
                        content_type: "text/javascript; charset=utf-8".to_owned(),
                        content: r##"import { createSecureGreetingClient } from "/assets/generated/secure-greeting.js";
const client = createSecureGreetingClient();
document.querySelector("#greet").addEventListener("submit", async (event) => {
  event.preventDefault();
  const result = await client.greet(
    { name: document.querySelector("#name").value },
    document.querySelector("#token").value,
  );
  document.querySelector("#result").textContent = result.ok ? result.value.message : result.error.error;
});
"##
                        .to_owned(),
                    },
                ],
                requirements: vec![DescribeResponseRequirementsItem {
                    capability_id: lenso_capability_secure_greeting::CAPABILITY_ID.to_owned(),
                    descriptor_version: lenso_capability_secure_greeting::DESCRIPTOR_VERSION
                        .to_owned(),
                    operations: vec![lenso_capability_secure_greeting::GREET_OPERATION.to_owned()],
                }],
            },
        })
    }
}

impl ContributionProvider for OrdersContribution {
    fn describe(
        &self,
        _context: InvocationContext,
        _request: DescribeRequest,
    ) -> LocalBoxFuture<'static, Result<DescribeResponse, ContributionInvocationError>> {
        let metadata = self.metadata.clone();
        Box::pin(async move { Ok(metadata) })
    }
}

#[derive(Debug)]
struct UiLifecycle;

impl ModuleLifecycle for UiLifecycle {
    fn activate(&self, context: ActivateContext) -> lenso_kernel::ModuleFuture {
        let result = SecureGreetingClient::from_dependencies(context.dependencies()).map(|_| ());
        Box::pin(futures::future::ready(result))
    }
}

#[derive(Clone, Debug)]
pub struct OrdersModuleFactory {
    verifier: ActorAssertionVerifier,
}

impl OrdersModuleFactory {
    pub const fn new(verifier: ActorAssertionVerifier) -> Self {
        Self { verifier }
    }
}

impl NativeModuleFactory for OrdersModuleFactory {
    fn package_id(&self) -> &'static str {
        ORDERS_PACKAGE_ID
    }

    fn package_version(&self) -> &'static str {
        "0.1.0"
    }

    fn instantiate(
        &self,
        context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        match context.entrypoint() {
            "backend" => Ok(NativeModuleInstance::new(vec![Rc::new(
                ActorBoundSecureGreetingEndpoint::new(
                    OrdersHandler,
                    self.verifier.clone(),
                    fixed_clock(),
                ),
            )])),
            "ui" => Ok(NativeModuleInstance::with_lifecycle(
                vec![Rc::new(ContributionEndpoint::new(
                    OrdersContribution::from_configuration(context.configuration())?,
                ))],
                UiLifecycle,
            )),
            entrypoint => Err(RuntimeFailure::InvalidResolvedPlan {
                detail: format!("unknown {ORDERS_PACKAGE_ID} entrypoint `{entrypoint}`"),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WorkerModuleFactory;

impl NativeModuleFactory for WorkerModuleFactory {
    fn package_id(&self) -> &'static str {
        WORKER_PACKAGE_ID
    }

    fn package_version(&self) -> &'static str {
        "0.1.0"
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        Ok(NativeModuleInstance::default())
    }
}
