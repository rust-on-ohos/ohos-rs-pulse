# Runtime experiment with the Rust CLI

The manual `OHOS runtime experiment` workflow builds the existing libc fixture
and executes it in an Oniro x86_64 guest. Rust `ohos-pulse` now owns check/link,
hdc deployment, bounded execution, evidence acceptance, cleanup and reporting.
Python orchestration and workflow snippets have been removed.

## Verified baseline and migration status

`config/feasibility.json` fixes Rust 1.99.0, cargo-ohos 0.3.4 and SDK 6.1.0.31 /
API 23. SDK setup downloads release 6.1 and checks installed version/API.
Release queries receive the job's read-only token only during SDK installation;
fixture/build execution receives no GitHub token.

The first build is preserved in the selected [evidence snapshot](../../evidence/build-38536826695-1/README.md).
The Python predecessor obtained a real successful runtime outcome in
[run 38694437152](https://github.com/rust-on-ohos/ohos-rs-pulse/actions/runs/38694437152).
The guest reported OpenHarmony 6.1.0.31, kernel 6.6.101 and root shell context.
Completion, actual guest exit status and cleanup all passed. The executable's
libc dependency resolved through `/lib/ld-musl-x86_64.so.1`.

That observation establishes the original execution recipe. The Rust replacement
has local acceptance/failure tests but needs new hosted runs before claiming
runtime equivalence and repeatability. Keep previous failed attempts as evidence.

## Image provisioning

`config/runtime.json` records Oniro v6.1, release asset ID 390845165 and its pinned
SHA-256. The workflow fetches emulator-action commit
07afed5305e89fbdac4f6e8a5560e61e3dcb77ae and inserts a digest gate immediately before
archive extraction. It verifies the archive before startup and after Action
completion. The upstream Action's optional cache dependency uses its upstream tag.

The CLI records observed kernel/build properties, process identity, hdc version,
target connection and loader dependencies. It checks the expected interpreter and
prepares `/data/local/tmp`; it does not assume libc exists at `/lib/libc.so`.
Image identity and observed guest OS properties remain distinct. Local execution
records an expected image profile; the CLI cannot attest image provenance through
hdc. CI adds archive verification evidence.

## Execution and acceptance

Each invocation creates a unique private directory. The verified executable is
sent with hdc and receives the run ID. Guest redirection captures stdout/stderr
separately, and a separate file stores the child's actual exit code. A deliberately
nonzero child verifies exit-status collection before the fixture executes. Fresh
status markers check remote shell operations independently of hdc's host exit code.

Runtime passes only with zero guest exit status, exactly one matching completion
record, no leftover fixture scratch file and successful required directory cleanup.
Failures preserve their logs and diagnostics. Cleanup failure retains an existing
scenario failure and converts otherwise successful runtime to infra_error.

Execution has a 60-second host budget; transport operations have 30 seconds each.
Host process groups are killed on timeout. A tracked guest PID allows cleanup to
attempt termination; workflow emulator teardown bounds guest work after transport
failure. The workflow always attempts teardown and evidence upload after failure.
SDK setup, image installation and emulator lifecycle remain workflow responsibilities.

## Run and review

After pushing the Rust replacement:

```sh
gh workflow run runtime-experiment.yml
gh run list --workflow runtime-experiment.yml --limit 5
gh run view <run-id>
gh run download <run-id> --name ohos-runtime-<run-id>-1 --dir results/runtime-<run-id>
```

Provisioned guest, fresh build and runtime evidence:

```sh
cargo build --locked --bin ohos-pulse
target/debug/ohos-pulse run --output results/build-001
target/debug/ohos-pulse runtime --build results/build-001 --output results/runtime-001
```

The workflow retains build/run.json, guest/run.json, summaries, logs, source and
configuration snapshots, executable, image verification and teardown diagnostics.
The runtime result also contains a portable copy of build evidence. Setup failures
have setup-outcome.json rather than an invented fixture result. Artifacts expire
after 14 days. Review at least three independent Rust runtime attempts before
calling the workflow repeatable. ArkTS and PostgreSQL remain out of scope.

## SDK provenance issue

Installed SDK version/API are checked, but SDK archive digest/attestation
verification is not an acceptance gate. Reports disclose this unresolved issue.
The emulator archive digest does not close SDK provenance.
