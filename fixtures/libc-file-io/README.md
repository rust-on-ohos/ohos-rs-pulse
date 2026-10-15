# libc-file-io

A standalone `file-round-trip` executable using crates.io libc 0.2.190 with
`default` and `std` enabled. It is an independent Cargo package.

It creates a file exclusively, writes known bytes, closes and reopens it, verifies
contents and EOF, then closes and unlinks it. Native I/O handles short transfers
and EINTR. Existing files are never overwritten or removed. A completion record
is emitted only after assertions and explicit cleanup succeed.

## Host development checks

```sh
cargo test --locked --manifest-path fixtures/libc-file-io/Cargo.toml
cargo clippy --locked --all-targets --manifest-path fixtures/libc-file-io/Cargo.toml -- -D warnings
cargo run --locked --manifest-path fixtures/libc-file-io/Cargo.toml -- /tmp pulse-host-001
```

The Rust contract test verifies success, cleanup, collision preservation, invalid
IDs and missing directories. Host execution does not establish OHOS compatibility.
Run IDs accept 1–128 ASCII letters, digits or hyphens. Choose a unique ID for each
invocation. Failure returns a nonzero status and emits diagnostics without a
completion record.

## OpenHarmony execution

Install the pinned tools/SDK and build the Rust CLI from the repository root:

```sh
cargo build --locked --bin ohos-pulse
target/debug/ohos-pulse run --output results/build-001
```

The CLI checks installed versions against config/feasibility.json, runs locked
OHOS check/link, inspects the executable and writes run.json and summary.md.
Runtime requires a provisioned guest; see the [runtime experiment](../../docs/mvp/runtime-experiment.md).

## Historical evidence

The initial local experiment used Rust 1.99.0, cargo-ohos 0.3.4 and SDK 26.0.0.38 /
API 26 on Apple Silicon macOS. Hosted check/link subsequently passed with SDK
6.1.0.31 / API 23; see the [first build snapshot](../../evidence/build-38536826695-1/README.md).
The Python predecessor also executed this scenario successfully in Oniro's
OpenHarmony 6.1.0.31 guest. Rust orchestration replaces that predecessor and
requires its own hosted validation. No crate-wide compatibility claim follows.
