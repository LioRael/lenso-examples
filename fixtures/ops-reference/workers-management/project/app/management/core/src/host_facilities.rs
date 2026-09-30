use lenso_management_authority::{Clock, Qualification};
use lenso_management_core::storage::Journal;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct AuthorityHandle {
    pub journal: Rc<dyn Journal>,
    pub qualification: Rc<dyn Qualification>,
    pub clock: Rc<dyn Clock>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
struct EventClock(lenso_native_adapter::NativeHostClock);

#[cfg(target_arch = "wasm32")]
impl Clock for EventClock {
    fn monotonic(&self) -> std::time::Duration {
        self.0.now()
    }
    fn wall(&self) -> time::OffsetDateTime {
        let milliseconds = js_sys::Date::now();
        // Date.now is an absolute wall clock; the Driver only supplies event deadlines.
        time::OffsetDateTime::from_unix_timestamp_nanos((milliseconds as i128) * 1_000_000)
            .expect("Worker wall timestamp is in the supported calendar range")
    }
}

#[cfg(target_arch = "wasm32")]
pub fn authority(
    binding: &wasm_bindgen::JsValue,
    clock: &lenso_native_adapter::NativeHostClock,
) -> Result<AuthorityHandle, lenso::RuntimeFailure> {
    let journal = Rc::new(
        lenso_management_core::workers::WorkersJournal::from_binding(binding)
            .map_err(|_| lenso::RuntimeFailure::InvalidResolvedPlan {
                detail: "Management requires its selected finite D1 journal facility".into(),
            })?,
    );
    Ok(AuthorityHandle {
        journal: journal.clone(),
        qualification: journal,
        clock: Rc::new(EventClock(clock.clone())),
    })
}
