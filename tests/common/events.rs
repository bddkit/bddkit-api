//! Helpers for the tests that read the event stream and the evidence on disk.

use super::command;
use serde_json::Value;
use std::path::Path;

/// `bddkit run --config <config> <args>` with extra environment variables:
/// exit code, stdout, stderr. (The suite's own `BDDKIT_*` variables are
/// already removed by `command`.)
pub fn run_env(
    bin: &Path,
    config: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> (i32, String, String) {
    let config = config.to_str().expect("UTF-8 path");
    let mut all = vec!["run", "--config", config];
    all.extend_from_slice(args);
    let out = command(bin, &all)
        .envs(envs.iter().copied())
        .output()
        .expect("bddkit");
    (
        out.status.code().expect("exit code"),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

pub fn read_stream(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .expect("the stream exists")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line:?}")))
        .collect()
}

pub fn of_type<'a>(stream: &'a [Value], kind: &str) -> Vec<&'a Value> {
    stream.iter().filter(|e| e["type"] == kind).collect()
}

/// `bddkit run --config <cfg> --events <dir>/events.ndjson <extra…>`: the exit
/// code and the stream it wrote.
pub fn run_with_events(bin: &Path, cfg: &Path, extra: &[&str]) -> (i32, Vec<Value>) {
    let events = cfg.with_file_name("events.ndjson");
    let mut args = vec!["--events", events.to_str().expect("UTF-8 path")];
    args.extend_from_slice(extra);
    let (code, _, _) = run_env(bin, cfg, &args, &[]);
    (code, read_stream(&events))
}
