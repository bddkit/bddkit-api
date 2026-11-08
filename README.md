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

`bddkit resource fields` lists every key of both groups, and `bddkit doctor --live` sends one GET at each `base_url`.

## Status

Under development. This build declares the resource groups, their configuration checks and the per-file handle; the steps are ported one issue at a time.

## Tests

The tests drive a real `bddkit` binary built with `--features dev-unreserve`:

```sh
BDDKIT_BIN=/path/to/bddkit cargo test
```

Without `BDDKIT_BIN` the end-to-end tests skip locally and fail under `CI`.
