"""Run the reviewed fixture through hdc; this is not the future CLI protocol."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import signal
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]


def completion_matches(stdout, run_id):
    try:
        records = [json.loads(line) for line in stdout.splitlines() if line.strip()]
        return records == [{"scenario_id": "file-round-trip", "run_id": run_id, "status": "passed"}]
    except (ValueError, TypeError):
        return False


class InfrastructureError(Exception):
    pass


class Experiment:
    def __init__(self, output, build, config):
        self.output = output
        self.build = build
        self.config = config
        self.commands = []
        self.run_id = "pulse-" + uuid.uuid4().hex
        self.directory = config["guest_base"] + "/" + self.run_id
        self.result = {
            "format": "ohos-pulse-runtime-feasibility-1",
            "timestamp_utc": datetime.now(timezone.utc).isoformat(),
            "run_id": self.run_id,
            "ci_run_id": os.environ.get("GITHUB_RUN_ID"),
            "ci_run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
            "profile": config,
            "build_evidence": os.path.relpath(build / "experiment.json", output),
            "runtime": {"status": "infra_error", "reason": "runtime setup did not complete"},
            "commands": self.commands,
            "sdk_provenance": "SDK archive attestation/digest verification remains unresolved; version/API are checked.",
        }

    def command(self, name, args, timeout=None):
        started = time.monotonic()
        record = {"name": name, "command": args, "exit_code": None,
                  "stdout": name + ".stdout.log", "stderr": name + ".stderr.log"}
        self.commands.append(record)
        with (self.output / record["stdout"]).open("wb") as stdout, (self.output / record["stderr"]).open("wb") as stderr:
            try:
                process = subprocess.Popen(args, stdout=stdout, stderr=stderr, start_new_session=True)
                try:
                    record["exit_code"] = process.wait(timeout=timeout or self.config["transport_timeout_seconds"])
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                    record["timed_out"] = True
                    raise InfrastructureError(name + " exceeded transport/execution budget")
            except OSError as error:
                raise InfrastructureError(str(error)) from error
            finally:
                record["elapsed_ms"] = round((time.monotonic() - started) * 1000)
        if record["exit_code"] != 0:
            raise InfrastructureError(name + " transport/host command failed")
        return (self.output / record["stdout"]).read_text(errors="replace")

    def hdc(self, name, *args, timeout=None):
        return self.command(name, ["hdc", "-t", self.config["connect_key"], *args], timeout)

    def shell(self, name, script, timeout=None):
        # hdc shell does not reliably forward a remote exit code. Require a fresh marker.
        marker = "PULSE-SHELL-" + uuid.uuid4().hex
        wrapped = "sh -c " + shlex.quote("( " + script + " ); rc=$?; printf '\\n" + marker + ":%s\\n' \"$rc\"")
        text = self.hdc(name, "shell", wrapped, timeout=timeout)
        match = re.search(r"(?:^|\n)" + marker + r":([0-9]+)\s*$", text)
        if not match or int(match[1]) != 0:
            detail = "status marker missing" if not match else "guest exit code " + match[1]
            raise InfrastructureError(name + ": " + detail + "; inspect " + name + ".stdout.log and " + name + ".stderr.log")
        return text[:match.start()].strip()

    def receive(self, remote, local):
        self.hdc("receive-" + local, "file", "recv", remote, str(self.output / local))
        if not (self.output / local).is_file():
            raise InfrastructureError("hdc did not produce " + local)

    def run(self):
        created = False
        try:
            report = json.loads((self.build / "experiment.json").read_text())
            self.result["source_commit"] = report["source_commit"]
            self.result["source_dirty"] = report["source_dirty"]
            if report["stages"]["link"]["status"] != "passed":
                self.result["runtime"] = {"status": "blocked", "reason": "link did not pass"}
                return
            artifact = report["artifact"]
            binary = self.build / artifact["path"]
            if hashlib.sha256(binary.read_bytes()).hexdigest() != artifact["sha256"]:
                raise InfrastructureError("build artifact digest mismatch")
            self.result["artifact"] = artifact
            self.command("hdc-version", ["hdc", "-v"])
            self.command("targets", ["hdc", "list", "targets"])
            self.result["guest_identity"] = self.shell("guest-identity", "uname -a; id; param get const.ohos.fullname; param get const.product.software.version")
            self.shell("guest-paths", "for p in /lib /lib64 /system/lib /system/lib64 /data/local /data/local/tmp; do echo \"PATH: $p\"; ls -ld \"$p\"; done; "
                       "find /lib /lib64 /system/lib /system/lib64 -name libc.so -o -name 'ld-musl*' 2>/dev/null; true")
            self.shell("guest-loader", "test -r /lib/ld-musl-x86_64.so.1 || { echo 'Required ELF interpreter is unavailable' >&2; exit 1; }")
            base = shlex.quote(self.config["guest_base"])
            self.shell("guest-writable-base", "mkdir -p " + base + " && test -d " + base + " && test -w " + base)
            # A deliberately nonzero child proves that the remote status channel works.
            probe = self.shell("exit-probe", "sh -c 'exit 37'; rc=$?; test \"$rc\" -eq 37 && echo PULSE-EXIT-37")
            if probe != "PULSE-EXIT-37":
                raise InfrastructureError("guest exit-status probe failed")
            directory = shlex.quote(self.directory)
            self.shell("mkdir", "umask 077; mkdir " + directory)
            created = True
            self.hdc("deploy", "file", "send", str(binary), self.directory + "/fixture")
            self.shell("chmod", "chmod 700 " + directory + "/fixture")
            # Resolve NEEDED entries through the guest loader instead of assuming a libc path.
            self.result["guest_dependencies"] = self.shell("guest-dependencies", "/lib/ld-musl-x86_64.so.1 --list " + directory + "/fixture")
            # Redirect inside the guest to preserve stdout/stderr separately. Store the
            # actual child status rather than trusting hdc's process return code.
            script = (f"cd {directory} || exit 1; ./fixture . {shlex.quote(self.run_id)} >stdout 2>stderr & "
                      "pid=$!; printf '%s\\n' \"$pid\" >pid; wait \"$pid\"; "
                      "rc=$?; rm -f pid; printf '%s\\n' \"$rc\" >exit-code")
            try:
                self.shell("execute", script, timeout=self.config["execution_timeout_seconds"])
            except InfrastructureError:
                if self.commands[-1].get("timed_out"):
                    self.result["runtime"] = {"status": "timed_out", "reason": "guest execution or its transport exceeded the execution budget"}
                raise
            for name in ("stdout", "stderr", "exit-code"):
                self.receive(self.directory + "/" + name, "guest-" + name + ".log")
            raw = (self.output / "guest-exit-code.log").read_text().strip()
            if not re.fullmatch(r"[0-9]{1,3}", raw) or int(raw) > 255:
                raise InfrastructureError("missing or invalid guest exit status")
            code = int(raw)
            matched = completion_matches((self.output / "guest-stdout.log").read_text(), self.run_id)
            self.result["runtime"] = {
                "status": "passed" if code == 0 and matched else "failed",
                "guest_exit_code": code, "completion_matches": matched,
                "stdout": "guest-stdout.log", "stderr": "guest-stderr.log",
            }
            if code != 0 or not matched:
                self.result["runtime"]["reason"] = "guest exit status or completion record failed acceptance"
            self.shell("fixture-cleanup-check", "test ! -e " + directory + "/ohos-pulse-" + self.run_id + ".tmp")
        except (InfrastructureError, OSError, ValueError, KeyError) as error:
            if self.result["runtime"]["status"] not in ("failed", "timed_out"):
                self.result["runtime"].update(status="infra_error", reason=str(error))
            self.result.setdefault("diagnostics", []).append(str(error))
        finally:
            if created:
                try:
                    # Remove only files owned by this unique invocation; rmdir reveals leftovers.
                    self.shell("cleanup", "cd " + shlex.quote(self.directory)
                               + " && { if test -f pid; then kill -9 \"$(cat pid)\" || exit 1; fi; }"
                               + " && rm -f fixture stdout stderr exit-code pid && cd / && rmdir "
                               + shlex.quote(self.directory))
                    self.result["cleanup"] = {"status": "passed"}
                except InfrastructureError as error:
                    self.result["cleanup"] = {"status": "infra_error", "reason": str(error)}
                    if self.result["runtime"]["status"] == "passed":
                        self.result["runtime"].update(status="infra_error", reason="required guest cleanup failed")
            else:
                self.result["cleanup"] = {"status": "not_run", "reason": "unique guest directory was not created"}
            temporary = self.output / "runtime.json.tmp"
            temporary.write_text(json.dumps(self.result, indent=2) + "\n")
            temporary.replace(self.output / "runtime.json")
            summary = ("# Oniro runtime feasibility\n\n"
                       f"Runtime: `{self.result['runtime']['status']}`; run ID: `{self.run_id}`.\n\n"
                       f"Reason: {self.result['runtime'].get('reason', 'acceptance criteria satisfied')}.\n\n"
                       "Scope: libc file-round-trip in the recorded Oniro guest; no ArkTS integration.\n\n"
                       "See runtime.json and command logs for status, identity, cleanup and diagnostics.\n\n"
                       "SDK archive provenance verification is unresolved. Evidence retention: 14 days.\n")
            (self.output / "summary.md").write_text(summary)
            print(summary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    config = json.loads((ROOT / "config/runtime.json").read_text())
    experiment = Experiment(args.output.resolve(), args.build.resolve(), config)
    experiment.run()
    return 0 if experiment.result["runtime"]["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
