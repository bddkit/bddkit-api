//! What every end-to-end test binary shares: the host, the plugin built for
//! it, and the lock directory that ties the two. `BDDKIT_BIN` names a `bddkit`
//! (`v0.5.0` or newer). Without it the tests skip, except
//! under `CI`, where a missing host is a failure: a skipped suite must never
//! look green there.

#![allow(
    dead_code,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "each test binary uses a part of this module; `allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

pub mod barrier;
pub mod events;
pub mod ported;
pub mod stub;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

pub fn bddkit_bin() -> Option<PathBuf> {
    let bin = PathBuf::from(std::env::var_os("BDDKIT_BIN")?);
    // A path that is set but wrong must not look like a green suite.
    assert!(bin.is_file(), "BDDKIT_BIN is not a file: {}", bin.display());
    Some(bin)
}

/// Returns from the test, with a note, when there is no host to run it on.
#[macro_export]
macro_rules! require_bddkit {
    () => {
        match $crate::common::bddkit_bin() {
            Some(bin) => bin,
            None if std::env::var_os("CI").is_some() => {
                panic!("BDDKIT_BIN does not name a bddkit binary, and CI must not skip")
            }
            None => {
                eprintln!("SKIP: set BDDKIT_BIN to a bddkit v0.5.0 or newer");
                return;
            }
        }
    };
}

pub fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Builds the plugin once (or takes `BDDKIT_API_LIBRARY`) and writes a lock file for it into a directory
/// handed to `--bddkit-dir`.
pub fn lock_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        // The release gate runs the library a release archive carries, not a build.
        let library = if let Some(library) = std::env::var_os("BDDKIT_API_LIBRARY") {
            PathBuf::from(library)
        } else {
            let out = Command::new(env!("CARGO"))
                .arg("build")
                .current_dir(root())
                .output()
                .expect("cargo build");
            assert!(
                out.status.success(),
                "plugin build failed:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            root().join("target/debug").join(format!(
                "{}bddkit_api{}",
                std::env::consts::DLL_PREFIX,
                std::env::consts::DLL_SUFFIX
            ))
        };
        let dir = std::env::temp_dir().join(format!("bddkit-api-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(
            dir.join("plugins.yaml"),
            format!("plugin:\n  - name: api\n    path: {}\n", library.display()),
        )
        .expect("lock");
        dir
    })
}

/// `bddkit <args>` with `BDDKIT_DIR` on the lock dir (every subcommand reads
/// it, `resource` has no flag), not yet run.
pub fn command(bin: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(bin);
    let empty = lock_dir().join("empty");
    std::fs::create_dir_all(&empty).expect("mkdir");
    // An empty home, so a developer's installed plugins and settings cannot
    // interfere with the suite under test.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("BDDKIT_") {
            command.env_remove(key);
        }
    }
    command
        .args(args)
        .env("BDDKIT_DIR", lock_dir())
        .env("XDG_CONFIG_HOME", &empty)
        .env("XDG_DATA_HOME", &empty);
    command
}

pub fn bddkit(bin: &Path, args: &[&str]) -> Output {
    command(bin, args).output().expect("bddkit")
}

/// Standard output and standard error as one string.
pub fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A suite on disk: one feature file and a config whose `resources` section is
/// `resources`, indented two spaces by the caller. Returns the config path.
pub fn project(name: &str, feature: &str, resources: &str, extra: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bddkit-api-{name}-{}", std::process::id()));
    // Left over from an earlier run with the same pid, a stale file would count.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("features")).expect("mkdir");
    std::fs::write(dir.join("features/only.feature"), feature).expect("feature");
    let config = dir.join("cfg.yaml");
    std::fs::write(
        &config,
        format!(
            "paths: [{}]\nresources:\n{resources}{extra}",
            dir.join("features")
                .display()
                .to_string()
                .replace('\\', "/")
        ),
    )
    .expect("config");
    config
}

pub const PING: &str = "Feature: only\n  Scenario: one\n    When I request \"/ping\"\n    Then the response code is 200\n";

/// A TCP port nothing listens on.
pub const CLOSED: &str = "http://127.0.0.1:1/";

/// A one-file project whose only API is `stub`, at `base`. `api_extra` is
/// appended to the resource (indented to sit inside it) and `top_extra` to the
/// config. Returns the config path.
pub fn api_project(
    name: &str,
    base: &str,
    feature: &str,
    api_extra: &str,
    top_extra: &str,
) -> PathBuf {
    let resources = format!("  api:\n    stub:\n      base_url: {base}\n{api_extra}");
    project(name, feature, &resources, top_extra)
}

/// `bddkit run --config <config> <args>`: exit code, stdout, stderr.
pub fn run(bin: &Path, config: &Path, args: &[&str]) -> (i32, String, String) {
    let config = config.to_str().expect("UTF-8 path");
    let mut all = vec!["run", "--config", config];
    all.extend_from_slice(args);
    let out = bddkit(bin, &all);
    (
        out.status.code().expect("exit code"),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The run with `--junit`: exit code, stdout and the report's text.
pub fn run_with_junit(bin: &Path, config: &Path) -> (i32, String, String) {
    let junit = config.with_file_name("junit.xml");
    let (code, stdout, _) = run(
        bin,
        config,
        &["--junit", junit.to_str().expect("UTF-8 path")],
    );
    (
        code,
        stdout,
        std::fs::read_to_string(&junit).expect("junit report"),
    )
}

/// The run with `--events … --evidence`: the evidence events it wrote.
pub fn evidence(bin: &Path, config: &Path) -> Vec<serde_json::Value> {
    let events = config.with_file_name("events.ndjson");
    run(
        bin,
        config,
        &[
            "--events",
            events.to_str().expect("UTF-8 path"),
            "--evidence",
        ],
    );
    std::fs::read_to_string(&events)
        .expect("event stream")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("JSON line"))
        .filter(|event| event["type"] == "evidence")
        .collect()
}

/// A feature of one scenario made of `steps`.
pub fn scenario(steps: &str) -> String {
    format!("Feature: f\n  Scenario: s\n{steps}")
}
