# Upstream libc experiment findings

## 2026-10-16: SDK-header kernel-version detection blocker

Run: https://github.com/rust-on-ohos/ohos-rs-pulse/actions/runs/38856216267
Artifact: upstream-libc-38856216267-1 (14-day retention).

Verified preparation: clean libc 0.2.190 at commit
7b0ab5528dc7f361f3a0b4c06c2cad9971617f75, reviewed Cargo.lock and ctest 0.5.1 template,
Rust 1.99.0, cargo-ohos 0.3.4, SDK 6.1.0.31 / API 23, OHOS x86_64 C compiler/sysroot.

The real upstream test-build returned 101. libc-test/build/main.rs:4072 panicked:

```text
failed to detect kernel version for Linux target
```

The build script logged `Versions { linux: None, ... }`. Its Linux version probe
uses the target C compiler's preprocessor, conditionally includes linux/version.h
and recognizes LINUX_VERSION_MAJOR and LINUX_VERSION_PATCHLEVEL. test_linux then
requires a detected version. The OHOS triple takes that Linux path.

This confirms an upstream test configuration/probe blocker in this SDK environment.
It does not establish that a libc ABI assertion failed. No ctest_output.rs was
produced, no executable was inspected, and the emulator/runtime phases did not run.
The secondary "generated ctest output; found 0" diagnostic is a consequence of
stopping before generation, not the primary failure.

The hosted logs alone do not identify which header or macro is missing. The local
probe below identifies the cause in the macOS SDK package. The CLI now captures a
separate SDK-targeted preprocessor diagnostic for subsequent builds without
modifying upstream sources or overriding their build result.

## 2026-10-16: real local API 23 reproduction and candidate probe fix

Host: Apple Silicon macOS; target: x86_64-unknown-linux-ohos.
Actual SDK: 6.1.0.31 / API 23, installed macOS ARM64 package.
Rust 1.99.0 and cargo-ohos 0.3.4 match CI. The host and SDK host binaries differ
from Ubuntu CI, so this is a separate observation, not an identical-host rerun.

Local evidence, ignored by Git:

| Attempt | Result |
|---|---|
| results/upstream-local-api23-002 | Clean upstream build reproduced the same version-detection panic |
| results/upstream-local-api23-patched-002 | Candidate probe fix with -ferror-limit=0; C compilation emitted 363 error diagnostics |

The actual local SDK-targeted preprocessor dump contains:

```text
#define __linux__ 1
#define PULSE_HAS_LINUX_VERSION_HEADER 1
#define LINUX_VERSION_CODE 330240
```

It does not contain LINUX_VERSION_MAJOR or LINUX_VERSION_PATCHLEVEL. The encoded
value decodes to major 5, minor 10, patch 0. This is the SDK header's version marker,
not a claim about the guest's running kernel (previously observed as 6.6.101).

The isolated candidate [patch](../../patches/libc/0.2.190-kernel-version-code.patch)
adds a fallback decoder and leaves upstream assertions, bindings and skip rules
unchanged. Its SHA-256 is
27336208211ced15e3c6d35471e181995aec071184bc80ca1196b7bb09416c43.
The baseline checkout remains clean. The candidate is not enabled by the workflow
or the normal clean-source CLI path.

After this fix, the build script reported linux: Some((5, 10)) and generated
ctest_output.rs and ctest_output.c with 8,075 functions in run_all. None executed:
C compilation still returned 101 through Cargo. Examples include:

- Repeated __packed declarations in linux/if_pppox.h.
- A sysinfo redefinition involving the SDK headers.
- Missing CAN_ERR_CNT and J1939 constants.
- Undeclared pthread_tryjoin_np, pthread_timedjoin_np and utmp/shadow-related APIs.

Disabling Clang's error limit changes diagnostic collection only. The 363 emitted
errors include possible cascades and header-composition effects; do not report
363 confirmed libc binding defects. Separate SDK header hygiene, upstream test
configuration and real binding/declaration mismatches through smaller probes.
No further skips or compatibility patches have been applied.

Generated sources, patch copies, Cargo JSON output and complete stderr are retained
in the patched local result directories. They are local evidence, not committed
or remotely archived; export them when needed for upstream review. No executable
or guest runtime result was produced. Next, classify the errors with minimal
probes and determine justified adaptations before attempting guest execution.
