//! Version-specific adapter for an upstream custom harness, separate from result-v1.
use crate::{
    build::digest,
    config::{self, BuildConfig},
    process::{Executor, args},
    report::{
        Artifact, CommandRecord, RunIdentity, RuntimeEnvironment, Stage, Status, safe_relative,
    },
    runtime::{Guest, quote},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub repository: String,
    pub version: String,
    pub commit: String,
    pub lockfile_sha256: String,
    pub package: String,
    pub test: String,
    pub generator_version: String,
    pub generator_template_sha256: String,
    pub build_timeout_seconds: u64,
    pub execution_timeout_seconds: u64,
}

pub fn load_config(root: &Path) -> Result<Config> {
    let config: Config =
        serde_json::from_slice(&fs::read(root.join("config/upstream-libc.json"))?)?;
    ensure!(
        config.repository == "rust-lang/libc"
            && config.package == "libc-test"
            && config.test == "ctest",
        "only the reviewed upstream ctest target is supported"
    );
    ensure!(
        config.commit.len() == 40 && config.commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid upstream commit"
    );
    ensure!(
        config.generator_version == "0.5.1"
            && config.generator_template_sha256 == REVIEWED_TEMPLATE_SHA,
        "unreviewed ctest harness; adapter review required"
    );
    ensure!(
        (1..=1800).contains(&config.build_timeout_seconds)
            && (1..=600).contains(&config.execution_timeout_seconds),
        "invalid experiment budgets"
    );
    Ok(config)
}
const REVIEWED_TEMPLATE_SHA: &str =
    "898fd73091c473df2529aaeaf073b9ebf7bc510a4c36c1ff1652420e9de229fb";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum BuildProfile {
    Baseline,
    KernelProbe,
    Sdk23Core,
    Sdk26Arm64Core,
}

