# libc-file-io

A standalone `file-round-trip` scenario using upstream crates.io libc 0.2.190 with default features enabled. This fixture is independent of the future CLI package.

It creates a file exclusively, writes known bytes, closes and reopens it, verifies the bytes and EOF, then closes and unlinks it. Native I/O handles short transfers and EINTR. Existing files are never overwritten or removed. A success record is emitted only after assertions and explicit cleanup succeed.

## Host development check

From the repository root, choose a unique run ID and an existing writable directory:

```sh
cargo clippy --locked --manifest-path fixtures/libc-file-io/Cargo.toml -- -D warnings
cargo run --locked --manifest-path fixtures/libc-file-io/Cargo.toml -- /tmp pulse-host-001
```

The successful process emits one JSON line with `scenario_id`, `run_id`, and `status`. Run IDs accept 1–128 ASCII letters, digits, or hyphens. Failure returns a nonzero exit status and diagnostic output. Host execution does not establish OpenHarmony runtime compatibility.

## OpenHarmony check and link

Install the Rust target, cargo-ohos, and an OpenHarmony native SDK before running these commands. Set `OHOS_SDK_NATIVE` to the selected SDK's native directory to avoid implicit SDK discovery.

```sh
cargo ohos check -t x86_64 --locked --manifest-path fixtures/libc-file-io/Cargo.toml
cargo ohos build -t x86_64 --locked --manifest-path fixtures/libc-file-io/Cargo.toml
```

The executable is produced at `fixtures/libc-file-io/target/x86_64-unknown-linux-ohos/debug/libc-file-io` with the default target directory.

## Initial local experiment

On 2026-10-14, host lint/execution and x86_64 OHOS check/link succeeded with:

- Host: Apple Silicon macOS.
- Rust: 1.99.0, commit b940084d7eb6a299eb4bfeb8e34901bc051e7ac4.
- cargo-ohos: 0.3.4.
- Native SDK: 26.0.0.38, API 26, discovered from the local installation.
- libc: 0.2.190, pinned in Cargo.toml and Cargo.lock.

The experiment used offline locked Cargo commands. It also verified cleanup after success, preservation of a preexisting file, rejection of an invalid run ID, and failure for a missing directory.

These are local development observations, not a normalized CI result or validation of the proposed SDK 6.1 environment. OHOS runtime execution and GitHub-hosted check/link have not yet been verified. The next experiment must pin a CI-available SDK and record its actual identity before these results can be compared.

## CI feasibility workflows

`pr-checks.yml` runs formatting, linting, and the host fixture contract checks on pull requests and main pushes. `cross-build.yml` runs on main pushes or manual dispatch and attempts the SDK 6.1 candidate on ubuntu-24.04. Rust and cargo-ohos versions are selected by `config/feasibility.json`. SDK setup uses cargo-ohos's existing downloader; the resulting native SDK version and API are captured rather than assumed.

The build experiment is a temporary script before the CLI is implemented:

```sh
python3 scripts/cross-build-experiment.py --output results/local-experiment-001
```

Prerequisites are Python 3.9 or newer, the configured Rust version and target, cargo-ohos, and a native SDK. The script uses an already installed SDK; it does not install tools or start an emulator. Local experiments may use a different SDK from CI, and their actual SDK metadata remains visible in the output. Use a fresh output directory for each run.

Outputs include `experiment.json`, `summary.md`, separate command logs, fixture source/lockfile copies, Cargo dependency metadata, ELF inspection, and the executable. This is explicitly a feasibility evidence format, not the proposed CLI result-v1 schema. Runtime and integration remain `not_run`. Check/link each have a ten-minute timeout and failed check blocks link. Logs and summary are uploaded even after a failing experiment; setup failures have workflow diagnostics without an invented scenario result.

GitHub artifacts are retained for 14 days. After pushing these files, inspect the first workflow run's setup logs, summary, and downloadable artifact. Until that run completes, hosted build feasibility remains unverified. The SDK release selector is a candidate input; the first successful run must establish exact package identity before promoting it to a baseline.
