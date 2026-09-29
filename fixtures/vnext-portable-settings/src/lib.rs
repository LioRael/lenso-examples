pub use example_settings_contract as contract;
pub use example_settings_http as endpoint;
pub use example_settings_store as storage;

pub mod composition;
pub mod host;

#[cfg(not(target_arch = "wasm32"))]
pub use example_settings_store::postgres;
#[cfg(target_arch = "wasm32")]
mod workers;

pub fn failure(detail: impl std::fmt::Display) -> lenso_kernel::RuntimeFailure {
    lenso_kernel::RuntimeFailure::PluginFailure {
        detail: detail.to_string(),
    }
}