impl BuildProfile {
    fn recipe(self) -> Option<(&'static str, &'static str, &'static str)> {
        match self {
            Self::Baseline => None,
            Self::KernelProbe => Some((
                "patches/libc/0.2.190-kernel-version-code.patch",
                "27336208211ced15e3c6d35471e181995aec071184bc80ca1196b7bb09416c43",
                "Full upstream ctest target with only kernel-header version detection adapted.",
            )),
            Self::Sdk23Core => Some((
                "patches/libc/0.2.190-ohos-sdk23-core.patch",
                "9b10e6cbac89de30502d471f725367930e9f3e12512547ced12d907045983ddb",
                "Bounded file, memory, time and IPv4 socket ABI allowlist; all other APIs excluded. This is not full upstream ctest coverage or behavioral API testing.",
            )),
            Self::Sdk26Arm64Core => Some((
                "patches/libc/0.2.190-ohos-sdk26-arm64-core.patch",
                "d97f372e81eea5f35f53faa657f0ccdd0f583420f42aea87fc9da666c904fefc",
                "Bounded ARM64 file, memory, time and IPv4 socket ABI allowlist for SDK API 26; all other APIs excluded. This is not full upstream ctest coverage or behavioral API testing.",
            )),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Adaptation {
    pub profile: BuildProfile,
    pub patch: String,
    pub patch_sha256: String,
    pub build_script: String,
    pub build_script_sha256: String,
    pub scope: String,
}
// This exact main body comes from the Cargo.lock-selected ctest 0.5.1 template.
const REVIEWED_MAIN: &str = r#"fn main() {
    println!("RUNNING ALL TESTS");
    run_all();
    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        panic!("some tests failed");
    } else {
        println!(
            "PASSED {} tests",
            NTESTS.load(std::sync::atomic::Ordering::Relaxed)
        );
    }
}"#;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Coverage {
    pub generated_functions: Vec<String>,
    pub generated_function_count: usize,
    pub successful_comparisons: Option<u64>,
    pub expected_comparisons: Option<u64>,
    pub skip_count: Option<u64>,
    pub exclusions_note: String,
    pub generated_rust: String,
    pub generated_rust_sha256: String,
    pub generated_c: String,
    pub generated_c_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Experiment {
    pub format: String,
    pub run: RunIdentity,
    pub driver_source_commit: Option<String>,
    pub driver_source_dirty: Option<bool>,
    pub upstream: Config,
    #[serde(default)]
    pub adaptation: Option<Adaptation>,
    pub build_pins: BuildConfig,
    pub lockfile_sha256: Option<String>,
    pub resolved_features: Vec<String>,
    pub generator: Option<Value>,
    pub environment: Option<Value>,
    pub rustc: Option<String>,
    pub phases: BTreeMap<String, Stage>,
    pub artifact: Option<Artifact>,
    pub coverage: Option<Coverage>,
    pub runtime_environment: Option<RuntimeEnvironment>,
    pub cleanup: Stage,
    pub commands: Vec<CommandRecord>,
    pub diagnostics: Vec<String>,
    pub sdk_provenance: String,
}
impl Experiment {
    fn new(config: Config, pins: BuildConfig) -> Self {
        let mut phases = BTreeMap::new();
        phases.insert(
            "preparation".into(),
            Stage::new(Status::InfraError, "preparation did not complete"),
        );
        phases.insert(
            "test_build".into(),
            Stage::new(Status::Blocked, "preparation did not pass"),
        );
        phases.insert(
            "inspection".into(),
            Stage::new(Status::Blocked, "test build did not pass"),
        );
        phases.insert(
            "execution".into(),
            Stage::new(Status::NotRun, "build-only experiment"),
        );
        Self {
            format: "ohos-pulse-upstream-libc-experiment-1".into(),
            driver_source_commit: None,
            driver_source_dirty: None,
            run: RunIdentity { id: format!("pulse-{}", uuid::Uuid::new_v4().simple()), timestamp_utc: chrono::Utc::now().to_rfc3339(), cli_version: env!("CARGO_PKG_VERSION").into(), source_commit: config.commit.clone(), source_dirty: false, ci_run_id: std::env::var("GITHUB_RUN_ID").ok(), ci_run_attempt: std::env::var("GITHUB_RUN_ATTEMPT").ok() },
            upstream: config, adaptation: None, build_pins: pins, lockfile_sha256: None, resolved_features: vec![], generator: None, environment: None, rustc: None, phases, artifact: None, coverage: None,
            runtime_environment: None, cleanup: Stage::new(Status::NotRun, "guest directory was not created"), commands: vec![], diagnostics: vec![],
            sdk_provenance: "SDK archive digest/attestation acceptance remains unresolved; installed version/API are checked.".into(),
        }
    }
    pub fn exit_code(&self) -> i32 {
        if self.phases.values().any(|p| p.status == Status::InfraError)
            || self.cleanup.status == Status::InfraError
        {
            2
        } else if self
            .phases
            .values()
            .any(|p| !matches!(p.status, Status::Passed | Status::NotRun))
        {
            1
        } else {
            0
        }
    }
    fn validate(&self) -> Result<()> {
        if let Some(adaptation) = &self.adaptation {
            let (_, expected, scope) = adaptation
                .profile
                .recipe()
                .ok_or_else(|| anyhow::anyhow!("baseline cannot have an adaptation"))?;
            ensure!(
                adaptation.patch_sha256 == expected
                    && adaptation.scope == scope
                    && safe_relative(&adaptation.patch)
                    && safe_relative(&adaptation.build_script)
                    && self.run.source_dirty,
                "invalid adaptation identity"
            );
        }
        ensure!(
            self.format == "ohos-pulse-upstream-libc-experiment-1",
            "unsupported experiment format"
        );
        for name in ["preparation", "test_build", "inspection", "execution"] {
            let phase = self
                .phases
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("missing {name}"))?;
            ensure!(
                phase.status == Status::Passed
                    || phase.reason.as_ref().is_some_and(|r| !r.is_empty()),
                "missing phase reason"
            );
        }
        ensure!(self.phases.len() == 4, "unexpected phase names");
        ensure!(
            self.upstream.generator_version == "0.5.1"
                && self.upstream.generator_template_sha256 == REVIEWED_TEMPLATE_SHA,
            "unreviewed harness profile"
        );
        if self.phases["test_build"].status == Status::Passed {
            ensure!(
                self.phases["preparation"].status == Status::Passed
                    && self.phases["test_build"]
                        .command
                        .as_ref()
                        .is_some_and(|c| c.status == Status::Passed && c.exit_code == Some(0)),
                "missing successful build evidence"
            );
        }
        for record in &self.commands {
            ensure!(
                safe_relative(&record.stdout) && safe_relative(&record.stderr),
                "unsafe log path"
            );
        }
        if let Some(artifact) = &self.artifact {
            ensure!(safe_relative(&artifact.path), "unsafe artifact path");
            ensure!(
                artifact.sha256.len() == 64
                    && artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid artifact digest"
            );
        }
        if let Some(coverage) = &self.coverage {
            ensure!(
                safe_relative(&coverage.generated_rust) && safe_relative(&coverage.generated_c),
                "unsafe generated-source path"
            );
            ensure!(
                coverage.generated_function_count > 0
                    && coverage.generated_function_count == coverage.generated_functions.len(),
                "empty/inconsistent inventory"
            );
        }
        if self.phases["inspection"].status == Status::Passed {
            ensure!(
                self.artifact.is_some() && self.coverage.is_some(),
                "missing inspection evidence"
            );
        }
        if self.phases["execution"].status == Status::Passed {
            let phase = &self.phases["execution"];
            ensure!(
                self.phases["test_build"].status == Status::Passed
                    && self.phases["inspection"].status == Status::Passed
                    && self.cleanup.status == Status::Passed,
                "passing execution without predecessors/cleanup"
            );
            ensure!(
                phase.guest_exit_code == Some(0)
                    && phase.completion_matches == Some(true)
                    && self
                        .coverage
                        .as_ref()
                        .is_some_and(|c| c.successful_comparisons.is_some_and(|n| n > 0)),
                "passing execution lacks nonempty harness evidence"
            );
            ensure!(
                phase
                    .command
                    .as_ref()
                    .is_some_and(|c| c.status == Status::Passed && c.exit_code == Some(0))
                    && phase.stdout.is_some()
                    && phase.stderr.is_some(),
                "missing execution command/output evidence"
            );
            ensure!(
                self.cleanup
                    .command
                    .as_ref()
                    .is_some_and(|c| c.status == Status::Passed && c.exit_code == Some(0)),
                "missing cleanup command evidence"
            );
            ensure!(
                self.runtime_environment
                    .as_ref()
                    .is_some_and(|e| e.guest_identity.is_some() && e.guest_dependencies.is_some()),
                "missing observed guest evidence"
            );
        }
        Ok(())
    }
    fn write(&self, output: &Path) -> Result<()> {
        self.validate()?;
        fs::write(
            output.join("upstream.json.tmp"),
            serde_json::to_vec_pretty(self)?,
        )?;
        fs::rename(
            output.join("upstream.json.tmp"),
            output.join("upstream.json"),
        )?;
        let saved: Self = serde_json::from_slice(&fs::read(output.join("upstream.json"))?)?;
        let escape = |s: &str| {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('|', "&#124;")
                .replace('`', "&#96;")
                .replace(['\n', '\r'], " ")
        };
        let mut summary = format!(
            "# Upstream libc ctest experiment\n\nlibc {} at `{}`; invocation `{}`.\n\nScope: the selected upstream ABI target in the recorded environment; no crate-wide or Rust tier claim.\n\n| Phase | Status | Reason |\n|---|---|---|\n",
            escape(&saved.upstream.version),
            escape(&saved.upstream.commit),
            escape(&saved.run.id)
        );
        for name in ["preparation", "test_build", "inspection", "execution"] {
            let phase = &saved.phases[name];
            summary.push_str(&format!(
                "| {} | {} | {} |\n",
                escape(name),
                phase.status,
                escape(phase.reason.as_deref().unwrap_or(""))
            ));
        }
        if let Some(adaptation) = &saved.adaptation {
            summary.push_str(&format!(
                "\nAdaptation: `{:?}`; patch SHA-256 `{}`.\n\n{}\n",
                adaptation.profile,
                adaptation.patch_sha256,
                escape(&adaptation.scope)
            ));
        }
        if let Some(c) = &saved.coverage {
            let comparisons = c
                .successful_comparisons
                .map(|n| n.to_string())
                .unwrap_or_else(|| "unavailable".into());
            summary.push_str(&format!("\nGenerated test functions: {}. Successful comparisons reported by harness: {}. These are different units; total excluded checks is unknown.\n", c.generated_function_count, comparisons));
        }
        summary.push_str(&format!(
            "\nCleanup: {}.\n\n{}\n",
            saved.cleanup.status,
            escape(&saved.sdk_provenance)
        ));
        for diagnostic in &saved.diagnostics {
            summary.push_str(&format!("\n- {}\n", escape(diagnostic)));
        }
        summary.push_str("\nSee upstream.json, generated sources, source archive and logs. Expected image profile is not local image attestation; CI retains archive verification. CI artifacts expire after 14 days.\n");
        fs::write(output.join("summary.md"), &summary)?;
        print!("{summary}");
        Ok(())
    }
}

