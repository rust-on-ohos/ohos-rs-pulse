# ohos-rs-pulse

The project collects reproducible evidence for specific Rust usage scenarios on OpenHarmony. The initial scope is one upstream libc file I/O scenario, a small `ohos-pulse` CLI, and GitHub Actions workflows.

## Reading order

1. [High-Level Design](high-level-design.md): project goals, evidence boundaries, architecture, and expansion principles.
2. [MVP overview](mvp/README.md): the current delivery scope and design entry point.
3. [MVP design](mvp/design.md): fixture behavior, CLI contracts, execution, evidence, CI, and acceptance criteria.
4. [libc upstream test experiment](experiments/libc-upstream-tests.md): proposed reuse of pinned upstream ABI tests in the existing OH guest.
5. [Upstream experiment usage](experiments/libc-upstream-usage.md): Rust adapter, manual workflow and result interpretation.

The HLD provides the broader direction; the MVP design defines the initial implementation scope. Additional detailed designs will be written as needed, based on actual experiment results.

Commands and interfaces are proposals until implemented. Dependency/tool versions and runtime feasibility must be established through real experiments; these documents do not claim verified compatibility.
