# Runtime feasibility experiment

The manual `OHOS runtime experiment` workflow builds the existing libc fixture
and executes it in an Oniro x86_64 guest. It does not implement the Rust CLI or
formal run.json schema. ArkTS and PostgreSQL remain outside this experiment.

## Baseline and first build

`config/feasibility.json` fixes Rust 1.99.0, cargo-ohos 0.3.4 and SDK 6.1.0.31 /
API 23. The build script rejects different SDK metadata before check/link.
The first hosted build is preserved as a selected [evidence snapshot](../../evidence/build-38536826695-1/README.md).
The SDK selector remains 6.1 for the build workflow; actual identity must match.

## Guest and image

`config/runtime.json` records Oniro v6.1, release asset ID 390845165 and the
GitHub-published archive SHA-256. The workflow fetches emulator-action commit
07afed5305e89fbdac4f6e8a5560e61e3dcb77ae. It adds an archive digest gate immediately
before extraction in that reviewed Action. It also checks the archive before
startup and after Action completion. Cache restoration cannot bypass the gate.
The upstream Action's optional cache dependency is selected by its upstream tag.

The guest kernel, OS build properties, process identity, hdc version and target
listing are logged. Preconditions include `/lib/ld-musl-x86_64.so.1`,
`/lib/libc.so` and a writable `/data/local/tmp`. The archive identity is distinct
from observed guest OS properties; neither implies compatibility with all OHOS
devices. The original image is booted with disposable writable disks.

## Execution and acceptance

A UUID run ID selects a newly created private guest directory. hdc deploys the
verified build artifact; the fixture receives that run ID and directory.
Guest redirection preserves stdout and stderr separately. The wrapper writes
the child's actual exit code to a separate file. A nonzero-child probe verifies
the status channel before scenario execution. Every shell operation also uses a
fresh status marker rather than relying on hdc's host return code alone.

Runtime passes only when the guest exit status is zero, stdout contains exactly
one JSON completion record matching the scenario and run ID, the fixture's
scratch file is absent, and required directory cleanup succeeds. A guest
assertion/exit failure is `failed`; transport, missing evidence and precondition
failures are `infra_error`. A host execution timeout is `timed_out`, with its
transport/guest attribution left open. Cleanup errors retain an existing failure;
they turn an otherwise successful result into `infra_error`.

Execution has a 60-second host budget, transport operations 30 seconds each.
Timed-out host process groups are killed. The guest child PID is recorded so
cleanup can attempt termination after an execution timeout; emulator teardown
bounds any remaining guest work if transport is unavailable. The workflow always
attempts emulator teardown and uploads available diagnostics, including setup
failures. A build failure blocks guest execution; inspect the build evidence.

## Run and review

After the workflow exists on the repository default branch:

```sh
gh workflow run runtime-experiment.yml
gh run list --workflow runtime-experiment.yml --limit 5
gh run view <run-id>
gh run download <run-id> --name ohos-runtime-<run-id>-1 --dir results/runtime-<run-id>
```

For a provisioned guest and an existing successful build evidence directory:

```sh
python3 scripts/runtime-experiment.py --build results/build-001 --output results/runtime-001
```

The output directory must be new. Outputs include runtime.json, summary.md,
command logs, guest output and status files. CI adds the build evidence,
configuration, image verification, setup/emulator logs and teardown diagnostics.
Artifacts expire after 14 days. Local transport tests verify acceptance boundaries
but provide no OHOS runtime evidence.

No real runtime result is claimed until a hosted attempt is inspected. Run the
manual workflow at least three times, retaining separate IDs and attempts; review
startup/execution durations and every failure before migrating orchestration into
Rust. Do not automatically retry and discard earlier failures.

## Open provenance issue

SDK version/API are checked, but the SDK downloader's archive digest and
attestation verification gap remains unresolved. Reports disclose this gap.
The emulator archive has a pinned digest; this does not close SDK provenance.