fn required(executor: &mut Executor, name: &str, values: &[&str], seconds: u64) -> Result<String> {
    let command = executor.execute(name, &args(values), Duration::from_secs(seconds))?;
    ensure!(
        command.status == Status::Passed,
        "{name}: {}; inspect {} and {}",
        command.status,
        command.stdout,
        command.stderr
    );
    executor.stdout(&command)
}

pub fn build(
    root: &Path,
    source: &Path,
    output: &Path,
    config: Config,
    profile: BuildProfile,
    build_config: Option<&Path>,
) -> Result<i32> {
    ensure!(
        !output.starts_with(source) && !source.starts_with(output),
        "source/output directories overlap"
    );
    let pins = match build_config {
        Some(path) => serde_json::from_slice::<BuildConfig>(&fs::read(path)?)?,
        None => config::load_build(root)?,
    };
    let (architecture, elf_machine, loader, _) = target_details(&pins.target)?;
    if profile == BuildProfile::Sdk23Core {
        ensure!(
            pins.target == "x86_64-unknown-linux-ohos" && pins.sdk_api_version == 23,
            "sdk23-core requires x86_64 / API 23"
        );
    }
    if profile == BuildProfile::Sdk26Arm64Core {
        ensure!(
            pins.target == "aarch64-unknown-linux-ohos" && pins.sdk_api_version == 26,
            "sdk26-arm64-core requires ARM64 / API 26"
        );
    }
    let mut report = Experiment::new(config.clone(), pins.clone());
    let mut executor = Executor::new(output, source)?;
    fs::create_dir(output.join("config"))?;
    fs::copy(
        root.join("config/upstream-libc.json"),
        output.join("config/upstream-libc.json"),
    )?;
    fs::write(
        output.join("config/feasibility.json"),
        serde_json::to_vec_pretty(&pins)?,
    )?;
    let mut phase = "preparation";
    let outcome = (|| -> Result<()> {
        executor.cwd = root.to_owned();
        report.driver_source_commit = Some(
            required(
                &mut executor,
                "driver-commit",
                &["git", "rev-parse", "HEAD"],
                30,
            )?
            .trim()
            .into(),
        );
        report.driver_source_dirty = Some(
            !required(
                &mut executor,
                "driver-status",
                &["git", "status", "--porcelain"],
                30,
            )?
            .trim()
            .is_empty(),
        );
        executor.cwd = source.to_owned();
        let commit = required(
            &mut executor,
            "upstream-commit",
            &["git", "rev-parse", "HEAD"],
            30,
        )?;
        report.run.source_commit = commit.trim().into();
        ensure!(
            commit.trim() == config.commit,
            "upstream checkout differs from reviewed commit"
        );
        let status = required(
            &mut executor,
            "upstream-status",
            &["git", "status", "--porcelain", "--untracked-files=all"],
            30,
        )?;
        report.run.source_dirty = !status.trim().is_empty();
        ensure!(
            status.trim().is_empty(),
            "upstream checkout must be unmodified; preserve a separate patched experiment instead"
        );
        let tracked = required(
            &mut executor,
            "upstream-files",
            &["git", "ls-files", "--stage"],
            30,
        )?;
        ensure!(
            !tracked
                .lines()
                .any(|l| l.starts_with("120000 ") || l.starts_with("160000 ")),
            "upstream symlinks/submodules require review"
        );
        let lock = digest(&source.join("Cargo.lock"))?;
        report.lockfile_sha256 = Some(lock.clone());
        ensure!(
            lock == config.lockfile_sha256,
            "upstream lockfile differs from reviewed digest"
        );
        required(
            &mut executor,
            "upstream-source-archive",
            &[
                "git",
                "archive",
                "--format=tar",
                "--output",
                &output.join("upstream-source.tar").to_string_lossy(),
                "HEAD",
            ],
            60,
        )?;
        if let Some((patch_path, expected_sha, scope)) = profile.recipe() {
            ensure!(
                config.commit == "7b0ab5528dc7f361f3a0b4c06c2cad9971617f75",
                "adaptation requires the reviewed libc 0.2.190 commit"
            );
            ensure!(
                digest(&root.join(patch_path))? == expected_sha,
                "adaptation patch changed; review and update its identity"
            );
            fs::copy(root.join(patch_path), output.join("adaptation.patch"))?;
            let working = output.join("adapted-source");
            fs::create_dir(&working)?;
            required(
                &mut executor,
                "adaptation-extract",
                &[
                    "tar",
                    "-xf",
                    &output.join("upstream-source.tar").to_string_lossy(),
                    "-C",
                    &working.to_string_lossy(),
                ],
                60,
            )?;
            executor.cwd = working;
            // An isolated repository prevents git apply from discovering the driver's parent repository.
            required(
                &mut executor,
                "adaptation-init",
                &["git", "init", "--quiet"],
                30,
            )?;
            required(
                &mut executor,
                "adaptation-check",
                &[
                    "git",
                    "apply",
                    "--check",
                    &output.join("adaptation.patch").to_string_lossy(),
                ],
                30,
            )?;
            required(
                &mut executor,
                "adaptation-apply",
                &[
                    "git",
                    "apply",
                    &output.join("adaptation.patch").to_string_lossy(),
                ],
                30,
            )?;
            let script = "adapted-build-main.rs";
            fs::copy(
                executor.cwd.join("libc-test/build/main.rs"),
                output.join(script),
            )?;
            ensure!(
                digest(&output.join(script))? != digest(&source.join("libc-test/build/main.rs"))?,
                "adaptation did not modify the test configuration"
            );
            report.run.source_dirty = true;
            report.adaptation = Some(Adaptation {
                profile,
                patch: "adaptation.patch".into(),
                patch_sha256: expected_sha.into(),
                build_script: script.into(),
                build_script_sha256: digest(&output.join(script))?,
                scope: scope.into(),
            });
        }
        fs::copy(source.join("Cargo.lock"), output.join("Cargo.lock"))?;
        fs::create_dir(output.join("upstream-test-config"))?;
        for name in [
            "Cargo.toml",
            "build/main.rs",
            "build/target.rs",
            "tests/ctest.rs",
        ] {
            let target = output.join("upstream-test-config").join(name);
            fs::create_dir_all(target.parent().unwrap())?;
            fs::copy(source.join("libc-test").join(name), target)?;
        }
        let rust = required(&mut executor, "upstream-rustc", &["rustc", "-Vv"], 30)?;
        ensure!(
            rust.starts_with(&format!("rustc {} ", pins.rust_version)),
            "Rust version differs from pin"
        );
        report.rustc = Some(rust);
        required(&mut executor, "upstream-cargo", &["cargo", "--version"], 30)?;
        let env_text = required(
            &mut executor,
            "upstream-environment",
            &[
                "cargo",
                "ohos",
                "env",
                "-t",
                &pins.target,
                "--format",
                "json",
            ],
            60,
        )?;
        let environment: Value = serde_json::from_str(&env_text)?;
        report.environment = Some(environment.clone());
        ensure!(
            environment["cargo_ohos_version"] == pins.cargo_ohos_version
                && environment["sdk"]["version"] == pins.sdk_version
                && environment["sdk"]["api_version"] == pins.sdk_api_version,
            "build tool/SDK identity differs from pins"
        );
        verify_c_environment(&environment, &pins)?;
        capture_kernel_probe(&mut executor, &environment, &mut report.diagnostics)?;
        let readelf = environment["toolchain"]["readelf"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("SDK readelf unavailable"))?;
        ensure!(Path::new(readelf).is_file(), "SDK readelf unavailable");
        required(
            &mut executor,
            "upstream-clang",
            &[
                environment["toolchain"]["clang"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("SDK clang unavailable"))?,
                "--version",
            ],
            30,
        )?;
        let metadata = required(
            &mut executor,
            "upstream-metadata",
            &["cargo", "metadata", "--locked", "--format-version", "1"],
            120,
        )?;
        let metadata: Value = serde_json::from_str(&metadata)?;
        let packages = metadata["packages"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("missing package metadata"))?;
        let libc = packages
            .iter()
            .find(|p| p["name"] == "libc" && p["source"].is_null())
            .ok_or_else(|| anyhow::anyhow!("source-tree libc missing"))?;
        ensure!(
            libc["version"] == config.version,
            "source libc version differs from selected release"
        );
        let test_package = packages
            .iter()
            .find(|p| p["name"] == config.package && p["source"].is_null())
            .ok_or_else(|| anyhow::anyhow!("libc-test missing"))?;
        ensure!(
            test_package["targets"]
                .as_array()
                .is_some_and(|t| t.iter().any(|t| t["name"] == config.test
                    && t["kind"]
                        .as_array()
                        .is_some_and(|k| k.iter().any(|k| k == "test")))),
            "selected test target missing"
        );
        let generator = packages
            .iter()
            .find(|p| {
                p["name"] == "ctest"
                    && p["version"] == config.generator_version
                    && p["source"]
                        .as_str()
                        .is_some_and(|s| s.starts_with("registry+"))
            })
            .ok_or_else(|| anyhow::anyhow!("reviewed ctest generator not resolved"))?;
        let template = Path::new(
            generator["manifest_path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("generator manifest path missing"))?,
        )
        .parent()
        .unwrap()
        .join("templates/test.rs");
        ensure!(
            digest(&template)? == config.generator_template_sha256,
            "generator template changed; adapter review required"
        );
        fs::copy(template, output.join("generator-template.rs"))?;
        report.generator = Some(generator.clone());
        let node = metadata["resolve"]["nodes"]
            .as_array()
            .and_then(|n| n.iter().find(|n| n["id"] == libc["id"]))
            .ok_or_else(|| anyhow::anyhow!("libc features unavailable"))?;
        report.resolved_features = serde_json::from_value(node["features"].clone())?;
        report.phases.insert(
            "preparation".into(),
            Stage::new(
                Status::Passed,
                "reviewed source, lockfile, harness and SDK verified",
            ),
        );
        phase = "test_build";
        let target_dir = output.join("target");
        let command = executor.execute(
            "upstream-test-build",
            &args(&[
                "cargo",
                "ohos",
                "test",
                "-t",
                &pins.target,
                "--locked",
                "-p",
                &config.package,
                "--test",
                &config.test,
                "--no-run",
                "--message-format=json",
                "--target-dir",
                &target_dir.to_string_lossy(),
            ]),
            Duration::from_secs(config.build_timeout_seconds),
        )?;
        report
            .phases
            .insert(phase.into(), Stage::executed(command.clone()));
        // Preserve generated sources even when the upstream build script fails in C compilation.
        let generated =
            capture_generated(&target_dir.join(&pins.target).join("debug/build"), output);
        if command.status != Status::Passed {
            let stderr = fs::read_to_string(output.join(&command.stderr))?;
            if stderr.contains("failed to detect kernel version for Linux target") {
                let reason = "upstream libc-test could not detect the SDK header kernel version; inspect logs/upstream-kernel-probe.*.log before proposing an adaptation";
                report.phases.get_mut("test_build").unwrap().reason = Some(reason.into());
                report.diagnostics.push(reason.into());
            }
            if let Err(error) = generated {
                report
                    .diagnostics
                    .push(format!("generated-source capture: {error}"));
            }
            return Ok(());
        }
        phase = "inspection";
        let generated = generated?;
        let rust_source = fs::read_to_string(output.join(&generated.0))?;
        let functions = inventory(&rust_source)?;
        report.coverage = Some(Coverage { generated_function_count: functions.len(), generated_functions: functions, successful_comparisons: None, expected_comparisons: None, skip_count: None, exclusions_note: "Baseline rules are in upstream-source.tar; explicit adaptations and allowlists, when selected, are in adaptation.patch and adapted-build-main.rs. Generated C/Rust record the resulting checks; total excluded checks is unknown. Successful comparisons are not generated-function counts.".into(), generated_rust_sha256: digest(&output.join(&generated.0))?, generated_c_sha256: digest(&output.join(&generated.1))?, generated_rust: generated.0, generated_c: generated.1 });
        let stdout = executor.stdout(&command)?;
        let binary = select_artifact(
            &stdout,
            test_package["id"].as_str().unwrap(),
            &config.test,
            &target_dir.join(&pins.target),
        )?;
        fs::create_dir(output.join("artifacts"))?;
        let copied = output.join("artifacts/ctest");
        fs::copy(binary, &copied)?;
        let record = executor.execute(
            "upstream-artifact-inspection",
            &args(&[readelf, "-h", "-l", "-d", &copied.to_string_lossy()]),
            Duration::from_secs(30),
        )?;
        let header = executor.stdout(&record)?;
        ensure!(
            record.status == Status::Passed
                && header.contains(elf_machine)
                && header.contains(loader),
            "test artifact architecture/interpreter mismatch"
        );
        report.artifact = Some(Artifact {
            path: "artifacts/ctest".into(),
            sha256: digest(&copied)?,
            architecture: architecture.into(),
            inspection: record.clone(),
        });
        report.phases.insert(phase.into(), Stage::executed(record));
        Ok(())
    })();
    if let Err(error) = outcome {
        report.phases.insert(
            phase.into(),
            Stage::new(Status::InfraError, &error.to_string()),
        );
        report.diagnostics.push(error.to_string());
    }
    report.commands = executor.records;
    // Retain generated sources/logs rather than large intermediate object directories.
    if output.join("target").exists() {
        fs::remove_dir_all(output.join("target"))?;
    }
    if output.join("adapted-source").exists() {
        fs::remove_dir_all(output.join("adapted-source"))?;
    }
    report.write(output)?;
    Ok(report.exit_code())
}

fn capture_kernel_probe(
    executor: &mut Executor,
    environment: &Value,
    diagnostics: &mut Vec<String>,
) -> Result<()> {
    // Mirror the Linux portion of upstream's version probe using only target SDK headers.
    let probe = executor.output.join("kernel-probe.c");
    fs::write(
        &probe,
        "#if defined(__linux__) && defined(__has_include)\n#if __has_include(<linux/version.h>)\n#define PULSE_HAS_LINUX_VERSION_HEADER 1\n#include <linux/version.h>\n#else\n#define PULSE_HAS_LINUX_VERSION_HEADER 0\n#endif\n#endif\n#include <stdio.h>\n",
    )?;
    let clang = environment["toolchain"]["clang"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("SDK clang missing"))?;
    let mut command = vec![clang.to_owned()];
    let flags: Vec<String> = serde_json::from_value(environment["flags"]["cflags"].clone())?;
    command.extend(flags);
    command.extend(args(&["-x", "c", "-dM", "-E", &probe.to_string_lossy()]));
    let record = executor.execute("upstream-kernel-probe", &command, Duration::from_secs(30))?;
    if record.status != Status::Passed {
        diagnostics.push(format!(
            "SDK header diagnostic probe {}; upstream test-build will still be attempted",
            record.status
        ));
    }
    // Never inject version macros or use host uname as a substitute for SDK-header identity.
    Ok(())
}

fn capture_generated(directory: &Path, output: &Path) -> Result<(String, String)> {
    let mut candidates = vec![];
    if directory.is_dir() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("libc-test-")
                && entry.file_type()?.is_dir()
            {
                let out = entry.path().join("out");
                if out.join("ctest_output.rs").is_file() {
                    candidates.push(out);
                }
            }
        }
    }
    ensure!(
        candidates.len() == 1,
        "expected one generated ctest output; found {}",
        candidates.len()
    );
    let generated = output.join("generated");
    fs::create_dir(&generated)?;
    for entry in fs::read_dir(&candidates[0])? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e == "rs" || e == "c" || e == "h")
        {
            fs::copy(entry.path(), generated.join(entry.file_name()))?;
        }
    }
    ensure!(
        generated.join("ctest_output.c").is_file(),
        "generated C counterpart missing"
    );
    Ok((
        "generated/ctest_output.rs".into(),
        "generated/ctest_output.c".into(),
    ))
}

