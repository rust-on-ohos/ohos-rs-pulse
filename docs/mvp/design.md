# MVP Design — One libc Scenario, One Evidence Workflow

**Status:** Draft / proposed contracts  
**Date:** 2026-10-14  
**Parent:** [HLD](../high-level-design.md)

## 1. Outcome and scope

A developer can run one reviewed libc scenario through `ohos-pulse`, locally or in GitHub Actions, and obtain a report stating what was checked, linked, executed, or not executed, with exact inputs and supporting logs.

| Included | Deferred |
|---|---|
| One upstream libc release and one locked dependency graph | Additional crates, including napi-ohos |
| One executable fixture and file I/O scenario | epoll, mmap, ABI layout suites, ArkTS applications |
| One x86_64-unknown-linux-ohos target and SDK configuration | Architecture, SDK, or feature matrices |
| Check and link execution; a bounded runtime experiment | Required nightly runtime, device fleet, self-hosted infrastructure |
| JSON evidence and generated Markdown | Baseline comparison, failure taxonomy engine, dashboards |
| A small Rust CLI and Actions orchestration | General plugin system, arbitrary commands, automatic upstream issues |

The first users are developers investigating a concrete platform failure. A report describes this scenario only; it does not certify libc as a whole.

## 2. Repository boundary

```text
Cargo.toml                         # CLI package: ohos-pulse-cli
Cargo.lock
rust-toolchain.toml
src/                               # binary: ohos-pulse
fixtures/libc-file-io/
  Cargo.toml
  Cargo.lock
  src/main.rs
  README.md
config/mvp.toml                     # one versioned test/environment definition
schemas/result.schema.json
.github/workflows/
  pr-checks.yml
  cross-build.yml
  runtime-experiment.yml
docs/mvp/
results/                           # generated, ignored by Git
```

The CLI is initially a single Cargo package. The fixture is an independent package with its own workspace boundary and lockfile so host CLI checks do not accidentally build it as a workspace member. Create implementation paths only when needed.

Use one small typed TOML configuration rather than a general matrix planner. It identifies the fixed fixture and scenario, executable target, target triple, selected feature settings, SDK/tool versions, and stage timeouts. CI setup and CLI verification consume the same pins; avoid separately maintained version values. Reject unknown fields, unsupported configurations, and fixture paths outside the repository after resolving symlinks. Configuration does not accept shell commands, download URLs, or arbitrary executable paths.

## 3. Fixture contract

**Fixture:** `libc-file-io`  
**Scenario:** `file-round-trip`  
**Artifact:** standalone executable

The executable receives a writable test directory and a per-run identifier. It creates a unique file exclusively, using `libc::open` with the selected flags and explicit creation mode. It writes a fixed nonempty byte sequence with `libc::write`, closes the descriptor, reopens the file for reading, and reads through `libc::read` until the expected content and EOF have been verified. It closes the descriptor and removes the file through `libc::unlink`.

Handle short reads/writes and interrupted I/O explicitly. Check return values and report errno for failed native operations. Avoid uninitialized buffer reads, invalid pointer lifetimes, and blind retries of close. Attempt cleanup on failure and preserve both the original error and any cleanup error. A cleanup failure prevents a passing scenario. Rust standard-library code may support argument handling and diagnostics, but must not replace the native operations under test.

On complete success, print exactly one machine-readable completion record containing the scenario ID and the current run identifier. The run identifier must be passed to the actual executable. Runtime passes only when the guest exit status is zero and the expected completion record is present. A stale record, wrong identifier, missing record, or nonzero exit cannot pass.

This is assertion evidence from a reviewed fixture, not a security attestation. A host execution may verify fixture behavior during development, but never counts as OpenHarmony runtime evidence.

## 4. CLI surface

Proposed commands:

```sh
ohos-pulse run --config config/mvp.toml --output results/local-001
ohos-pulse run --config config/mvp.toml --runtime --output results/local-002
ohos-pulse report --input results/local-001/run.json --output results/local-001/summary.md
```

The default run performs preflight, check, and link. Runtime requires explicit opt-in and a pre-provisioned guest. The CLI invokes existing build tools and hdc; Actions or the developer installs the SDK and starts the emulator. SDK downloads and emulator lifecycle are outside the CLI's MVP responsibility.

