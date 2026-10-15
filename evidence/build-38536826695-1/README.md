# First hosted build evidence

Source: https://github.com/rust-on-ohos/ohos-rs-pulse/actions/runs/38536826695

This reviewed snapshot preserves the first successful hosted check/link report,
summary, compiler diagnostics, SDK setup log and ELF inspection. The complete
original artifact was named `ohos-cross-build-38536826695-1` and retained for 14
days. This directory is a selected evidence snapshot, not the full downloadable
artifact; report references to the executable and other logs are original paths.
The executable SHA-256 was independently checked after download against the report.

Verified combination: Rust 1.99.0, cargo-ohos 0.3.4, upstream libc 0.2.190 with
`default` and `std`, SDK 6.1.0.31 / API 23, x86_64-unknown-linux-ohos.
Check and link passed; runtime and ArkTS integration were not run.

SDK download attestation verification was skipped by the installer. Exact SDK
archive digest/attestation verification remains an open provenance gap; the
recorded version/API alone do not resolve it. This exceptional first-build
snapshot does not establish a policy of committing every generated result.