pub fn inventory(source: &str) -> Result<Vec<String>> {
    let compact = |s: &str| {
        s.chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect::<String>()
    };
    let main_start = source
        .find("fn main() {")
        .ok_or_else(|| anyhow::anyhow!("ctest main missing"))?;
    let main_end = source[main_start..]
        .find("\n}")
        .ok_or_else(|| anyhow::anyhow!("ctest main terminator missing"))?
        + main_start
        + 2;
    ensure!(
        compact(&source[main_start..main_end]) == compact(REVIEWED_MAIN),
        "ctest completion semantics changed; adapter review required"
    );
    let start = source
        .find("fn run_all() {")
        .ok_or_else(|| anyhow::anyhow!("ctest run_all missing"))?
        + "fn run_all() {".len();
    let end = source[start..]
        .find('}')
        .ok_or_else(|| anyhow::anyhow!("ctest run_all terminator missing"))?
        + start;
    let mut functions = vec![];
    for line in source[start..end]
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        let name = line
            .strip_suffix("();")
            .ok_or_else(|| anyhow::anyhow!("unexpected ctest inventory statement"))?;
        ensure!(
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "invalid generated function name"
        );
        ensure!(
            source.contains(&format!("pub fn {name}()")),
            "inventory function definition missing"
        );
        ensure!(
            !functions.iter().any(|f| f == name),
            "duplicate generated function"
        );
        functions.push(name.to_owned());
    }
    ensure!(!functions.is_empty(), "empty upstream test inventory");
    Ok(functions)
}

