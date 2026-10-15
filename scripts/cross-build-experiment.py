"""Capture feasibility evidence before implementing the CLI/result-v1 contract."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "fixtures/libc-file-io"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=True)
if (output / "experiment.json").exists():
    parser.error("output already contains an experiment; use a new directory")
with (ROOT / "config/feasibility.json").open("rb") as source:
    inputs = json.load(source)


def execute(name, command, timeout=600):
    started = time.monotonic()
    stdout_path = output / f"{name}.stdout.log"
    stderr_path = output / f"{name}.stderr.log"
    status = "failed"
    code = None
    reason = None
    with stdout_path.open("w") as stdout, stderr_path.open("w") as stderr:
        try:
            process = subprocess.Popen(
                command, cwd=FIXTURE, stdout=stdout, stderr=stderr, start_new_session=True
            )
            try:
                code = process.wait(timeout=timeout)
                status = "passed" if code == 0 else "failed"
                if code != 0:
                    reason = "command returned nonzero; attribution requires review"
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
                status = "timed_out"
                reason = "command exceeded its time budget"
        except OSError as error:
            status = "infra_error"
            reason = str(error)
            print(error, file=stderr)
    result = {
        "status": status,
        "command": command,
        "exit_code": code,
        "elapsed_ms": round((time.monotonic() - started) * 1000),
        "stdout": stdout_path.name,
        "stderr": stderr_path.name,
    }
    if reason:
        result["reason"] = reason
    return result


preflight = execute("preflight", ["cargo", "ohos", "env", "-t", inputs["target"], "--format", "json"])
environment = None
if preflight["status"] == "passed":
    try:
        environment = json.loads((output / preflight["stdout"]).read_text())
        if environment["cargo_ohos_version"] != inputs["cargo_ohos_version"]:
            raise ValueError("cargo-ohos version differs from the experiment pin")
        if environment["sdk"]["version"] != inputs["sdk_version"] or environment["sdk"]["api_version"] != inputs["sdk_api_version"]:
            raise ValueError("SDK version/API differs from the verified baseline")
        readelf = Path(environment["toolchain"]["readelf"])
        if not readelf.is_file():
            raise ValueError("SDK readelf tool is unavailable")
    except (ValueError, KeyError, TypeError) as error:
        preflight.update(status="infra_error", reason=str(error))
else:
    preflight["status"] = "infra_error"

stages = {
    "runtime": {"status": "not_run", "reason": "build-only feasibility experiment"},
    "integration": {"status": "not_run", "reason": "outside MVP scope"},
}
artifact = None
if preflight["status"] != "passed":
    stages["check"] = {"status": "infra_error", "reason": "preflight failed; see preflight logs"}
    stages["link"] = {"status": "blocked", "reason": "preflight failed"}
else:
    for stage, subcommand in (("check", "check"), ("link", "build")):
        if stage == "link" and stages["check"]["status"] != "passed":
            stages[stage] = {"status": "blocked", "reason": "check did not pass"}
            continue
        # Isolate artifacts so a stale binary cannot satisfy this experiment.
        command = [
            "cargo", "ohos", subcommand, "-t", inputs["target"], "--locked",
            "--bin", "libc-file-io", "--target-dir", str(output / "build"),
        ]
        stages[stage] = execute(stage, command)
    if stages["link"]["status"] == "passed":
        binary = output / "build" / inputs["target"] / "debug/libc-file-io"
        try:
            artifact_directory = output / "artifacts"
            artifact_directory.mkdir()
            copied = artifact_directory / "libc-file-io"
            shutil.copy2(binary, copied)
            inspection = execute("artifact-inspection", [str(readelf), "-h", "-l", "-d", str(copied)], timeout=30)
            header = (output / inspection["stdout"]).read_text()
            if inspection["status"] != "passed" or "Advanced Micro Devices X86-64" not in header:
                raise ValueError("could not verify the linked artifact's x86_64 architecture")
            artifact = {
                "path": str(copied.relative_to(output)),
                "sha256": hashlib.sha256(copied.read_bytes()).hexdigest(),
                "architecture": "x86_64",
                "inspection": inspection,
            }
        except (OSError, ValueError) as error:
            stages["link"].update(status="infra_error", reason=str(error))

source_commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True))
# Preserve fixture inputs for reproducing dirty local experiments as well as CI runs.
for name in ("Cargo.toml", "Cargo.lock", "src/main.rs"):
    destination = output / "fixture" / name
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(FIXTURE / name, destination)
metadata = execute("cargo-metadata", ["cargo", "metadata", "--locked", "--format-version", "1"], timeout=120)
rust = execute("rustc-version", ["rustc", "-Vv"], timeout=30)
cargo = execute("cargo-version", ["cargo", "--version"], timeout=30)
if rust["status"] == "passed" and not (output / rust["stdout"]).read_text().startswith(f"rustc {inputs['rust_version']} "):
    # The first experiment may reveal a bad pin; preserve evidence and fail the run.
    rust.update(status="infra_error", reason="Rust version differs from the experiment pin")
report = {
    "format": "ohos-pulse-feasibility-1",
    "note": "Experimental evidence; not the proposed CLI result-v1 schema.",
    "timestamp_utc": datetime.now(timezone.utc).isoformat(),
    "source_commit": source_commit,
    "source_dirty": dirty,
    "host": platform.platform(),
    "ci_run_id": os.environ.get("GITHUB_RUN_ID"),
    "ci_run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
    "inputs": inputs,
    "subject": {"fixture_id": "libc-file-io", "scenario_id": "file-round-trip", "source": "crates.io"},
    "sdk_and_tools": environment,
    "preflight": preflight,
    "metadata": metadata,
    "rust": rust,
    "cargo": cargo,
    "stages": stages,
    "artifact": artifact,
}
temporary = output / "experiment.json.tmp"
temporary.write_text(json.dumps(report, indent=2) + "\n")
temporary.replace(output / "experiment.json")
lines = [
    "# libc file I/O — OHOS cross-build experiment", "",
    f"Source commit: `{source_commit}`; dirty checkout: `{dirty}`.", "",
    "This is build evidence for one scenario, not crate-wide compatibility.", "",
    "| Stage | Status |", "|---|---|",
]
for stage in ("check", "link", "runtime", "integration"):
    lines.append(f"| {stage} | {stages[stage]['status']} |")
if environment:
    sdk = environment.get("sdk", {})
    lines.extend(["", f"SDK identity: `{sdk.get('version')}`; API: `{sdk.get('api_version')}`."])
lines.extend(["", "Inspect experiment.json and stage logs for commands, reasons, and exact environment metadata.",
              "CI artifacts are retained for 14 days; this is not a durable history archive."])
(output / "summary.md").write_text("\n".join(lines) + "\n")
# Intermediate Cargo objects are not diagnostic artifacts and can be very large.
shutil.rmtree(output / "build", ignore_errors=True)
print("\n".join(lines))
results = [preflight, metadata, rust, cargo, stages["check"], stages["link"]]
sys.exit(0 if all(result["status"] == "passed" for result in results) else 1)
