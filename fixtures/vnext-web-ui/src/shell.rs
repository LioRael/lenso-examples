use std::{cell::RefCell, collections::BTreeMap, fmt::Write, rc::Rc};

use futures::future::LocalBoxFuture;
use lenso_capability_ui_contribution::{
    Contribution, DESCRIBE_OPERATION, DescribeRequest, DescribeResponse,
};
use lenso_capability_web_shell::{
    ReadAssetError, ReadAssetRequest, ReadAssetResponse, RenderRouteError, RenderRouteRequest,
    RenderRouteResponse, RenderRouteResponseNavigationItem, RenderRouteResponseRequirementsItem,
    ShellEndpoint, ShellProvider, ShellReadAssetInvocationError, ShellRenderRouteInvocationError,
};
use lenso_kernel::{ActivateContext, InvocationContext, ModuleLifecycle, RuntimeFailure};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};

const WEB_SHELL_PACKAGE_ID: &str = "lenso.web-shell";

#[derive(Clone, Debug)]
struct Route {
    contribution_id: String,
    path: String,
    navigation_label: String,
    body: String,
    asset_paths: Vec<String>,
    requirements: Vec<RenderRouteResponseRequirementsItem>,
}

#[derive(Clone, Debug)]
struct Asset {
    contribution_id: String,
    content_type: String,
    content: String,
}

#[derive(Debug, Default)]
struct ShellState {
    routes: BTreeMap<String, Route>,
    assets: BTreeMap<String, Asset>,
}

impl ShellState {
    fn register_contribution(&mut self, metadata: DescribeResponse) -> Result<(), RuntimeFailure> {
        validate_metadata(&metadata)?;
        if let Some(existing) = self.routes.get(&metadata.route) {
            return Err(RuntimeFailure::InvalidResolvedPlan {
                detail: format!(
                    "UI Contribution route collision at `{}` between `{}` and `{}`",
                    metadata.route, existing.contribution_id, metadata.contribution_id
                ),
            });
        }
        let asset_paths = metadata
            .assets
            .iter()
            .map(|asset| asset.path.clone())
            .collect();
        for asset in &metadata.assets {
            if let Some(existing) = self.assets.get(&asset.path) {
                return Err(RuntimeFailure::InvalidResolvedPlan {
                    detail: format!(
                        "UI Contribution asset collision at `{}` between `{}` and `{}`",
                        asset.path, existing.contribution_id, metadata.contribution_id
                    ),
                });
            }
            self.assets.insert(
                asset.path.clone(),
                Asset {
                    contribution_id: metadata.contribution_id.clone(),
                    content_type: asset.content_type.clone(),
                    content: asset.content.clone(),
                },
            );
        }
        let requirements = metadata
            .requirements
            .iter()
            .map(|requirement| RenderRouteResponseRequirementsItem {
                capability_id: requirement.capability_id.clone(),
                descriptor_version: requirement.descriptor_version.clone(),
                operations: requirement.operations.clone(),
            })
            .collect();
        self.routes.insert(
            metadata.route.clone(),
            Route {
                contribution_id: metadata.contribution_id,
                path: metadata.route,
                navigation_label: metadata.navigation_label,
                body: metadata.body,
                asset_paths,
                requirements,
            },
        );
        Ok(())
    }
}

