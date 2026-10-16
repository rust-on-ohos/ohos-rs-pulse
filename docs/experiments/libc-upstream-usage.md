# Running the upstream libc ABI experiment

Status: implementation ready for review; no hosted upstream-suite outcome yet.
This experiment is separate from the existing libc file-round-trip workflow.

## Reviewed inputs

`config/upstream-libc.json` selects the initial libc 0.2.190 source commit
7b0ab5528dc7f361f3a0b4c06c2cad9971617f75, its lockfile digest, libc-test's ctest target,
and Cargo.lock's ctest 0.5.1 generator. Its template digest is independently checked
before compilation. SDK/tools and guest use the existing JSON profiles.

This is the initial recipe, not a permanent version ceiling. Future scheduled
release discovery should select a release, resolve its immutable commit and
write an invocation-specific profile. Every execution remains pinned and
reproducible. Release discovery/scheduling is not implemented by this change;
an unsupported generator/harness must fail adapter review instead of silently
passing a new output format.

## Manual GitHub Actions experiment

After reviewing and pushing the workflow:

```sh
gh workflow run upstream-libc-experiment.yml
gh run list --workflow upstream-libc-experiment.yml --limit 5
gh run view <run-id>
gh run download <run-id> --name upstream-libc-<run-id>-1 --dir results/upstream-<run-id>
```

The workflow checks out upstream at the configured commit, installs the pinned
SDK/tools, builds ctest without running it, provisions the verified Oniro image,
and invokes Rust guest execution. It has no arbitrary ref/URL/command inputs and
no automatic retries. Build failure prevents emulator startup. Setup logs and
available experiment evidence upload even on failure. Artifacts expire in 14 days.
The original smoke-test workflows remain unchanged.

## Local commands

Prerequisites: reviewed clean upstream Git checkout, pinned Rust/cargo-ohos/SDK,
OHOS target installed, and hdc on PATH for execution. SDK installation and guest
startup remain provisioning responsibilities. Commands use new output directories.

```sh
cargo build --locked --bin ohos-pulse
target/debug/ohos-pulse upstream-build \
  --source /path/to/reviewed/libc-checkout --output results/upstream-build-001

# After preparing the guest described by config/runtime.json:
target/debug/ohos-pulse upstream-runtime \
  --build results/upstream-build-001 --output results/upstream-runtime-001
```

Only a clean source tree at the selected commit with the expected lockfile is
accepted. No test assertions, skips or upstream source files are patched by the
CLI. Untracked files also cause rejection. SDK environment discovery runs in the
upstream package, not the file-round-trip fixture. All command argument vectors,
identity checks and compiler output are logged.

## Completion interpretation

The reviewed generator prints `RUNNING ALL TESTS`, calls its generated run_all,
checks FAILED and prints `PASSED N tests` only on successful completion. N counts
successful comparisons, not generated test functions. The CLI requires exactly
those two nonempty stdout lines with N greater than zero, zero guest exit status,
a nonempty generated inventory and successful cleanup. stderr is retained for
review; it is not used as a substitute for exit/summary evidence.

The generated main must match the reviewed main body. Inventory entries must be
unique function calls with corresponding definitions. Main/function source and
executable digests are verified before execution. `sha256sum` must be available
in the guest to verify deployed bytes; absence is an explicit infrastructure
blocker. Test processes do not receive the smoke fixture's directory/run-ID
arguments; invocation isolation comes from the private directory and fresh logs.

The adapter does not invent a total comparison count or exact skip count. Both
are null until a trustworthy counting method exists. Generated functions are a
separate inventory. Upstream skip/cfg rules and generated sources are retained,
so exclusions remain reviewable. A nonempty summary does not establish coverage
of every libc binding or behavior.

## Evidence format

The independent `ohos-pulse-upstream-libc-experiment-1` format is written as
`upstream.json`, with Markdown re-generated from the serialized JSON. It is not
result-v1/run.json and cannot be imported as a smoke fixture result.

Groups include invocation/upstream identity, build pins, lockfile digest, resolved
features, generator identity, SDK environment, rustc, preparation/test_build/
inspection/execution phases, artifact and coverage, observed guest environment,
cleanup, command records, diagnostics and SDK provenance disclosure.

Build evidence retains:

- upstream-source.tar from the verified Git commit, including upstream licenses;
- Cargo.lock, configuration snapshots, selected upstream build/target rules;
- generator-template.rs and generated C/Rust sources where available;
- Cargo machine-readable artifact output and separate command logs;
- inspected ctest executable and SHA-256 after successful production.

Large target intermediates are deleted after diagnostics are captured. An upstream
C compilation failure may occur before generated output exists; the report states
that limitation. Runtime evidence copies build evidence beneath build-evidence/
and rebases artifact/source/log paths to remain portable. CI also retains image
verification, SDK setup and emulator teardown evidence.

Exit codes: 0 if requested phases pass; 1 for test-build/assertion failure, blocked
execution or timeout; 2 for infrastructure/input/reporting errors. Cleanup errors
retain existing assertion failures and take precedence in the exit code. SDK
archive digest/attestation acceptance remains unresolved and disclosed.

## Validation and next action

Rust tests use local command doubles, not an SDK or guest. They cover source
rejection, generated-source capture after a failed build, artifact discovery,
harness changes/empty coverage, zero/missing summaries, nonzero guest status,
transfer/deployed-digest/cleanup failures and timeout. Existing smoke tests also
run to catch transport regressions.

The next real action is a maintainer-reviewed hosted manual attempt. Keep its
first unmodified build/runtime outcome even if it fails. Diagnose missing headers,
ABI disagreements or environment assumptions before proposing any OH-specific
patch. Repeatability and scheduled new-release testing follow recipe validation.
