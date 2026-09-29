use std::{rc::Rc, time::Duration};

use futures::future::LocalBoxFuture;
use js_sys::{Function, Promise};
use lenso_kernel::{CancellationToken, Kernel};
use lenso_web_ingress_plugin::WebIngressEventFactory;
use lenso_workers_driver::WorkersDriver;
use serde::{Serialize, de::DeserializeOwned};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::{
    composition,
    contract::*,
    failure,
    host::{Input, Output},
    storage::*,
};

/// The Host closure fixes the URL and attaches authorization. Neither the
/// endpoint nor this Wasm module receives database or bridge credentials.
#[derive(Clone, Debug)]
struct Bridge(Function);

impl Bridge {
    fn call<T: Serialize + 'static, R: DeserializeOwned + 'static>(
        &self,
        operation: &str,
        input: T,
    ) -> LocalBoxFuture<'static, Result<R, lenso_kernel::RuntimeFailure>> {
        let function = self.0.clone();
        let operation = operation.to_owned();
        Box::pin(async move {
            let input = serde_json::to_string(&input).map_err(failure)?;
            let promise = function
                .call2(&JsValue::NULL, &operation.into(), &input.into())
                .map_err(|_| failure("settings persistence transport failed"))?;
            let value = JsFuture::from(Promise::resolve(&promise))
                .await
                .map_err(|_| failure("settings persistence unavailable"))?;
            let text = value
                .as_string()
                .filter(|text| text.len() <= 4096)
                .ok_or_else(|| failure("invalid persistence response"))?;
            serde_json::from_str(&text).map_err(failure)
        })
    }
}

impl Persistence for Bridge {
    fn read(&self, request: ReadRequest) -> LocalBoxFuture<'static, ReadResult> {
        self.call("read", request)
    }
    fn change(&self, request: ChangeRequest) -> LocalBoxFuture<'static, ChangeResult> {
        self.call("change", request)
    }
}

struct EventGuard(WorkersDriver);

impl Drop for EventGuard {
    fn drop(&mut self) {
        self.0.request_shutdown();
    }
}

#[wasm_bindgen]
pub async fn settings_event(input: String, persistence: Function) -> Result<String, JsValue> {
    async fn run(
        input: String,
        persistence: Function,
    ) -> Result<String, lenso_kernel::RuntimeFailure> {
        if input.len() > 8192 {
            return Err(failure("fixture event exceeds bound"));
        }
        let input: Input = serde_json::from_str(&input).map_err(failure)?;
        let request = input.request()?;
        let driver = WorkersDriver::new();
        let _event = EventGuard(driver.clone());
        let ingress = WebIngressEventFactory::new();
        let app = Kernel::start_native(
            composition::plan()?,
            driver.clone(),
            composition::registry(Rc::new(Bridge(persistence)), ingress.clone()),
        )
        .await?;
        let result = ingress.handle(request, CancellationToken::new()).await;
        let shutdown = app.shutdown(Duration::from_secs(1)).await;
        driver.request_shutdown();
        if !matches!(shutdown, lenso_kernel::ShutdownOutcome::Clean) {
            return Err(failure("fixture shutdown was not clean"));
        }
        let result = Output::from_response(result?)?;
        serde_json::to_string(&result).map_err(failure)
    }
    run(input, persistence)
        .await
        .map_err(|error| JsValue::from_str(&format!("{error:?}")))
}
