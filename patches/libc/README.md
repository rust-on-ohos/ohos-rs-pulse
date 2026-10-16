# Experimental upstream libc patches

## 0.2.190-kernel-version-code.patch

Status: candidate; selected explicitly by `upstream-build --profile kernel-probe`.
The default `baseline` profile remains unmodified.
Base commit: 7b0ab5528dc7f361f3a0b4c06c2cad9971617f75 (libc 0.2.190).

The upstream kernel-header version detector reads LINUX_VERSION_MAJOR and
LINUX_VERSION_PATCHLEVEL. OHOS SDK 6.1.0.31 / API 23's macOS package instead
provides LINUX_VERSION_CODE=330240. This patch falls back to decoding major/minor
from that header macro when no version was found. It does not use host/guest uname,
invent a version, change libc bindings, modify ABI assertions or add skip rules.

A local build passed version detection but still failed during C compilation;
see [experiment findings](../../docs/experiments/libc-upstream-findings.md).
Candidate builds used a separate Git worktree. The current CLI intentionally
rejects this dirty source as a normal baseline.
Patched CLI builds use isolated source copies and retain the exact patch, digest
and adapted build script. They do not replace unmodified results.

## Bounded ABI profiles

- `0.2.190-ohos-sdk23-core.patch`: `--profile sdk23-core`, x86_64 / API 23.
- `0.2.190-ohos-sdk26-arm64-core.patch`: `--profile sdk26-arm64-core`, ARM64 / API 26.

Both use the same reviewed base commit and include the version-detector fallback.
Each adds explicit allowlists for basic file, memory, time and IPv4 socket ABI
checks using the original upstream ctest generator and completion harness. All
other APIs, unions and statics are outside the bounded scope. These patches do
not fix the full target's missing declarations or header conflicts and do not
change libc bindings. The CLI pins each patch digest and rejects mismatched
target/API profiles. See the [ARM64 recipe](../../docs/experiments/libc-upstream-arm64.md).
