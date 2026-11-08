//! bddkit plugin serving the `api` and `srp` resource groups: HTTP requests,
//! response assertions and SRP. Written against docs/plugin-authoring.md; it
//! must never need the host's source.

mod assert;
pub mod config;
mod hawk;
pub mod header;
mod login;
mod markup;
mod read;
mod reply;
mod runtime;
mod send;
mod srp;
pub mod state;
mod steps;
mod upload;

use std::collections::{BTreeMap, HashMap};
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use config::Api;
use serde_json::Value;
use state::{Instance, State};
use steps::Step;

/// The plugin ABI this library is built for.
const ABI: u32 = 4;

/// Outside `guard`, and safe there only because it provably cannot panic:
/// `CString::new` returns a `Result` and the fallback is a C string literal.
/// Anything added here must keep that property.
fn out(s: String) -> *mut c_char {
    CString::new(s)
        .unwrap_or_else(|_| c"{\"ok\":false,\"error\":\"NUL in reply\"}".to_owned())
        .into_raw()
}

/// Inside `guard` at every call site.
fn input(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: `p` is not null. The host builds a request from a `CString` and
    // keeps it valid for the duration of the call, which encloses this one;
    // it is copied before returning.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// A panic must never unwind across the FFI boundary. Every export is
/// guarded, including the trivial ones, so "every export is guarded" is an
/// invariant a reader checks in one pass.
fn guard(on_panic: &str, body: impl FnOnce() -> String) -> *mut c_char {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(reply) => out(reply),
        Err(_) => out(on_panic.to_string()),
    }
}

const PANIC_ENVELOPE: &str = r#"{"ok":false,"error":"the plugin panicked"}"#;
const PANIC_DISPATCH: &str = r#"{"status":"fatal","error":"the plugin panicked"}"#;
const PANIC_DIAGNOSTICS: &str = r#"{"diagnostics":[]}"#;

pub fn manifest_json() -> String {
    serde_json::json!({
        "name": "api",
        "version": env!("CARGO_PKG_VERSION"),
        "groups": ["api", "srp"],
        // A handle carries the request being built and the last exchange, and
        // is reset at every scenario boundary.
        "concurrency": "per_worker",
        // The last response survives `I use "<name>" api`, so one handle
        // serves every instance of a group.
        "handle_scope": "group",
        "fields": config::fields_json(),
    })
    .to_string()
}

#[unsafe(no_mangle)]
pub extern "C" fn bddkit_abi_version() -> u32 {
    ABI
}

#[unsafe(no_mangle)]
pub extern "C" fn bddkit_manifest() -> *mut c_char {
    guard(PANIC_ENVELOPE, manifest_json)
}

#[unsafe(no_mangle)]
pub extern "C" fn bddkit_list_steps() -> *mut c_char {
    guard(PANIC_ENVELOPE, steps::list_json)
}

/// # Safety
/// `s` must be a pointer this library returned and has not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bddkit_free_string(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: by this function's contract `s` came from `out`'s
        // `CString::into_raw` in this library and has not been freed yet.
        drop(unsafe { CString::from_raw(s) });
    }
}

/// What `validate_config` and `probe_config` read of their request.
struct ConfigRequest {
    group: String,
    config: Value,
}

fn parse_config_request(request: &str) -> Result<ConfigRequest, String> {
    let mut value: Value = serde_json::from_str(request).map_err(|e| e.to_string())?;
    Ok(ConfigRequest {
        group: value["group"].as_str().unwrap_or_default().to_string(),
        config: value["config"].take(),
    })
}

/// A body checked, and the static line `doctor` shows for it.
fn check(group: &str, config: &Value) -> Result<String, String> {
    match group {
        "api" => Api::parse(config).map(|api| api.detail()),
        "srp" => config::check_srp(config),
        other => Err(format!("this plugin serves no group \"{other}\"")),
    }
}

