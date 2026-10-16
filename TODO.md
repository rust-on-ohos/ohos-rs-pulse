# TODO

Track follow-up work without expanding the current one-fixture MVP implicitly.
Completed implementation and experiment evidence belong in the project documentation.

## Next

- [ ] Review and run the bounded [libc upstream test experiment](docs/experiments/libc-upstream-tests.md).
  Start with the pinned ctest target; establish nonempty coverage and harness
  completion semantics before claiming a passing upstream runtime result.
  The Rust adapter/manual workflow are implemented; hosted validation is pending.
- [ ] Add scheduled upstream release discovery after the first recipe is verified.
  Resolve each selected release to an immutable commit and lockfile, preserve
  version-specific outcomes, and report unsupported harness changes explicitly.

- [ ] Verify Rust runtime repeatability with two more independent hosted runs.
  The first successful Rust run is [38699405669](https://github.com/rust-on-ohos/ohos-rs-pulse/actions/runs/38699405669).
  Keep every attempt and compare setup, transport and execution durations.
- [ ] Remove hardcoded fixture paths from the workflows and Rust CLI.
  Put fixture selection in a shared typed configuration so adding a reviewed
  fixture does not require editing workflow execution logic. Keep the default
  libc scenario and explicit scope in each report.
- [ ] Centralize SDK environment discovery and hdc path setup.
  The runtime workflow currently enters `fixtures/libc-file-io` to run
  `cargo ohos env`, then parses SDK metadata separately from the CLI. Establish
  one Rust-owned validation/discovery path that provisioning can invoke without
  hardcoding a particular fixture in the workflow.
- [ ] Update checkout/upload-artifact Actions to reviewed Node.js 24-compatible
  releases and pin their commit SHAs. Verify the workflows after the update.

## Evidence and reliability

- [ ] Close the SDK archive provenance verification gap.
  Define and enforce SDK archive digest/attestation acceptance before use;
  retain verification evidence. Installed version/API checks are not sufficient.
  Keep the current disclosure until this is implemented and verified.
- [ ] Provide a standalone JSON Schema for result protocol version 1 and test it
  against Rust-generated success and failure reports. Keep schema and semantic
  validation consistent.
- [ ] Review transport budgets using repeated hosted measurements.
  The first Rust run's guest path diagnostics took about 25.8 seconds against a
  30-second transport budget. Reduce unnecessary diagnostic work or adjust
  bounded budgets based on evidence; do not add retries that hide failures.
- [ ] Keep runtime image/Action settings in one source of truth.
  The profile records image and Action pins, but the workflow repeats some of
  them. Prevent configuration drift while retaining reviewed, fixed Action refs
  and image digest verification before extraction.
- [ ] Pin the emulator Action's transitive cache Action to a reviewed commit,
  or remove that dependency from the execution path.
- [ ] Ensure missing logs and artifact expiration are visible in retained reports.
  GitHub artifacts currently expire after 14 days.

## After the MVP

- [ ] Add scheduled validation after the manual workflow demonstrates repeatability.
- [ ] Establish durable JSON evidence archival, retention and recovery policies.
- [ ] Evaluate one minimal napi-ohos/ArkTS integration fixture with real loading
  and call assertions in an explicitly identified application environment.
- [ ] Add PostgreSQL ingestion and historical queries when accumulated evidence
  creates that need. Keep execution independent of the database.
- [ ] Expand libc scenarios, SDKs, targets or crates only for concrete requirements;
  preserve separate evidence for each tested combination.
