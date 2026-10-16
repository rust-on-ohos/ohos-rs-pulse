# Experimental upstream libc patches

## 0.2.190-kernel-version-code.patch

Status: local candidate only; not automatically applied by the CLI or workflows.
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
Any future automated patched experiment must record patch identity and retain
unmodified results; it must not silently replace upstream-source evidence.