/// Eager, at startup, for every declared instance, touching nothing.
#[unsafe(no_mangle)]
pub extern "C" fn bddkit_validate_config(request: *const c_char) -> *mut c_char {
    guard(PANIC_ENVELOPE, move || {
        match parse_config_request(&input(request)) {
            Ok(request) => reply::config(
                check(&request.group, &request.config),
                &config::secrets(&request.group, &request.config),
            ),
            Err(error) => reply::err(error),
        }
    })
}

/// `doctor --live`: one GET at the base URL. Any status counts as reachable,
/// and is reported, not judged: `doctor` checks that the API is there, not
/// that the application routes its own root. An SRP configuration has nothing
/// to reach.
#[unsafe(no_mangle)]
pub extern "C" fn bddkit_probe_config(request: *const c_char) -> *mut c_char {
    guard(PANIC_ENVELOPE, move || {
        let request = match parse_config_request(&input(request)) {
            Ok(request) => request,
            Err(error) => return reply::err(error),
        };
        let secrets = config::secrets(&request.group, &request.config);
        let result = match request.group.as_str() {
            "api" => Api::parse(&request.config).and_then(|api| runtime::block_on(probe(&api))),
            "srp" => Ok(String::new()),
            other => Err(format!("this plugin serves no group \"{other}\"")),
        };
        reply::config(result, &secrets)
    })
}

async fn probe(api: &Api) -> Result<String, String> {
    let status = reqwest::Client::builder()
        .timeout(Duration::from_secs(api.timeout_secs))
        .build()
        .map_err(|e| format!("failed to create HTTP client: {e}"))?
        .get(api.base_url.clone())
        .send()
        .await
        .map_err(|e| e.to_string())?
        .status()
        .as_u16();
    Ok(format!("GET {} → {status}", api.base_url_text))
}

/// A handle is an index into this table, never a pointer.
static HANDLES: LazyLock<Mutex<HashMap<u64, Arc<Mutex<State>>>>> = LazyLock::new(Mutex::default);
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// Nothing can panic under this lock (a lookup, an insert, a remove), and the
/// table holds no half-updated state if it ever did.
fn handles() -> MutexGuard<'static, HashMap<u64, Arc<Mutex<State>>>> {
    HANDLES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The state behind a handle, cloned out so the table is not locked while it
/// is used.
fn state_of(handle: u64) -> Option<Arc<Mutex<State>>> {
    handles().get(&handle).map(Arc::clone)
}

