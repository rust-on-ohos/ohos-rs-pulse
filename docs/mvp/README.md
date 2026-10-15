# ohos-rs Pulse — MVP

The MVP establishes one reproducible evidence workflow for one libc file I/O
scenario on one OpenHarmony target. The Rust CLI now owns the execution path;
GitHub Actions provisions tools/SDK and the emulator. Python has been removed.

Read the [CLI usage](../../README.md), [implemented result protocol](result-protocol.md)
and [runtime experiment](runtime-experiment.md). The [MVP design](design.md)
remains the broader design reference; its initial TOML and standalone JSON Schema
proposals have not been implemented. The CLI currently consumes the existing
typed JSON build/runtime profiles and validates result semantics in Rust.

Historical Python experiments established real hosted check/link and one runtime
success. Rust tests validate orchestration locally; new hosted runtime attempts
must establish replacement behavior and repeatability. ArkTS, PostgreSQL and
SDK archive provenance verification remain deferred/open as documented.