The target build command and deployment invocation must be established by the feasibility experiment before being implemented. The CLI constructs argument vectors directly rather than evaluating shell strings from configuration. Check and link use the same executable target, resolved features, lockfile, and target triple. Link evidence includes the actual artifact path, digest, architecture, and dynamic dependency inspection where applicable.

The output directory must not already exist. Within it, each stage writes distinct logs. No automatic retries are performed in the MVP; a rerun uses a new output directory and preserves the previous evidence.

| Exit code | Meaning |
|---|---|
| 0 | All requested stages passed and evidence was written |
| 1 | A requested stage failed, was blocked, or timed out; inspect stage evidence |
| 2 | Invalid input, infrastructure failure, or CLI/reporting error |

A deliberately omitted runtime stage does not make a default check/link run fail. Never return zero when requested runtime did not run. If multiple outcomes apply, exit code 2 takes precedence over 1. Write available evidence before returning a nonzero code whenever storage remains usable.

## 5. Execution and status rules

1. Validate configuration, lockfile, output destination, and declared pins; capture tool and SDK identity.
2. Execute target check and capture command, stdout, stderr, exit code, and duration.
3. If check passes, build/link the declared executable and inspect the artifact.
4. If runtime was requested and link passed, verify guest identity, deploy into a unique guest directory, execute with a bounded timeout, collect guest output and actual exit status, and attempt cleanup.
5. Write validated JSON atomically, then generate Markdown from that JSON.

| Condition | Result |
|---|---|
| Stage satisfies its acceptance criteria | `passed` |
| Compile/link error or executed assertion failure | `failed`, with cause left open |
| Failed predecessor prevents a requested stage | `blocked`, referencing that predecessor |
| Runtime was not requested | `not_run` |
| Tool/SDK unavailable, guest startup or transport fails | `infra_error` at the affected stage |
| Stage exceeds its configured budget | `timed_out` |
| ArkTS integration | Always `not_run`, with an out-of-scope reason |

An omitted stage remains `not_run` even if a previous stage failed. A preflight failure marks check as `infra_error` and requested downstream stages as `blocked`. CLI filesystem failures may prevent any valid report; return code 2 with a clear diagnostic rather than fabricating evidence.

Transport failure must not be confused with the executable's exit status. Deployment, identity discovery, and cleanup failures are infrastructure diagnostics. If the scenario already failed, retain that failure and attach cleanup diagnostics; if it passed but required transport cleanup fails, runtime is `infra_error` with the successful assertion evidence retained.

Bound all subprocesses and terminate their process groups on timeout. Proposed initial budgets are 10 minutes each for check/link and 60 seconds for scenario execution; deployment and transport commands also require finite budgets. Actions owns emulator startup limits and teardown even after failure. These values are starting defaults, to be adjusted using measured runs.

## 6. Evidence contract

One run contains exactly one scenario record. The MVP does not implement aggregation across fixtures.

```text
results/<run-id>/
  run.json
  summary.md
  logs/preflight.log
  logs/check.stdout.log
  logs/check.stderr.log
  logs/link.stdout.log
  logs/link.stderr.log
  logs/runtime.stdout.log           # when attempted
  logs/runtime.stderr.log          # when attempted
```

Required JSON groups:

| Group | Required information |
|---|---|
| `schema_version` | Integer protocol version, initially 1 |
| `run` | Unique ID, UTC time, CLI version, source commit and dirty state; optional CI URL/run ID |
| `subject` | Fixture/scenario IDs, artifact kind, fixture content SHA-256, lockfile SHA-256, resolved libc version and features |
| `environment` | Host identity, target, rustc verbose version, Cargo/build-tool versions, SDK package identity and API metadata |
| `runtime_environment` | Null if unavailable; otherwise distribution, image reference/digest when obtainable, OS build, transport target, execution context |
| `stages` | All four stage names, statuses, reasons, commands as argument arrays, duration, exit code where available, relative log references |
| `artifact` | Null before successful production; otherwise relative artifact reference, SHA-256, architecture, dependency inspection evidence |

