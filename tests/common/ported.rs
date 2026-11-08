//! Helpers of the tests ported from the host's suite: `doctor` on a project,
//! an include pair, `bddkit init` in a directory, a telemetry suite.

use super::{CLOSED, Output, api_project, bddkit, command, lock_dir};
use std::path::{Path, PathBuf};
use std::process::Stdio;

pub fn path(p: &Path) -> &str {
    p.to_str().expect("UTF-8 path")
}

/// `bddkit doctor --config <config> <args>`.
pub fn doctor(bin: &Path, config: &Path, args: &[&str]) -> Output {
    let mut all = vec!["doctor", "--config", path(config)];
    all.extend_from_slice(args);
    bddkit(bin, &all)
}

/// A project whose only API is `stub` at a closed port, with `feature` as its
/// one file.
pub fn closed_project(name: &str, feature: &str, top_extra: &str) -> PathBuf {
    api_project(name, CLOSED, feature, "", top_extra)
}

/// A caller and a helper side by side; returns `doctor`'s exit code and output.
pub fn doctor_include_pair(
    bin: &Path,
    name: &str,
    caller: &str,
    helper: &str,
) -> (Option<i32>, String) {
    let config = closed_project(name, caller, "");
    let features = config.parent().expect("parent").join("features");
    std::fs::write(features.join("helper.feature"), helper).expect("write helper");
    let out = doctor(bin, &config, &[]);
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// A fresh empty directory.
pub fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bddkit-api-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

/// `bddkit <args>` run in `dir`: no terminal, `dir` as home, and the local
/// plugin index.
pub fn in_dir(bin: &Path, dir: &Path, args: &[&str]) -> Output {
    command(bin, args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("HOME", dir)
        .env("BDDKIT_PLUGIN_REGISTRY", registry_url())
        .env_remove("CI")
        .output()
        .expect("bddkit")
}

/// A telemetry suite: a project in `dir`, a state directory whose lock names
/// the plugin (`entry` is appended to its entry), a log for the payloads.
pub struct Suite {
    pub dir: PathBuf,
}

impl Suite {
    pub fn new(name: &str, config: &str, entry: &str) -> Suite {
        let dir = temp(&format!("telemetry-{name}"));
        std::fs::create_dir_all(dir.join("features")).expect("mkdir");
        std::fs::create_dir_all(dir.join("state")).expect("mkdir");
        std::fs::write(
            dir.join("features/a.feature"),
            "Feature: plain\n  Scenario: s\n    Given set variable \"x\" to \"1\"\n",
        )
        .expect("feature");
        std::fs::write(dir.join("bddkit.yaml"), config).expect("config");
        let lock = std::fs::read_to_string(lock_dir().join("plugins.yaml")).expect("lock");
        std::fs::write(dir.join("state/plugins.yaml"), format!("{lock}{entry}")).expect("lock");
        Suite { dir }
    }

    pub fn bddkit(&self, bin: &Path, args: &[&str]) -> Output {
        let mut command = command(bin, args);
        for name in [
            "DO_NOT_TRACK",
            "CI",
            "GITHUB_ACTIONS",
            "GITLAB_CI",
            "JENKINS_URL",
            "BUILDKITE",
            "CIRCLECI",
            "TF_BUILD",
            "TEAMCITY_VERSION",
            "BITBUCKET_BUILD_NUMBER",
        ] {
            command.env_remove(name);
        }
        command
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .env("HOME", &self.dir)
            .env("BDDKIT_DIR", self.dir.join("state"))
            .env("BDDKIT_TELEMETRY_LOG", self.dir.join("telemetry.log"))
            .output()
            .expect("bddkit")
    }

    /// The only payload recorded: one per run.
    pub fn only_payload(&self) -> serde_json::Value {
        let log = std::fs::read_to_string(self.dir.join("telemetry.log")).unwrap_or_default();
        let mut all: Vec<serde_json::Value> = log
            .lines()
            .map(|line| serde_json::from_str(line).expect("a payload is one JSON object"))
            .collect();
        assert_eq!(all.len(), 1, "one payload per run: {all:?}");
        all.remove(0)
    }
}

/// The URL of a one-entry plugin index naming this plugin's groups, served
/// from a thread of this process. `init` asks the index which plugins a config
/// needs, and an installed plugin has to be found in it to count as satisfied.
pub fn registry_url() -> &'static str {
    use std::io::{Read, Write};
    static URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    URL.get_or_init(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}/registry.yaml", listener.local_addr().expect("addr"));
        let body = "plugin:\n  - name: api\n    repo: bddkit/bddkit-api\n    groups: [api, srp]\n    description: HTTP API and SRP\n    abi:\n      \"4\": 0.1.0\n";
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        url
    })
}