/// Held for a few field writes only, so a poisoned lock is still usable.
fn locked(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One handle for the whole group of one feature file.
fn init(request: &str) -> String {
    let value: Value = match serde_json::from_str(request) {
        Ok(v) => v,
        Err(e) => return reply::err(e.to_string()),
    };
    let group = value["group"].as_str().unwrap_or_default();
    let mut apis: BTreeMap<String, Instance> = BTreeMap::new();
    for (name, instance) in value["instances"].as_object().into_iter().flatten() {
        let api = match group {
            "api" => Api::parse(&instance["config"]).map(|api| Instance::Api(Arc::new(api))),
            "srp" => srp::SrpParams::from_config(&instance["config"])
                .map(|params| Instance::Srp(Arc::new(params))),
            other => Err(format!("this plugin serves no group \"{other}\"")),
        };
        match api {
            Ok(api) => {
                apis.insert(name.clone(), api);
            }
            Err(error) => return reply::err_in(name, error),
        }
    }
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    handles().insert(handle, Arc::new(Mutex::new(State::new(apis))));
    reply::handle(handle)
}

#[unsafe(no_mangle)]
pub extern "C" fn bddkit_init_instance(request: *const c_char) -> *mut c_char {
    guard(PANIC_ENVELOPE, move || init(&input(request)))
}

fn dispatch(handle: u64, step_index: u32, request: &str) -> String {
    let Some(step) = Step::from_index(step_index) else {
        return reply::fatal("unknown step");
    };
    let Some(state) = state_of(handle) else {
        return reply::fatal("unknown handle");
    };
    let request: Value = match serde_json::from_str(request) {
        Ok(v) => v,
        Err(e) => return reply::fatal(&e.to_string()),
    };
    if let Err(error) = locked(&state).begin(request["instance"].as_str().unwrap_or_default()) {
        return reply::fatal(&error);
    }
    let call = steps::Call::parse(&request);
    steps::run(step, &mut locked(&state), &call).json()
}

#[unsafe(no_mangle)]
pub extern "C" fn bddkit_dispatch(
    handle: u64,
    step_index: u32,
    request: *const c_char,
) -> *mut c_char {
    guard(PANIC_DISPATCH, move || {
        dispatch(handle, step_index, &input(request))
    })
}

/// `I use "<name>" api` on a file that already holds the group's handle.
#[unsafe(no_mangle)]
pub extern "C" fn bddkit_select_instance(handle: u64, request: *const c_char) -> *mut c_char {
    guard(PANIC_ENVELOPE, move || {
        let Some(state) = state_of(handle) else {
            return reply::err("unknown handle");
        };
        let request: Value = match serde_json::from_str(&input(request)) {
            Ok(v) => v,
            Err(e) => return reply::err(e.to_string()),
        };
        match locked(&state).switch_to(request["instance"].as_str().unwrap_or_default()) {
            Ok(()) => reply::ok(),
            Err(error) => reply::err(error),
        }
    })
}

/// The scenario boundary: the request being built and the selection go.
#[unsafe(no_mangle)]
pub extern "C" fn bddkit_reset_scenario(handle: u64) -> *mut c_char {
    guard(PANIC_ENVELOPE, move || match state_of(handle) {
        Some(state) => {
            locked(&state).reset();
            reply::ok()
        }
        None => reply::err("unknown handle"),
    })
}

/// What a failing step adds to its dump: the last exchange, once there is one.
#[unsafe(no_mangle)]
pub extern "C" fn bddkit_failure_context(handle: u64) -> *mut c_char {
    guard(PANIC_DIAGNOSTICS, move || {
        let diagnostics =
            state_of(handle).map_or_else(Vec::new, |state| locked(&state).failure_context());
        serde_json::json!({"diagnostics": diagnostics}).to_string()
    })
}

/// Before the first request, for every plugin step the suite holds, with
/// nothing interpolated. Only a literal attachment path can be refused here.
fn validate_step(step_index: u32, request: &str) -> String {
    let Some(step) = Step::from_index(step_index) else {
        return reply::err("unknown step");
    };
    let value: Value = match serde_json::from_str(request) {
        Ok(v) => v,
        Err(e) => return reply::err(e.to_string()),
    };
    match steps::validate(step, &value) {
        Ok(()) => reply::ok(),
        Err(error) => reply::err(error),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bddkit_validate_step(step_index: u32, request: *const c_char) -> *mut c_char {
    guard(PANIC_ENVELOPE, move || {
        validate_step(step_index, &input(request))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn bddkit_drop_instance(handle: u64) -> *mut c_char {
    guard(PANIC_ENVELOPE, move || match handles().remove(&handle) {
        Some(_) => reply::ok(),
        None => reply::err("unknown handle"),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn bddkit_plugin_json_matches_the_library_and_the_manifest() {
        let declared: serde_json::Value =
            serde_json::from_str(include_str!("../bddkit-plugin.json")).expect("valid JSON");
        assert_eq!(declared["abi"], crate::bddkit_abi_version());
        let manifest: serde_json::Value =
            serde_json::from_str(&crate::manifest_json()).expect("JSON");
        assert_eq!(declared["groups"], manifest["groups"]);
    }

    #[test]
    fn the_manifest_claims_api_and_srp_as_group_scoped_per_worker_handles() {
        let v: serde_json::Value = serde_json::from_str(&crate::manifest_json()).expect("JSON");
        assert_eq!(v["name"], "api");
        assert_eq!(v["concurrency"], "per_worker");
        assert_eq!(v["handle_scope"], "group");
        assert!(v.get("implicit_instance").is_none());
        assert!(v["fields"]["api"].is_array() && v["fields"]["srp"].is_array());
    }
}
