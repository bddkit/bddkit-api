//! The handle model, through the exports the host calls: one handle serves
//! every instance of a group, a switch reaches it, and the scenario boundary
//! resets the selection.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

use std::ffi::{CStr, CString, c_char};

use bddkit_api::{
    bddkit_dispatch, bddkit_drop_instance, bddkit_free_string, bddkit_init_instance,
    bddkit_reset_scenario, bddkit_select_instance,
};
use serde_json::{Value, json};

fn take(ptr: *mut c_char) -> Value {
    assert!(!ptr.is_null());
    // SAFETY: the pointer came from an export of this library and is freed once, below.
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: as above; `ptr` has not been freed yet.
    unsafe { bddkit_free_string(ptr) };
    serde_json::from_str(&text).expect("JSON")
}

fn call(export: unsafe extern "C" fn(*const c_char) -> *mut c_char, request: &Value) -> Value {
    let request = CString::new(request.to_string()).expect("no NUL");
    // SAFETY: `request` outlives the call.
    take(unsafe { export(request.as_ptr()) })
}

fn init(group: &str, instances: &Value) -> Value {
    call(
        bddkit_init_instance,
        &json!({"group": group, "instances": instances, "concurrency": 1}),
    )
}

fn two_apis() -> Value {
    json!({
        "main": {"config": {"base_url": "http://127.0.0.1:1/", "default_headers": {"x-a": "1"}}, "options": {}},
        "other": {"config": {"base_url": "http://127.0.0.1:2/"}, "options": {}},
    })
}

fn dispatch(handle: u64, step: u32, instance: &str) -> Value {
    let request =
        CString::new(json!({"instance": instance, "args": []}).to_string()).expect("no NUL");
    take(bddkit_dispatch(handle, step, request.as_ptr()))
}

fn select(handle: u64, instance: &str) -> Value {
    let request =
        CString::new(json!({"group": "api", "instance": instance}).to_string()).expect("no NUL");
    take(bddkit_select_instance(handle, request.as_ptr()))
}

#[test]
fn two_instances_get_one_handle_and_a_switch_reaches_it() {
    let reply = init("api", &two_apis());
    assert_eq!(reply["ok"], true, "{reply}");
    let handle = reply["handle"].as_u64().expect("handle");
    // The first dispatch sets the starting point; a switch afterwards is a
    // notice to the same handle, not a second one.
    assert_eq!(dispatch(handle, 0, "main")["status"], "fatal");
    assert_eq!(select(handle, "other")["ok"], true);
    assert_eq!(select(handle, "nope")["ok"], false);
    assert_eq!(dispatch(handle, 1, "other")["status"], "fatal");
    assert_eq!(take(bddkit_reset_scenario(handle))["ok"], true);
    assert_eq!(take(bddkit_drop_instance(handle))["ok"], true);
    assert_eq!(
        take(bddkit_drop_instance(handle))["ok"],
        false,
        "dropped once"
    );
}

#[test]
fn every_init_gets_a_distinct_handle() {
    let a = init("api", &two_apis())["handle"].as_u64().expect("handle");
    let b = init("api", &two_apis())["handle"].as_u64().expect("handle");
    let srp = init(
        "srp",
        &json!({"main": {"config": {"variant": "hex-string"}, "options": {}}}),
    )["handle"]
        .as_u64()
        .expect("handle");
    assert!(a != b && b != srp && a != srp);
}

#[test]
fn a_refused_instance_is_named() {
    let reply = init(
        "api",
        &json!({"main": {"config": {"base_url": "http://x/"}, "options": {}},
                "audit": {"config": {"base_url": "x", "timeout_secs": 0}, "options": {}}}),
    );
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["instance"], "audit");
}

#[test]
fn an_unknown_handle_and_an_unknown_step_are_fatal_not_a_panic() {
    assert_eq!(dispatch(u64::MAX, 0, "main")["status"], "fatal");
    let handle = init("api", &two_apis())["handle"].as_u64().expect("handle");
    assert_eq!(dispatch(handle, 99, "main")["error"], "unknown step");
}
