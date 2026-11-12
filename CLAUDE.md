# CLAUDE.md — bddkit-api

Agent orientation for this plugin repository.

`bddkit-api` is a plugin for `bddkit`, a Gherkin-based acceptance-testing tool in Rust: this plugin sends HTTP requests, asserts on and reads the responses, signs with Hawk, runs SRP-6a and uploads files. It is a `cdylib` loaded by the host at run time; resource groups: `api` and `srp`; plugin ABI 4 (`bddkit-plugin.json`). `README.md` is the tester-facing description.

## Where the contract lives

The authoring contract is `docs/plugin-authoring.md` in the `bddkit/bddkit` repository (https://github.com/bddkit/bddkit/blob/main/docs/plugin-authoring.md); the host's side of it is `src/plugin/CLAUDE.md` there. If your memory has no entry saying where a local checkout of the host is, do not guess a path: ask the operator for a folder or a URL and save the answer as a `reference` memory. A change to the ABI is made in the host first and then followed here; a gap found here is fixed in the ABI, not worked around in the plugin.

## Rules only the plugin can keep (host cannot check them)

- **Every `extern "C"` export runs inside `catch_unwind`, argument decoding included.** Unwinding across the FFI boundary is undefined behaviour.
- **Only JSON strings cross the boundary.** Never print to stdout: the host composes a file's output atomically.
- **The plugin sends real values and says what is sensitive; the host masks.** Everything a request carries is registered (`secrets`, `scan`) before it leaves, a signed `Authorization` header included, and the response registers its half on every status.
- **One handle serves every instance of a group** (`handle_scope: group`), so the last response survives `I use "<name>" api`. `begin_send` clears the last request and exchange before it builds anything, so a send that fails leaves nothing earlier to dump.
- **A replay rebuilds the request from the recipe.** The recipe owns the attached bytes (read at the step) and the Hawk credentials; the signature and the multipart form are made again on every send, never stored.
- **A failure class is set where the error is born.** `unclassified` is only the clock and the system entropy failing (Hawk, and the SRP salt and ephemeral).

## SRP

- **SRP `hex-string` pads the client proof before hashing it into the server proof.** (`M1`/`M2` below are the SRP-6a proof messages, nothing to do with any release numbering.) The variant zero-pads the client proof `M1` to the hash's digest width before it goes into the server proof `M2`; getting that padding wrong — e.g. stripping the proof's leading zero the way some browser SRP clients do — silently produces a different `M2` about 1 login in 16, whenever the client proof's leading nibble happens to be zero.
- **`<prefix>_a`, the client's private value, lives in the file's variables, not in the handle.** `I complete SRP login` declares `wants_vars` and reads it from there, so it keeps the variables' scoping. `<prefix>_a` and `<prefix>_sessionKey` go in `secrets` whatever the prefix is called.

## Layout

`src/lib.rs` the exports and guard · `config.rs` instance config · `state.rs` per-handle state · `steps.rs` step declarations and dispatch · `send.rs` building, signing and sending a request · `hawk.rs` Hawk · `upload.rs` attachments and the multipart form · `srp/` and `login.rs` SRP and its steps · `assert.rs`, `read.rs`, `markup.rs` response assertions and reading · `reply.rs` reply shapes · `tests/common/stub.rs` the reference service the end-to-end tests talk to.

## Dev workflow

- Edition 2024, `rust-version` 1.97 (CI pins the toolchain). Dependency versions are not bumped without being asked.
- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` must be clean; no file over 1,000 lines.
- The end-to-end tests drive a real `bddkit`: `BDDKIT_BIN=/path/to/bddkit cargo test`. A red feature file is a real bug: never weaken one to force green.
- `tests/pre_split_gate.rs` and CI's `pre-split gate` job are the release gate: the core's suites at the commit of the `pre-split` tag (checked out beside, never copied here) run unmodified on the host with the RELEASED plugin (`PLUGIN_TAG`). A failure is a bug in the plugin or the host, never a reason to touch those files. Locally: `PRE_SPLIT_DIR=<checkout> BDDKIT_BIN=<host> cargo test --test pre_split_gate -- --ignored`.
- Everything in the repository is English: code, comments, error messages, documentation. Conversational replies follow the operator's language.
- Markdown is never soft-wrapped: one paragraph or list item is one physical line.
