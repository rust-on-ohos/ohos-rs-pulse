"""Exercise the host fixture contract; these are not OHOS runtime results."""

import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "fixtures/libc-file-io/target/debug/libc-file-io"


def invoke(directory, run_id):
    return subprocess.run(
        [str(BINARY), str(directory), run_id],
        capture_output=True,
        text=True,
        timeout=10,
    )


with tempfile.TemporaryDirectory() as directory:
    result = invoke(directory, "host-contract")
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout) == {
        "scenario_id": "file-round-trip",
        "run_id": "host-contract",
        "status": "passed",
    }
    assert not list(Path(directory).iterdir()), "successful run left a file behind"

    existing = Path(directory) / "ohos-pulse-collision.tmp"
    existing.write_bytes(b"preserve existing content")
    result = invoke(directory, "collision")
    assert result.returncode != 0, "exclusive creation did not reject collision"
    assert not result.stdout, "failure emitted a completion record"
    assert existing.read_bytes() == b"preserve existing content"

    for test_directory, run_id in (
        (directory, "../invalid"),
        (directory, ""),
        (Path(directory) / "missing", "missing-directory"),
    ):
        result = invoke(test_directory, run_id)
        assert result.returncode != 0, "invalid invocation unexpectedly passed"
        assert not result.stdout, "failure emitted a completion record"

print("Host fixture contract verified; no OHOS runtime conclusion.")
