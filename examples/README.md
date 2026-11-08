# Running the examples

One suite, `examples/api.yaml`, run by a `bddkit` that has this plugin installed. Build the plugin with `cargo build`, then point the host at it with a lock directory:

```bash
mkdir -p /tmp/lock
printf 'plugin:\n  - name: api\n    path: %s/target/debug/libbddkit_api.so\n' "$PWD" > /tmp/lock/plugins.yaml
```

and pass `--bddkit-dir /tmp/lock` to `bddkit run`. Once the plugin is released, `bddkit plugin install api` does the same.

## The suite

Talks to a local [Smocker](https://github.com/smocker-dev/smocker) instance, so the suite runs offline and its responses are fixed by a file in this repo.

```bash
docker run -d --name smocker -p 8080:8080 -p 8081:8081 -e SMOCKER_INIT_MOCKS=/mocks/api-server.yaml -v "$PWD/examples/mocks:/mocks:ro" ghcr.io/smocker-dev/smocker:v1.1.2-preview
bddkit run --config examples/api.yaml --bddkit-dir /tmp/lock
```

The mocks live in `examples/mocks/api-server.yaml` and are seeded into Smocker at startup — after editing them, `docker restart smocker` reloads. Smocker's web UI (mocks, request history) is on <http://localhost:8081>; the mocked API itself is on <http://localhost:8080>.

| File | What it demonstrates |
|---|---|
| `features/methods.feature` | GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS, `Location`/`Allow` headers, a 404 asserted like any other answer; the scenarios are grouped under `Rule:` blocks, and the write rule's `Background:` sets the JSON `Content-Type` once; the POST is marked with `I am collecting evidence` / `I am not collecting evidence`, so `--events run.ndjson --evidence=marked` keeps that exchange and no other |
| `features/json_matchers.feature` | `@variableType`, `@regExp`, `@arrayLength`, paths into arrays, `contains` vs `equals`, grouped under `Rule:` blocks |
| `features/variables.feature` | `set variable`, extraction from JSON and cookies, reuse across steps and scenarios, `<<unique()>>`, a `<<variable>>` in a typed position (an HTTP method, a status code), `Show all variables` and `Print table:` |
| `features/variable_assertions.feature` | substring, regex and JSON assertions over a variable, extraction from a variable by JSON path or regex, and a macro in `macros/users.yaml` that publishes its extracts with `global` |
| `features/macros.feature` | YAML-declared steps from `macros/posts.yaml`, macro calling a macro, Scenario Outline, a Cucumber Expression template with an alternation and optional text, a typed parameter (`{code:uint}`) given a literal or a `<<variable>>` |
| `features/content_types.feature` | HTML/XML/plain-text responses, a form login, multipart file uploads — a file plus a text field with a `<<variable>>` in the path, and two files under one part name (`fixtures/`) |
| `features/response_headers.feature` | `contains` / `matches` and their negations over a response header: a repeated `Link` line with double quotes inside the expected text, and a `Vary` list |
| `features/eventual.feature` | An assertion polled until it passes; a number written with or without quotes (`I sleep 0 seconds` / `I sleep "0" seconds`) |
| `features/include_setup.feature` + `features/include_caller.feature` | `I include "<file>" scenario "<name>"`, a Scenario Outline included with a `with:` table, `@exports` isolation — an unexported variable set inside the included scenario does not exist in the caller — and `with prefix "<p>"`, including the same flow twice without the second call's exports overwriting the first's. Neither file makes an HTTP request, so this pair also runs with no Smocker container: `bddkit run --config examples/api.yaml --bddkit-dir /tmp/lock examples/features/include_setup.feature examples/features/include_caller.feature` |

`eventual.feature` is only interesting on a fresh mock session: `/jobs/1` answers "pending" twice and then "done", and Smocker keeps that counter until the container restarts. Run `docker restart smocker` to see the polling actually retry — on a session where the job is already done, the assertion just passes on its first attempt.

## Narrowing a run

```bash
# one file (a positional path overrides the config's `paths`)
bddkit run --config examples/api.yaml --bddkit-dir /tmp/lock examples/features/methods.feature

# one tag (repeatable; feature-level tags apply to every scenario in the file)
bddkit run --config examples/api.yaml --bddkit-dir /tmp/lock --tag json
```

Exit codes: `0` everything passed, `1` a scenario failed, `2` the run never started (bad config, unknown step, no scenario matched the filter).