fn validate_metadata(metadata: &DescribeResponse) -> Result<(), RuntimeFailure> {
    let invalid = |detail: String| RuntimeFailure::InvalidResolvedPlan { detail };
    if metadata.contribution_id.trim().is_empty() {
        return Err(invalid("UI Contribution id must not be empty".to_owned()));
    }
    if !metadata.route.starts_with('/') {
        return Err(invalid(format!(
            "UI Contribution `{}` route must start with `/`",
            metadata.contribution_id
        )));
    }
    if metadata.route == "/" || metadata.route.starts_with("/assets/") {
        return Err(invalid(format!(
            "UI Contribution `{}` route is reserved",
            metadata.contribution_id
        )));
    }
    if metadata.navigation_label.trim().is_empty() || metadata.body.trim().is_empty() {
        return Err(invalid(format!(
            "UI Contribution `{}` navigation and body must not be empty",
            metadata.contribution_id
        )));
    }
    for asset in &metadata.assets {
        if !asset.path.starts_with("/assets/")
            || asset.content_type.trim().is_empty()
            || asset.content_type.contains(['\r', '\n'])
            || asset.content.is_empty()
        {
            return Err(invalid(format!(
                "UI Contribution `{}` has invalid asset metadata",
                metadata.contribution_id
            )));
        }
    }
    for requirement in &metadata.requirements {
        if requirement.capability_id.trim().is_empty()
            || requirement.descriptor_version.trim().is_empty()
            || requirement.operations.is_empty()
            || requirement
                .operations
                .iter()
                .any(|operation| operation.trim().is_empty())
        {
            return Err(invalid(format!(
                "UI Contribution `{}` has an invalid portable requirement",
                metadata.contribution_id
            )));
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct WebShell {
    state: Rc<RefCell<ShellState>>,
}

impl ShellProvider for WebShell {
    fn read_asset(
        &self,
        _context: InvocationContext,
        request: ReadAssetRequest,
    ) -> LocalBoxFuture<'static, Result<ReadAssetResponse, ShellReadAssetInvocationError>> {
        let result = self
            .state
            .borrow()
            .assets
            .get(&request.path)
            .map(|asset| ReadAssetResponse {
                content_type: asset.content_type.clone(),
                content: asset.content.clone(),
            })
            .ok_or(ShellReadAssetInvocationError::Domain(
                ReadAssetError::AssetNotFound,
            ));
        Box::pin(futures::future::ready(result))
    }

    fn render_route(
        &self,
        _context: InvocationContext,
        request: RenderRouteRequest,
    ) -> LocalBoxFuture<'static, Result<RenderRouteResponse, ShellRenderRouteInvocationError>> {
        let state = self.state.borrow();
        let Some(route) = state.routes.get(&request.path) else {
            return Box::pin(futures::future::ready(Err(
                ShellRenderRouteInvocationError::Domain(RenderRouteError::RouteNotFound),
            )));
        };
        let navigation = state
            .routes
            .values()
            .map(|route| RenderRouteResponseNavigationItem {
                route: route.path.clone(),
                label: route.navigation_label.clone(),
            })
            .collect::<Vec<_>>();
        let mut nav_html = String::new();
        for item in &navigation {
            write!(nav_html, "<a href=\"{}\">{}</a>", item.route, item.label)
                .expect("String writes cannot fail");
        }
        let mut scripts = String::new();
        for path in route.asset_paths.iter().filter(|path| {
            state
                .assets
                .get(*path)
                .is_some_and(|asset| asset.content_type.starts_with("text/javascript"))
        }) {
            write!(scripts, "<script type=\"module\" src=\"{path}\"></script>")
                .expect("String writes cannot fail");
        }
        let response = RenderRouteResponse {
            contribution_id: route.contribution_id.clone(),
            body: format!(
                "<!doctype html><html><body><nav>{nav_html}</nav>{}{scripts}</body></html>",
                route.body
            ),
            navigation,
            asset_paths: route.asset_paths.clone(),
            requirements: route.requirements.clone(),
        };
        Box::pin(futures::future::ready(Ok(response)))
    }
}

#[derive(Debug)]
struct ShellLifecycle {
    state: Rc<RefCell<ShellState>>,
}

impl ModuleLifecycle for ShellLifecycle {
    fn activate(&self, context: ActivateContext) -> lenso_kernel::ModuleFuture {
        let handles = match context.dependencies().many::<Contribution>() {
            Ok(handles) => handles,
            Err(error) => return Box::pin(futures::future::ready(Err(error))),
        };
        let state = self.state.clone();
        Box::pin(async move {
            for handle in handles {
                let metadata = handle
                    .invoke(DESCRIBE_OPERATION, DescribeRequest {})
                    .await?
                    .map_err(|error| RuntimeFailure::ModuleFailure {
                        detail: format!("UI Contribution metadata failed: {error:?}"),
                    })?;
                state.borrow_mut().register_contribution(metadata)?;
            }
            Ok(())
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WebShellFactory;

impl NativeModuleFactory for WebShellFactory {
    fn package_id(&self) -> &'static str {
        WEB_SHELL_PACKAGE_ID
    }

    fn package_version(&self) -> &'static str {
        "0.1.0"
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        let state = Rc::new(RefCell::new(ShellState::default()));
        Ok(NativeModuleInstance::with_lifecycle(
            vec![Rc::new(ShellEndpoint::new(WebShell {
                state: state.clone(),
            }))],
            ShellLifecycle { state },
        ))
    }
}