pub fn select_artifact(stdout: &str, package: &str, test: &str, target: &Path) -> Result<PathBuf> {
    let mut candidates = vec![];
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let value: Value = serde_json::from_str(line)?;
        if value["reason"] == "compiler-artifact"
            && value["package_id"] == package
            && value["target"]["name"] == test
            && value["target"]["kind"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k == "test"))
            && value["profile"]["test"] == true
        {
            let path = value["executable"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("test artifact has no executable"))?;
            let path = Path::new(path).canonicalize()?;
            ensure!(
                path.starts_with(target.canonicalize()?),
                "test executable is outside target output"
            );
            ensure!(
                value["fresh"] == false,
                "stale test artifact in fresh build directory"
            );
            candidates.push(path);
        }
    }
    ensure!(
        candidates.len() == 1,
        "expected one selected test executable; found {}",
        candidates.len()
    );
    Ok(candidates.remove(0))
}

pub fn completed(stdout: &str) -> Option<u64> {
    let lines: Vec<_> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.len() != 2 || lines[0] != "RUNNING ALL TESTS" {
        return None;
    }
    let number = lines[1].strip_prefix("PASSED ")?.strip_suffix(" tests")?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let count = number.parse::<u64>().ok()?;
    (count > 0).then_some(count)
}

fn target_details(
    target: &str,
) -> Result<(&'static str, &'static str, &'static str, &'static str)> {
    match target {
        "x86_64-unknown-linux-ohos" => Ok((
            "x86_64",
            "Advanced Micro Devices X86-64",
            "/lib/ld-musl-x86_64.so.1",
            "x86_64-linux-ohos",
        )),
        "aarch64-unknown-linux-ohos" => Ok((
            "aarch64",
            "AArch64",
            "/lib/ld-musl-aarch64.so.1",
            "aarch64-linux-ohos",
        )),
        _ => anyhow::bail!("unreviewed upstream target"),
    }
}

