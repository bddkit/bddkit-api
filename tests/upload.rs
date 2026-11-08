//! `I attach the file …` seen through the real `bddkit`: the upload on the
//! wire, the check before the first request, and where a relative path is
//! read from. Ported from the core's acceptance suite with their meaning
//! unchanged, each test with its own stub.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::routing::post;
use axum::{Json, Router};
use common::{api_project, bddkit, combined, project, run, scenario};
use serde_json::json;

/// Answers what an upload endpoint would care about: was it multipart, and did
/// the file and the text field arrive. `ready` turns true on the second call,
/// so a polled assertion has to replay the upload; `calls` counts the requests.
async fn upload_stub() -> (String, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let app = Router::new().route(
        "/upload",
        post(
            |headers: axum::http::HeaderMap, body: axum::body::Bytes| async move {
                let content_type = headers
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                let text = String::from_utf8_lossy(&body);
                let call = counted.fetch_add(1, Ordering::SeqCst) + 1;
                Json(json!({
                    "multipart": content_type.starts_with("multipart/form-data; boundary="),
                    "has_file": text.contains("filename=\"id_front.png\"") && text.contains("PNGDATA"),
                    "has_field": text.contains("name=\"kind\"") && text.contains("ID_CARD"),
                    "ready": call >= 2,
                }))
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    (format!("http://{address}/"), calls)
}

const UPLOAD: &str = r#"Feature: upload
  Scenario: a file and a field go out as multipart
    Given the request form parameters are:
      | name | value   |
      | kind | ID_CARD |
    And I attach the file "fixtures/id_front.png" to the request as "content"
    When I request "/upload" using HTTP POST
    Then the response body contains JSON:
      """
      {"multipart": true, "has_file": true, "has_field": true}
      """
"#;

fn fixture(dir: &Path) {
    std::fs::create_dir_all(dir.join("fixtures")).expect("mkdir");
    std::fs::write(dir.join("fixtures/id_front.png"), b"PNGDATA").expect("fixture");
}

/// The directory the project's feature file sits in.
fn features(cfg: &Path) -> PathBuf {
    cfg.with_file_name("features")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_is_uploaded_as_multipart_with_its_form_fields() {
    let bin = require_bddkit!();
    let (base, _) = upload_stub().await;
    let cfg = api_project("upload-ok", &base, UPLOAD, "", "");
    fixture(&features(&cfg));
    let (code, stdout, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}{stderr}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_polled_upload_is_replayed_with_the_same_file() {
    let bin = require_bddkit!();
    let (base, calls) = upload_stub().await;
    let feature = r#"Feature: upload
  Scenario: the second answer is ready
    Given I attach the file "fixtures/id_front.png" to the request as "content"
    When I request "/upload" using HTTP POST
    And I expect the next assertion to pass within 2 seconds, checking every 25 milliseconds
    Then the response body contains JSON:
      """
      {"ready": true, "has_file": true}
      """
"#;
    let cfg = api_project("upload-polled", &base, feature, "", "");
    fixture(&features(&cfg));
    let (code, stdout, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "one send, one replay");
}

#[test]
fn a_missing_literal_attachment_stops_the_run_before_any_request_with_exit_two() {
    let bin = require_bddkit!();
    let cfg = api_project("upload-missing", common::CLOSED, UPLOAD, "", "");
    let (code, _, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("fixtures/id_front.png"), "{stderr}");
    assert!(stderr.contains("no such file"), "{stderr}");
}

#[test]
fn a_directory_is_not_a_file_to_attach() {
    let bin = require_bddkit!();
    let cfg = api_project("upload-dir", common::CLOSED, UPLOAD, "", "");
    std::fs::create_dir_all(features(&cfg).join("fixtures/id_front.png")).expect("mkdir");
    let (code, _, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("no such file"), "{stderr}");
}

#[test]
fn doctor_reports_a_missing_literal_attachment() {
    let bin = require_bddkit!();
    let cfg = api_project("upload-doctor", common::CLOSED, UPLOAD, "", "");
    let out = bddkit(&bin, &["doctor", "--config", cfg.to_str().expect("UTF-8")]);
    assert_eq!(out.status.code(), Some(1), "{}", combined(&out));
    assert!(
        combined(&out).contains("no such file"),
        "{}",
        combined(&out)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_attachment_behind_a_variable_fails_the_step_not_the_startup() {
    let bin = require_bddkit!();
    let (base, _) = upload_stub().await;
    let feature = scenario(
        "    Given set variable \"file\" to \"fixtures/gone.png\"\n    And I attach the file \"<<file>>\" to the request as \"content\"\n",
    );
    let cfg = api_project("upload-var-missing", &base, &feature, "", "");
    let (code, stdout, _) = run(&bin, &cfg, &[]);
    assert_eq!(code, 1, "{stdout}");
    assert!(stdout.contains("fixtures/gone.png"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_relative_path_is_read_from_the_directory_of_the_file_the_step_is_written_in() {
    let bin = require_bddkit!();
    let (base, _) = upload_stub().await;
    let cfg = api_project("upload-nested", &base, UPLOAD, "", "");
    // Neither next to the config nor where the run starts: only beside the
    // feature file, which sits one level down.
    let features = features(&cfg);
    std::fs::remove_file(features.join("only.feature")).expect("rm");
    std::fs::create_dir_all(features.join("deep")).expect("mkdir");
    std::fs::write(features.join("deep/nested.feature"), UPLOAD).expect("feature");
    fixture(&features.join("deep"));
    let (code, stdout, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}{stderr}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_include_reads_its_relative_path_from_its_own_directory() {
    let bin = require_bddkit!();
    let (base, _) = upload_stub().await;
    let caller = "Feature: caller\n  Scenario: the included file uploads\n    Given I include \"inc/upload.feature\"\n";
    let cfg = api_project("upload-include", &base, caller, "", "");
    let inc = features(&cfg).join("inc");
    std::fs::create_dir_all(&inc).expect("mkdir");
    std::fs::write(inc.join("upload.feature"), UPLOAD).expect("feature");
    fixture(&inc);
    let (code, stdout, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}{stderr}");
}

#[test]
fn a_missing_attachment_inside_an_include_stops_the_run_with_exit_two() {
    let bin = require_bddkit!();
    let caller = "Feature: caller\n  Scenario: the included file uploads\n    Given I include \"inc/upload.feature\"\n";
    let cfg = api_project("upload-include-missing", common::CLOSED, caller, "", "");
    let inc = features(&cfg).join("inc");
    std::fs::create_dir_all(&inc).expect("mkdir");
    std::fs::write(inc.join("upload.feature"), UPLOAD).expect("feature");
    // The file is next to the caller, not next to the include.
    fixture(&features(&cfg));
    let (code, _, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("inc/fixtures/id_front.png"), "{stderr}");
}

const MACRO: &str = r#"- step: 'I upload the front of the card'
  do:
    - I attach the file "fixtures/id_front.png" to the request as "content"
    - I request "/upload" using HTTP POST
"#;

const MACRO_CALL: &str = r#"Feature: macro
  Scenario: the macro uploads
    When I upload the front of the card
    Then the response body contains JSON:
      """
      {"multipart": true, "has_file": true}
      """
"#;

fn macro_project(name: &str, base: &str) -> PathBuf {
    let resources = format!("  api:\n    stub:\n      base_url: {base}\n");
    let cfg = project(name, MACRO_CALL, &resources, "");
    let dir = cfg.with_file_name("macros");
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("card.yaml"), MACRO).expect("macro");
    let mut text = std::fs::read_to_string(&cfg).expect("config");
    text.push_str(&format!(
        "macro_paths: [{}]\n",
        dir.display().to_string().replace('\\', "/")
    ));
    std::fs::write(&cfg, text).expect("config");
    cfg
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_macro_body_reads_its_relative_path_from_the_macro_file() {
    let bin = require_bddkit!();
    let (base, _) = upload_stub().await;
    let cfg = macro_project("upload-macro", &base);
    fixture(&cfg.with_file_name("macros"));
    let (code, stdout, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}{stderr}");
}

#[test]
fn a_macro_body_attaching_a_missing_literal_file_is_refused_at_startup() {
    let bin = require_bddkit!();
    let cfg = macro_project("upload-macro-missing", common::CLOSED);
    // Beside the feature file, not beside the macro.
    fixture(&features(&cfg));
    let (code, _, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("macros/fixtures/id_front.png"), "{stderr}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_absolute_path_is_taken_as_is() {
    let bin = require_bddkit!();
    let (base, _) = upload_stub().await;
    let elsewhere = std::env::temp_dir().join(format!("bddkit-api-abs-{}", std::process::id()));
    fixture(&elsewhere);
    let absolute = elsewhere
        .join("fixtures/id_front.png")
        .display()
        .to_string()
        .replace('\\', "/");
    let feature = UPLOAD.replace("fixtures/id_front.png", &absolute);
    let cfg = api_project("upload-absolute", &base, &feature, "", "");
    let (code, stdout, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}{stderr}");
}
