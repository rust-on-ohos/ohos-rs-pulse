# Manual ARM64 / OpenHarmony 7.0 ABI experiment

Status: first real ARM64 guest execution passed; repeatability is not yet established.
This is separate from the OH 6.1 / x86_64 baseline. It reuses upstream libc-test
0.2.190 and ctest 0.5.1 with an explicit bounded test-configuration patch.

## Environment

| Input | Pin / observation |
|---|---|
| Guest | OpenHarmony-7.0.0.39, API 26, aarch64, Linux 6.6.101 |
| Image | harmony-contrib/ohos-qemu v20260919, ARM64 phone |
| Emulator manager | cargo-ohos 0.3.4 |
| SDK | 26.0.0.38 / API 26 |
| Rust target | aarch64-unknown-linux-ohos |
| Connection | 127.0.0.1:5555 |

The expected image archive digest is recorded in
`config/upstream-arm64-runtime.json` from cargo-ohos's pinned image catalog.
The runtime command observes the guest architecture, OS and API but does not
attest which archive booted. The SDK archive provenance gap remains open.

## Start and check the guest

```sh
cargo ohos init emulator --device phone
cargo ohos emulator start aarch64-phone --timeout 300
cargo ohos emulator status
hdc tconn 127.0.0.1:5555
hdc -t 127.0.0.1:5555 shell uname -a
hdc -t 127.0.0.1:5555 shell param get const.ohos.fullname
hdc -t 127.0.0.1:5555 shell param get const.ohos.apiversion
```

If a long-running instance stops responding, use `cargo ohos emulator stop
aarch64-phone`, then start it again. Restart preserves the instance disks; reset
and delete are unnecessary for normal verification.

## Build

Run from the project root. The SDK path below is the verified local installation.
Use the clean pinned `.upstream-libc` checkout from the original experiment.

```sh
export OHOS_SDK_NATIVE="$HOME/Library/OpenHarmony/Sdk/26.0.0/native"
export PATH="$(dirname "$OHOS_SDK_NATIVE")/toolchains:$PATH"
rustup target add aarch64-unknown-linux-ohos --toolchain 1.99.0
cargo build --locked

pulse_attempt="$(date -u +%Y%m%dT%H%M%SZ)-$$"
pulse_build="results/upstream-arm64-$pulse_attempt-build"
pulse_guest="results/upstream-arm64-$pulse_attempt-guest"

target/debug/ohos-pulse upstream-build \
  --source .upstream-libc \
  --build-config config/upstream-arm64-build.json \
  --profile sdk26-arm64-core \
  --output "$pulse_build"
```

Only continue if the build and inspection pass. The CLI applies the digest-pinned
patch to an isolated copy, records its identity and resulting build script, and
preserves the original source archive. It does not modify the checkout or libc
bindings. The resulting artifact must be AArch64 with the ARM64 musl loader.

## Execute and repeat

```sh
target/debug/ohos-pulse upstream-runtime \
  --build "$pulse_build" \
  --runtime-config config/upstream-arm64-runtime.json \
  --output "$pulse_guest"
cat "$pulse_guest/summary.md"
```

The runtime verifies guest architecture, OS and API before deployment; checks the
loader, dependencies and deployed digest; captures guest exit status and separate
stdout/stderr; validates the original upstream harness completion; and cleans up.
Use a fresh output directory for every repeat:

```sh
target/debug/ohos-pulse upstream-runtime \
  --build "$pulse_build" \
  --runtime-config config/upstream-arm64-runtime.json \
  --output "results/upstream-arm64-$(date -u +%Y%m%dT%H%M%SZ)-$$-guest"
```

## Scope and existing build evidence

The patch contains explicit alias, struct, constant and function allowlists for
basic file, memory, time and IPv4 socket interfaces. Unions, statics and all other
APIs are outside this profile. Upstream ctest generates and executes ABI/value/
function-pointer and applicable roundtrip checks; this does not exercise the
application behavior of every selected function. In particular, a passing socket
ABI check does not demonstrate that socket operations work.

Real local evidence, ignored by Git:

| Directory | Outcome |
|---|---|
| results/upstream-arm64-api26-baseline-001 | Unmodified upstream version detection failed |
| results/upstream-arm64-api26-kernel-probe-001 | Version detection fixed; full target C compilation still failed |
| results/upstream-arm64-api26-core-001 | Bounded profile built and passed ELF inspection; 236 generated test functions |
| results/upstream-arm64-20261016T081134Z-4807-guest | Guest exit 0; upstream harness reported 270 successful comparisons; cleanup passed |

The first guest run, `pulse-54b4ca8e8e754a56bb16abd9eae79182`, completed on
2026-10-16. Captured stdout is `RUNNING ALL TESTS` followed by `PASSED 270 tests`;
stderr is empty. Architecture, OS and API checks, deployed digest verification,
dependency inspection and cleanup passed. The 236 generated functions can each
perform multiple comparisons, so their count differs from the 270 harness checks.
Evidence is local and ignored by Git; it has not been remotely archived.

The full target still encounters header conflicts and missing declarations.
A passing bounded result does not resolve those blockers. Keep its result
separate from the full target. Comparison counts must come from actual guest
output; total excluded checks is unknown. Inspect `upstream.json`, `summary.md`,
the preserved patch/generated sources and `logs/` for each invocation.

The existing GitHub workflow continues to use the OH 6.1 / x86_64 environment.
This ARM64 recipe currently runs locally through cargo-ohos-managed QEMU.
