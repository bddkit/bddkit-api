# bddkit-api

A bddkit plugin that sends HTTP requests and asserts on the responses, with Hawk signing, SRP and multipart, as a resource group.

It serves two groups, `api` and `srp`:

```yaml
resources:
  api:
    main:
      base_url: https://api.example.com
      timeout_secs: 20
      default_headers:
        Authorization: Bearer ${API_TOKEN}
  srp:
    main:
      variant: hex-string
```

`bddkit resource fields --config <suite>` lists every key of the groups the suite declares, and `bddkit doctor --live` sends one GET at each `base_url`.

## Steps

`bddkit steps list api --config <suite>` and `bddkit steps list srp --config <suite>` print every step the two groups add; `--filter <text>` narrows the list and `-v` adds a line of description under each step. They cover building a request (headers, query, body, form, `I attach the file`), sending it, the response assertions (status, headers, JSON, XML and HTML bodies, cookies, with the eventual forms that poll), reading the response (`extract … from JSON`, `from cookies`, `Print response …`), signing the next request with Hawk, and the four SRP steps.

### Uploading a file

`I attach the file "<path>" to the request as "<name>"` adds a file part to the request being built; with the form-parameters step the request goes out as `multipart/form-data`, the parameters as its text parts. With no attachment, form parameters stay URL-encoded.

```gherkin
Given the request form parameters are:
  | name     | value                                   |
  | metadata | {"idDocType":"ID_CARD","country":"DEU"} |
And I attach the file "fixtures/id_front.png" to the request as "content"
When I request "/applicants/<<applicantId>>/documents" using HTTP POST
```

The path is relative to the feature file the step is written in, and `<<variables>>` work in the path and the part name. Attach more than once to send several files, under one part name or several. The part's content type comes from the file extension, `application/octet-stream` when unknown. The multipart `Content-Type` replaces any set by `default_headers` or a header step. A literal path that does not exist stops `bddkit run` before the first request (exit 2); a path holding a `<<variable>>` fails at the step. The file is read when the step runs, so a retried assertion replays the same content, and a failure dump lists each part's name, file name, type and size, never its bytes. Hawk signing of a multipart request is refused. The whole file is held in memory.

## Config

Keys of an `api` resource: `base_url` (required), `timeout_secs`, `default_headers`, `sensitive_headers`, `sensitive_headers_only` and `options` (`polling.timeout_secs` / `polling.interval_ms` for the eventual assertions sent to this API). A scenario reaches any API by name with `I use "<name>" api`; the first one, or `default_api`, is current at the start of every scenario. An `srp` resource takes `variant` (required), `prime`, `generator` and `hash`.

## Privacy

The plugin sends the real values and says which of them are sensitive; bddkit hides them everywhere it prints or writes about a run (see the Privacy section of the bddkit README for the filter, `privacy.*` and `--reveal`). What concerns HTTP:

- **Headers.** `authorization`, `cookie` and `set-cookie` are masked by default: the header keeps its name and shows `***`, and the usable part of the value — the credential after `Bearer `, each cookie value — is hidden wherever it appears afterwards. Add `sensitive_headers: [x-api-key]` to an API resource to mask more, or `sensitive_headers_only: [x-api-key]` to replace the defaults (`[]` masks no header at all). A `default_headers` value under a sensitive name is masked from startup.
- **Fields.** A JSON key, form field or query parameter is sensitive when `password`, `secret` or `token` is one of its words, split on `_`, `-` and lower-to-upper case: `access_token`, `accessToken` and `refresh-token` are, `tokenizer` is not. The value is hidden where it stands, in the request and in the response, and the text around it is left as it was. A string of at least `min_length` characters joins the set of secrets; a number is hidden where it stands and never joins. The response of every request is registered whatever its status, so a token in a login answer is masked in the failure dump of that very step.
- **Hawk and SRP.** The Hawk key and the SRP password are `secret` parameters; the SRP steps also register their private `<prefix>_a` and `<prefix>_sessionKey`.
- **Limits.** A secret in a body that is not JSON, or under a name no field word matches, stays visible until a later step saves it into a sensitive variable. A short value is hidden by position only. A multipart text part, the message of an assertion that quotes a body and `Print response body as "<path>"` hide a short value only by value, so only when it is long enough. A transport error leaves the URL's query and credentials out.

## Tests

The tests drive a real `bddkit` binary, `v0.5.0-rc.3` or newer:

```sh
BDDKIT_BIN=/path/to/bddkit cargo test
```

Without `BDDKIT_BIN` the end-to-end tests skip locally and fail under `CI`.
