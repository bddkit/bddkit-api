# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/bddkit/bddkit-api/releases/tag/v0.1.0) - 2026-11-11

### Added

- sign with Hawk, run SRP and attach files as multipart parts
- extract from the response and print it, through the host's variables and output
- assert on the response, replaying the request while a polled assertion waits
- build and send a request, registering what is sensitive with the host
- per-file handle state and the plugin exports
- config checks, secrets and live probe for the api and srp groups

### Other

- pre-split release gate (bddkit-api#20)
- release workflow with a rebuild_tag input for hand-cut candidates ([#19](https://github.com/bddkit/bddkit-api/pull/19))
- port the host tests of bddkit/bddkit#225 and #226, drop the loopback plugin index ([#15](https://github.com/bddkit/bddkit-api/pull/15))
- test against bddkit v0.5.0-rc.3, which has no dev-unreserve feature ([#15](https://github.com/bddkit/bddkit-api/pull/15))
- test against bddkit v0.5.0-rc.2, the candidate plugin ABI 4 is frozen on
- doctor, resource add, init and telemetry on an api instance, over the real host
- polling and replay of a POST and of an absence, unique() prepared once, and a malformed SRP resource, over the real host, ported with their meaning
- evidence, long bodies, reports and failure classes over the real host, ported with their meaning
- masking by field name and form, over the real host, ported with their meaning
- README covers the steps, the config and the privacy notes that concern HTTP
- the API example suite, its mocks and how to run it with the plugin
- two feature files dispatch to the plugin at the same time, proven by a barrier in the stub
- the feature suite runs from its config with the lines that waited for the steps restored
- Hawk, SRP and uploads over the real host, ported with their meaning
- extracts and Print over the real host, ported with their meaning
- response assertions, replay and the failure dump over the real host
- masking and request tests over the real host, ported with their meaning
- harness, stub and the moved acceptance suite, config and handle tests
- crate, manifest, code health policy and CI
- Initial commit