The fixture digest covers sorted repository-relative paths and file bytes for Cargo.toml, Cargo.lock, and fixture source files; generated build files are excluded. Hash inputs must be documented with the implementation. A dirty checkout is allowed only when labeled, and reproduction must retain the patch or source snapshot; a commit alone cannot identify modified source.

Unavailable metadata is null with an explicit reason, never an invented version. Required build pins must be verified before build execution. Runtime reports must expose missing image provenance; recording a distribution name alone does not establish image equivalence.

The JSON Schema must validate status-dependent requirements, including reasons for skipped/blocked/error outcomes and command/exit evidence for executed stages. Validate relative artifact paths, escape report content, and avoid environment dumps or secret-bearing command arguments. The Markdown includes the scenario boundary, environment, stage table, diagnostics, reproduction instructions, and artifact retention policy. It contains no single crate-wide compatibility badge.

## 7. GitHub Actions responsibilities

| Workflow | Trigger | Responsibility |
|---|---|---|
| `pr-checks.yml` | Pull requests and main pushes | Formatting, linting, host tests, configuration/schema checks; no guest required |
| `cross-build.yml` | Main pushes and manual dispatch initially | Install pinned tools/SDK, run default CLI path, upload evidence and publish summary |
| `runtime-experiment.yml` | Maintainer manual dispatch initially | Prepare the same build environment, start pinned emulator image, run CLI with runtime enabled, collect diagnostics, stop emulator |

Use ubuntu-24.04, read-only contents permissions, reviewed Action commit SHAs, and no secrets in build/test jobs. Manual inputs cannot select arbitrary refs, commands, or download URLs. Keep runtime outside required PR checks until repeated runs demonstrate feasibility.

Evidence upload and teardown run even after CLI failure. Preserve the CLI exit status so uploading artifacts does not make a failing execution appear successful. An experimental workflow may fail without being a required merge check. If setup fails before CLI startup, retain setup logs and an Actions summary identifying infrastructure failure; do not invent a scenario result.

Set explicit job timeouts and artifact retention, initially 14 days. Publish the normalized JSON, Markdown, and necessary diagnostics without committing results to main. If a reproduction requires the executable, include it under the artifact reference recorded in JSON. Caching is optional and follows a successful uncached run, not a prerequisite for the first experiment.

## 8. Implementation sequence and validation

| Step | Deliverable | Exit evidence |
|---|---|---|
| 1. Feasibility | Minimal fixture and pinned build recipe | Real hosted check/link logs, exact commands, verified configuration or a reproducible blocker |
| 2. Evidence workflow | Small CLI, result schema, Markdown generation | One actual build outcome produces a validated and inspectable report |
| 3. CI integration | Host checks and cross-build workflow | Downloadable evidence survives both success and failure |
| 4. Runtime experiment | Guest deployment and assertion collection | An actual runtime outcome, or an explicit documented execution blocker |
| 5. Reproduction review | Short fixture/CLI instructions | A clean checkout can repeat the documented flow with stated prerequisites |

Test meaningful failure boundaries: nonzero commands preserve logs; timeouts terminate children; failed check blocks requested successors; runtime omission remains not_run; a missing/wrong completion record cannot pass; transport failure is not an assertion failure; report generation preserves all stage statuses. Validate the fixture's native resource handling and the JSON/report relationship. Do not build a generic scheduler to support one scenario.

## 9. MVP acceptance and open decisions

The MVP is complete when one real hosted check/link attempt yields usable evidence, the CLI and Actions share the same execution path, the result can be reproduced, and the runtime experiment has a documented outcome or blocker. A check/link-only delivery is acceptable when labeled. Positive crate compatibility is not a release requirement; trustworthy evidence is.

Before implementation, resolve through the feasibility experiment:

- The exact libc release, feature selection, Rust toolchain, SDK package/API, cargo-ohos version, and verified commands.
- The artifact interpreter/dependencies and their availability in the candidate guest.
- The guest writable directory, hdc exit-status propagation, and timeout/cleanup behavior.
- The pinned emulator image and how its identity can be recorded.

The SDK 6.1 and Oniro x86_64 combination from the HLD remains a candidate, not a verified baseline. napi-ohos integration starts only after reviewing this MVP's results and maintenance cost.