fn verify_c_environment(environment: &Value, pins: &BuildConfig) -> Result<()> {
    let (_, _, _, clang_triple) = target_details(&pins.target)?;
    let cc_key = format!("CC_{}", pins.target.replace('-', "_"));
    ensure!(
        environment["target"]["rust_triple"] == pins.target,
        "C environment target differs from selected Rust target"
    );
    ensure!(
        environment["env"][&cc_key] == environment["toolchain"]["clang"]
            && environment["toolchain"]["clang"].is_string(),
        "target C compiler does not match SDK clang"
    );
    let sysroot = environment["sdk"]["sysroot"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("SDK sysroot missing"))?;
    ensure!(Path::new(sysroot).is_dir(), "SDK sysroot unavailable");
    let flags = environment["flags"]["cflags"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("target C flags missing"))?;
    for required in [
        format!("--target={clang_triple}"),
        format!("--sysroot={sysroot}"),
        "-D__MUSL__".into(),
    ] {
        ensure!(
            flags.iter().any(|flag| flag == &required),
            "missing target C flag {required}"
        );
    }
    Ok(())
}

pub fn runtime(
    root: &Path,
    build: &Path,
    output: &Path,
    runtime_config: Option<&Path>,
) -> Result<i32> {
    let mut report: Experiment = serde_json::from_slice(&fs::read(build.join("upstream.json"))?)?;
    report.validate()?;
    ensure!(
        report.phases["execution"].status == Status::NotRun,
        "build already requested execution"
    );
    crate::copy_evidence(build, &output.join("build-evidence"))?;
    if let Some(adaptation) = &mut report.adaptation {
        ensure!(
            digest(&build.join(&adaptation.patch))? == adaptation.patch_sha256
                && digest(&build.join(&adaptation.build_script))? == adaptation.build_script_sha256,
            "adaptation evidence changed"
        );
        adaptation.patch = format!("build-evidence/{}", adaptation.patch);
        adaptation.build_script = format!("build-evidence/{}", adaptation.build_script);
    }
    for record in &mut report.commands {
        crate::rebase(record);
    }
    for phase in report.phases.values_mut() {
        if let Some(record) = &mut phase.command {
            crate::rebase(record);
        }
    }
    if let Some(artifact) = &mut report.artifact {
        artifact.path = format!("build-evidence/{}", artifact.path);
        crate::rebase(&mut artifact.inspection);
    }
    if let Some(coverage) = &mut report.coverage {
        coverage.generated_rust = format!("build-evidence/{}", coverage.generated_rust);
        coverage.generated_c = format!("build-evidence/{}", coverage.generated_c);
    }
    report.run.id = format!("pulse-{}", uuid::Uuid::new_v4().simple());
    report.run.timestamp_utc = chrono::Utc::now().to_rfc3339();
    report.run.ci_run_id = std::env::var("GITHUB_RUN_ID").ok();
    report.run.ci_run_attempt = std::env::var("GITHUB_RUN_ATTEMPT").ok();
    if report.phases["inspection"].status != Status::Passed
        || report.phases["test_build"].status != Status::Passed
    {
        report.phases.insert(
            "execution".into(),
            Stage::new(Status::Blocked, "test build/inspection did not pass"),
        );
        report.write(output)?;
        return Ok(report.exit_code());
    }
    let (architecture, _, loader, _) = target_details(&report.build_pins.target)?;
    ensure!(
        report
            .artifact
            .as_ref()
            .is_some_and(|a| a.architecture == architecture),
        "artifact architecture differs from build target"
    );
    let mut config = match runtime_config {
        Some(path) => config::load_runtime_file(path)?,
        None => config::load_runtime(root)?,
    };
    ensure!(
        config
            .expected_api_version
            .is_none_or(|api| api == report.build_pins.sdk_api_version),
        "runtime API differs from selected SDK API"
    );
    config.execution_timeout_seconds = report.upstream.execution_timeout_seconds;
    fs::write(
        output.join("runtime-profile.json"),
        serde_json::to_vec_pretty(&config)?,
    )?;
    report.runtime_environment = Some(RuntimeEnvironment {
        profile: config.clone(),
        guest_identity: None,
        guest_dependencies: None,
    });
    let directory = format!("{}/{}", config.guest_base, report.run.id);
    let quoted = quote(&directory);
    let budget = config.transport_timeout_seconds;
    let mut executor = Executor::new(output, root)?;
    let mut created = false;
    let mut execution = Stage::new(Status::InfraError, "guest preparation did not complete");
    let result = (|| -> Result<()> {
        let artifact = report.artifact.as_ref().unwrap();
        let binary = output.join(&artifact.path).canonicalize()?;
        ensure!(
            binary.starts_with(output) && digest(&binary)? == artifact.sha256,
            "artifact containment/digest mismatch"
        );
        let coverage = report.coverage.as_ref().unwrap();
        let generated = output.join(&coverage.generated_rust).canonicalize()?;
        let generated_c = output.join(&coverage.generated_c).canonicalize()?;
        ensure!(
            generated.starts_with(output) && generated_c.starts_with(output),
            "generated sources escape evidence directory"
        );
        ensure!(
            digest(&generated)? == coverage.generated_rust_sha256
                && digest(&generated_c)? == coverage.generated_c_sha256,
            "generated sources digest mismatch"
        );
        ensure!(
            inventory(&fs::read_to_string(generated)?)? == coverage.generated_functions,
            "coverage inventory differs from build"
        );
        required(&mut executor, "hdc-version", &["hdc", "-v"], budget)?;
        required(
            &mut executor,
            "targets",
            &["hdc", "list", "targets"],
            budget,
        )?;
        let mut guest = Guest {
            executor: &mut executor,
            config: &config,
        };
        report.runtime_environment.as_mut().unwrap().guest_identity = Some(guest.shell(
            "guest-identity",
            "uname -a; id; param get const.ohos.fullname; param get const.product.software.version",
            budget,
        )?);
        ensure!(
            guest.shell("guest-architecture", "uname -m", budget)? == architecture,
            "guest architecture differs from selected build target"
        );
        if let Some(expected) = &config.expected_os_version {
            ensure!(
                guest.shell("guest-os-version", "param get const.ohos.fullname", budget)?
                    == *expected,
                "guest OS differs from runtime pins"
            );
        }
        if let Some(expected) = config.expected_api_version {
            ensure!(
                guest.shell(
                    "guest-api-version",
                    "param get const.ohos.apiversion",
                    budget
                )? == expected.to_string(),
                "guest API differs from runtime pins"
            );
        }
        guest.shell("guest-loader", &format!("test -r {loader}"), budget)?;
        let base = quote(&config.guest_base);
        guest.shell(
            "guest-writable-base",
            &format!("mkdir -p {base} && test -w {base}"),
            budget,
        )?;
        ensure!(
            guest.shell(
                "exit-probe",
                "sh -c 'exit 37'; rc=$?; test \"$rc\" -eq 37 && echo PULSE-EXIT-37",
                budget
            )? == "PULSE-EXIT-37",
            "guest status channel failed probe"
        );
        guest.shell("mkdir", &format!("umask 077; mkdir {quoted}"), budget)?;
        created = true;
        guest.hdc(
            "deploy",
            &[
                "file",
                "send",
                &binary.to_string_lossy(),
                &format!("{directory}/ctest"),
            ],
            budget,
        )?;
        guest.shell("chmod", &format!("chmod 700 {quoted}/ctest"), budget)?;
        // Require a deployed-byte check for the first experiment; missing tooling is an explicit blocker.
        let digest_output = guest.shell(
            "deployed-digest",
            &format!("sha256sum {quoted}/ctest"),
            budget,
        )?;
        ensure!(
            digest_output.split_whitespace().next() == Some(artifact.sha256.as_str()),
            "deployed executable digest mismatch"
        );
        report
            .runtime_environment
            .as_mut()
            .unwrap()
            .guest_dependencies = Some(guest.shell(
            "guest-dependencies",
            &format!("{loader} --list {quoted}/ctest"),
            budget,
        )?);
        let script = format!(
            "cd {quoted} || exit 1; ./ctest >stdout 2>stderr & pid=$!; printf '%s\\n' \"$pid\" >pid; wait \"$pid\"; rc=$?; rm -f pid; printf '%s\\n' \"$rc\" >exit-code"
        );
        guest.shell("execute", &script, config.execution_timeout_seconds)?;
        execution.command = guest.executor.records.last().cloned();
        for name in ["stdout", "stderr", "exit-code"] {
            guest.receive(&directory, name)?;
        }
        let raw = fs::read_to_string(output.join("logs/guest-exit-code.log"))?;
        let raw = raw.trim();
        ensure!(
            !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()),
            "invalid guest exit status"
        );
        let code: u8 = raw.parse()?;
        let count = completed(&fs::read_to_string(output.join("logs/guest-stdout.log"))?);
        report.coverage.as_mut().unwrap().successful_comparisons = count;
        execution.guest_exit_code = Some(code);
        execution.completion_matches = Some(count.is_some());
        execution.stdout = Some("logs/guest-stdout.log".into());
        execution.stderr = Some("logs/guest-stderr.log".into());
        execution.status = if code == 0 && count.is_some() {
            Status::Passed
        } else {
            Status::Failed
        };
        execution.reason = if execution.status == Status::Passed {
            None
        } else {
            Some(
                "nonzero guest exit or missing/nonpositive reviewed ctest completion summary"
                    .into(),
            )
        };
        Ok(())
    })();
    if let Err(error) = result {
        if execution.status != Status::Failed {
            execution.status = if executor
                .records
                .last()
                .is_some_and(|r| r.status == Status::TimedOut)
            {
                Status::TimedOut
            } else {
                Status::InfraError
            };
            execution.reason = Some(error.to_string());
        }
        report.diagnostics.push(error.to_string());
    }
    if created {
        let mut guest = Guest {
            executor: &mut executor,
            config: &config,
        };
        let script = format!(
            "cd {quoted} && {{ if test -f pid; then pid=$(cat pid); if kill -0 \"$pid\" 2>/dev/null; then kill -9 \"$pid\" || exit 1; fi; fi; }} && rm -f ctest stdout stderr exit-code pid && cd / && rmdir {quoted}"
        );
        match guest.shell("cleanup", &script, budget) {
            Ok(_) => {
                report.cleanup = Stage::executed(guest.executor.records.last().unwrap().clone())
            }
            Err(error) => {
                report.cleanup = Stage::new(Status::InfraError, &error.to_string());
                report.diagnostics.push(error.to_string());
                if execution.status == Status::Passed {
                    execution.status = Status::InfraError;
                    execution.reason = Some("required guest cleanup failed".into());
                }
            }
        }
    }
    report.phases.insert("execution".into(), execution);
    report.commands.extend(executor.records);
    report.write(output)?;
    Ok(report.exit_code())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn generated(functions: &str) -> String {
        format!(
            "mod generated_tests {{\npub fn check_type() {{}}\n}}\n{REVIEWED_MAIN}\nfn run_all() {{\n{functions}\n}}\n"
        )
    }
    #[test]
    fn inventory_requires_reviewed_main_and_nonempty_calls() {
        assert_eq!(
            inventory(&generated("check_type();")).unwrap(),
            vec!["check_type"]
        );
        assert!(inventory(&generated("")).is_err());
        assert!(inventory(&generated("unknown();")).is_err());
        assert!(inventory(&generated("check_type();\ncheck_type();")).is_err());
        assert!(inventory(&generated("check_type();").replace("run_all();", "")).is_err());
    }
    #[test]
    fn completion_rejects_empty_or_partial_execution() {
        assert_eq!(
            completed("RUNNING ALL TESTS\nPASSED 123 tests\n"),
            Some(123)
        );
        for text in [
            "",
            "RUNNING ALL TESTS\n",
            "RUNNING ALL TESTS\nPASSED 0 tests",
            "PASSED 1 tests",
            "RUNNING ALL TESTS\nPASSED 1 tests\nPASSED 1 tests",
            "RUNNING ALL TESTS\nPASSED -1 tests",
        ] {
            assert_eq!(completed(text), None, "{text}");
        }
    }
    #[test]
    fn artifact_discovery_rejects_ambiguous_or_stale_output() -> Result<()> {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join("ctest"), "test")?;
        let record = serde_json::json!({"reason":"compiler-artifact", "package_id":"libc-test", "target":{"name":"ctest","kind":["test"]},"profile":{"test":true}, "executable":temp.path().join("ctest"), "fresh":false});
        let text = record.to_string();
        assert!(select_artifact(&text, "libc-test", "ctest", temp.path()).is_ok());
        assert!(
            select_artifact(
                &format!("{text}\n{text}"),
                "libc-test",
                "ctest",
                temp.path()
            )
            .is_err()
        );
        assert!(
            select_artifact(
                &text.replace("\"fresh\":false", "\"fresh\":true"),
                "libc-test",
                "ctest",
                temp.path()
            )
            .is_err()
        );
        assert!(select_artifact(&text, "other", "ctest", temp.path()).is_err());
        Ok(())
    }
}
