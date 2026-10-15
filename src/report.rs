use crate::config::{BuildConfig, RuntimeConfig};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Passed,
    Failed,
    Blocked,
    NotRun,
    InfraError,
    TimedOut,
}
impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_value(self).unwrap().as_str().unwrap()
        )
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CommandRecord {
    pub name: String,
    pub command: Vec<String>,
    pub status: Status,
    pub exit_code: Option<i32>,
    pub elapsed_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Stage {
    pub status: Status,
    pub reason: Option<String>,
    pub command: Option<CommandRecord>,
    pub guest_exit_code: Option<u8>,
    pub completion_matches: Option<bool>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}
impl Stage {
    pub fn new(status: Status, reason: &str) -> Self {
        Self {
            status,
            reason: Some(reason.into()),
            command: None,
            guest_exit_code: None,
            completion_matches: None,
            stdout: None,
            stderr: None,
        }
    }
    pub fn executed(command: CommandRecord) -> Self {
        let mut stage = Self::new(command.status, "");
        stage.reason = command.reason.clone();
        stage.command = Some(command);
        stage
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Artifact {
    pub path: String,
    pub sha256: String,
    pub architecture: String,
    pub inspection: CommandRecord,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunIdentity {
    pub id: String,
    pub timestamp_utc: String,
    pub cli_version: String,
    pub source_commit: String,
    pub source_dirty: bool,
    pub ci_run_id: Option<String>,
    pub ci_run_attempt: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Subject {
    pub fixture_id: String,
    pub scenario_id: String,
    pub artifact_kind: String,
    pub fixture_sha256: String,
    pub lockfile_sha256: String,
    pub libc_version: Option<String>,
    pub features: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Environment {
    pub host: String,
    pub inputs: BuildConfig,
    pub rustc: Option<String>,
    pub cargo: Option<String>,
    pub sdk_and_tools: Option<Value>,
    pub sdk_provenance: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuntimeEnvironment {
    pub profile: RuntimeConfig,
    pub guest_identity: Option<String>,
    pub guest_dependencies: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub run: RunIdentity,
    pub subject: Subject,
    pub environment: Environment,
    pub runtime_environment: Option<RuntimeEnvironment>,
    pub stages: BTreeMap<String, Stage>,
    pub artifact: Option<Artifact>,
    pub commands: Vec<CommandRecord>,
    pub cleanup: Stage,
    pub diagnostics: Vec<String>,
}

pub fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}
impl Report {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "unsupported report version");
        ensure!(self.stages.len() == 4, "unexpected stage names");
        ensure!(
            self.stages
                .get("integration")
                .is_some_and(|s| s.status == Status::NotRun),
            "integration is outside implemented scope"
        );
        for name in ["check", "link", "runtime", "integration"] {
            let stage = self
                .stages
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("missing stage {name}"))?;
            if stage.status != Status::Passed {
                ensure!(
                    stage.reason.as_ref().is_some_and(|s| !s.is_empty()),
                    "missing reason for {name}"
                );
            }
            for path in [&stage.stdout, &stage.stderr].into_iter().flatten() {
                ensure!(safe_relative(path), "invalid log path");
            }
            if matches!(name, "check" | "link")
                && matches!(
                    stage.status,
                    Status::Passed | Status::Failed | Status::TimedOut
                )
            {
                ensure!(stage.command.is_some(), "missing command for {name}");
                if stage.status == Status::Passed {
                    ensure!(
                        stage
                            .command
                            .as_ref()
                            .is_some_and(|c| c.status == Status::Passed && c.exit_code == Some(0)),
                        "passing {name} lacks successful command evidence"
                    );
                }
            }
        }
        for command in &self.commands {
            ensure!(
                safe_relative(&command.stdout) && safe_relative(&command.stderr),
                "invalid command log path"
            );
        }
        if self.stages["link"].status == Status::Passed {
            ensure!(
                self.stages["check"].status == Status::Passed,
                "link passed without check"
            );
            let artifact = self
                .artifact
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("missing artifact"))?;
            ensure!(safe_relative(&artifact.path), "invalid artifact path");
            ensure!(
                artifact.inspection.status == Status::Passed
                    && artifact.inspection.exit_code == Some(0),
                "artifact inspection did not pass"
            );
            ensure!(
                artifact.sha256.len() == 64
                    && artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid artifact digest"
            );
        }
        if self.stages["runtime"].status == Status::Passed {
            let runtime = &self.stages["runtime"];
            ensure!(
                self.stages["link"].status == Status::Passed
                    && self.cleanup.status == Status::Passed,
                "runtime passed without link/cleanup"
            );
            ensure!(
                runtime.guest_exit_code == Some(0) && runtime.completion_matches == Some(true),
                "runtime passed without assertion evidence"
            );
            ensure!(
                runtime
                    .command
                    .as_ref()
                    .is_some_and(|c| c.status == Status::Passed && c.exit_code == Some(0)),
                "runtime command evidence missing"
            );
            ensure!(
                self.cleanup
                    .command
                    .as_ref()
                    .is_some_and(|c| c.status == Status::Passed && c.exit_code == Some(0)),
                "cleanup command evidence missing"
            );
            ensure!(
                runtime.stdout.is_some()
                    && runtime.stderr.is_some()
                    && self
                        .runtime_environment
                        .as_ref()
                        .is_some_and(|r| r.guest_identity.is_some()),
                "missing runtime evidence"
            );
        }
        Ok(())
    }
    pub fn exit_code(&self) -> i32 {
        if self.commands.iter().any(|c| c.status == Status::InfraError)
            || self.stages.values().any(|s| s.status == Status::InfraError)
            || self.cleanup.status == Status::InfraError
        {
            2
        } else if self
            .stages
            .values()
            .any(|s| !matches!(s.status, Status::Passed | Status::NotRun))
        {
            1
        } else {
            0
        }
    }
    pub fn markdown(&self) -> Result<String> {
        self.validate()?;
        let escape = |s: &str| {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('|', "&#124;")
                .replace('`', "&#96;")
                .replace(['\r', '\n'], " ")
        };
        let mut text = format!(
            "# libc file-round-trip\n\nRun: `{}`; source: `{}`; dirty: `{}`.\n\nScope: one standalone executable, one scenario; no crate-wide compatibility claim.\n\n| Stage | Status | Reason |\n|---|---|---|\n",
            escape(&self.run.id),
            escape(&self.run.source_commit),
            self.run.source_dirty
        );
        for name in ["check", "link", "runtime", "integration"] {
            let stage = &self.stages[name];
            text.push_str(&format!(
                "| {name} | {} | {} |\n",
                stage.status,
                escape(stage.reason.as_deref().unwrap_or(""))
            ));
        }
        text.push_str(&format!(
            "\nSDK: {} / API {}. Rust: {}; cargo-ohos: {}.\n\n{}\n",
            escape(&self.environment.inputs.sdk_version),
            self.environment.inputs.sdk_api_version,
            escape(&self.environment.inputs.rust_version),
            escape(&self.environment.inputs.cargo_ohos_version),
            escape(&self.environment.sdk_provenance)
        ));
        if let Some(runtime) = &self.runtime_environment {
            text.push_str(&format!("\nGuest: {}\n\nDependencies: {}\n\nImage: {}/{}; expected SHA-256: {}. Image verification is performed by CI provisioning; the local CLI does not attest guest image provenance.\n", escape(runtime.guest_identity.as_deref().unwrap_or("unavailable")), escape(runtime.guest_dependencies.as_deref().unwrap_or("unavailable")), escape(&runtime.profile.image_release), escape(&runtime.profile.image_asset), escape(&runtime.profile.image_sha256)));
        }
        text.push_str(&format!("\nCleanup: {}.\n\n", self.cleanup.status));
        for diagnostic in &self.diagnostics {
            text.push_str(&format!("- {}\n", escape(diagnostic)));
        }
        text.push_str("\nInspect run.json and logs for exact commands, durations and outcomes. CI diagnostics are retained for 14 days. Reproduce from the recorded source/fixture snapshot using the documented pinned tools and a fresh output directory.\n");
        Ok(text)
    }
    pub fn write(&self, output: &Path) -> Result<()> {
        self.validate()?;
        let temp = output.join("run.json.tmp");
        fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temp, output.join("run.json"))?;
        // Generate Markdown from the serialized evidence, not an independent status path.
        let saved: Self = serde_json::from_slice(&fs::read(output.join("run.json"))?)?;
        let summary = saved.markdown()?;
        fs::write(output.join("summary.md"), &summary)?;
        print!("{summary}");
        Ok(())
    }
}
