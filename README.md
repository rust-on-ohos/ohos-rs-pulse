# ohos-rs-pulse

Follow-up improvements and expansion gates are tracked in [TODO.md](TODO.md).

A Rust CLI and reviewed fixtures for reproducible OpenHarmony compatibility evidence.
The current scope is one upstream libc file-round-trip scenario on
`x86_64-unknown-linux-ohos`. A result applies to its recorded environment and
scenario; it does not certify a whole crate or all OH devices.

## Build and test

The pinned Rust toolchain is selected by `rust-toolchain.toml`. The CLI and fixture
are independent Cargo packages with committed lockfiles.

```sh
cargo build --locked --bin ohos-pulse
cargo test --locked --all-targets
cargo test --locked --manifest-path fixtures/libc-file-io/Cargo.toml
```

The CLI currently runs on Linux and macOS. Host tests do not establish OH runtime
compatibility. Rust integration tests use local command doubles to exercise build,
transport and reporting failure boundaries without installing an SDK or emulator.

## Run

Install Rust's OHOS target, cargo-ohos and the SDK selected by
`config/feasibility.json` before running. SDK downloads and emulator lifecycle
belong to Actions or the developer; the CLI checks installed tool and SDK pins.
Invoke commands from the repository root or pass `--root /path/to/checkout`.
Use a new output directory for every attempt.

```sh
target/debug/ohos-pulse run --output results/build-001
```

For a provisioned guest reachable through hdc, build and execute in one run:

```sh
target/debug/ohos-pulse run --runtime --output results/runtime-001
```

Or execute a previous Rust CLI build after starting the guest:

```sh
target/debug/ohos-pulse runtime --build results/build-001 --output results/runtime-002
```

The latter command copies original build evidence under `build-evidence/`, assigns
a fresh runtime ID, verifies the artifact digest and keeps all evidence paths
relative to the new output directory. It accepts CLI `run.json`, not the retired
Python feasibility formats. Historical experiment artifacts remain historical
records; they are not silently converted.

Regenerate a summary without executing any evidence commands:

```sh
target/debug/ohos-pulse report --input results/runtime-001/run.json --output results/runtime-001/summary.md
```

Exit codes: 0 for all requested stages passing, 1 for failed/blocked/timed-out
requested stages, 2 for invalid input, infrastructure or reporting errors. An
omitted runtime remains `not_run`. Available evidence is retained on failure.

## Evidence and CI

Each execution writes `run.json`, a Markdown summary, separate stdout/stderr logs,
fixture inputs and configuration snapshots, and the executable when produced.
See [the result protocol](docs/mvp/result-protocol.md) and
[the runtime experiment](docs/mvp/runtime-experiment.md).

PR checks run Rust formatting, Clippy and tests. Main pushes run check/link.
`runtime-experiment.yml` is manually triggered and provisions the pinned Oniro
image before guest execution. All execution/reporting code is Rust; workflow
provisioning uses shell, jq and existing Actions. There is no Python dependency.

The previous Python-based runtime experiment succeeded in
[run 38694437152](https://github.com/rust-on-ohos/ohos-rs-pulse/actions/runs/38694437152).
The Rust replacement requires a new hosted runtime run to establish equivalent
real guest evidence. SDK archive provenance verification remains unresolved and
is disclosed in reports. CI artifacts expire after 14 days; this is not durable
historical storage. ArkTS integration and PostgreSQL remain deferred.
