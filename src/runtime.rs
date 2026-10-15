use crate::{
    build::digest,
    config::RuntimeConfig,
    process::{Executor, args},
    report::*,
};
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{fs, path::Path, time::Duration};

pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Completion {
    scenario_id: String,
    run_id: String,
    status: String,
}
pub fn completion_matches(stdout: &str, id: &str) -> bool {
    let mut lines = stdout.lines().filter(|line| !line.trim().is_empty());
    let Some(line) = lines.next() else {
        return false;
    };
    if lines.next().is_some() {
        return false;
    }
    serde_json::from_str::<Completion>(line)
        .is_ok_and(|r| r.scenario_id == "file-round-trip" && r.run_id == id && r.status == "passed")
}

struct Guest<'a> {
    executor: &'a mut Executor,
    config: &'a RuntimeConfig,
}
impl Guest<'_> {
    fn hdc(&mut self, name: &str, values: &[&str], timeout: u64) -> Result<String> {
        let mut command = args(&["hdc", "-t", &self.config.connect_key]);
        command.extend(args(values));
        let record = self
            .executor
            .execute(name, &command, Duration::from_secs(timeout))?;
        ensure!(
            record.status == Status::Passed,
            "{name}: transport {}; inspect {} and {}",
            record.status,
            record.stdout,
            record.stderr
        );
        self.executor.stdout(&record)
    }
    fn shell(&mut self, name: &str, script: &str, timeout: u64) -> Result<String> {
        let marker = format!("PULSE-SHELL-{}", uuid::Uuid::new_v4().simple());
        let wrapped = format!(
            "sh -c {}",
            quote(&format!(
                "( {script} ); rc=$?; printf '\\n{marker}:%s\\n' \"$rc\""
            ))
        );
        let text = self.hdc(name, &["shell", &wrapped], timeout)?;
        let (body, code) = parse_marker(&text, &marker)?;
        ensure!(
            code == 0,
            "{name}: guest exit code {code}; inspect logs/{name}.stdout.log and logs/{name}.stderr.log"
        );
        Ok(body)
    }
    fn receive(&mut self, directory: &str, name: &str) -> Result<()> {
        let local = self.executor.output.join(format!("logs/guest-{name}.log"));
        self.hdc(
            &format!("receive-{name}"),
            &[
                "file",
                "recv",
                &format!("{directory}/{name}"),
                &local.to_string_lossy(),
            ],
            self.config.transport_timeout_seconds,
        )?;
        ensure!(local.is_file(), "missing guest {name} evidence");
        Ok(())
    }
}
pub fn parse_marker(text: &str, marker: &str) -> Result<(String, u8)> {
    let text = text.trim_end();
    let (body, last) = text
        .rsplit_once('\n')
        .ok_or_else(|| anyhow::anyhow!("remote status marker missing"))?;
    let raw = last
        .strip_prefix(&format!("{marker}:"))
        .ok_or_else(|| anyhow::anyhow!("remote status marker missing"))?;
    ensure!(
        !raw.is_empty() && raw.bytes().all(|c| c.is_ascii_digit()),
        "invalid remote status marker"
    );
    Ok((body.trim().into(), raw.parse()?))
}

pub fn run(root: &Path, output: &Path, report: &mut Report, config: RuntimeConfig) -> Result<()> {
    if report.stages["link"].status != Status::Passed {
        report.stages.insert(
            "runtime".into(),
            Stage::new(Status::Blocked, "link did not pass"),
        );
        return Ok(());
    }
    let mut executor = Executor::new(output, root)?;
    report.runtime_environment = Some(RuntimeEnvironment {
        profile: config.clone(),
        guest_identity: None,
        guest_dependencies: None,
    });
    let directory = format!("{}/{}", config.guest_base, report.run.id);
    let quoted = quote(&directory);
    let budget = config.transport_timeout_seconds;
    let mut created = false;
    let mut stage = Stage::new(Status::InfraError, "guest setup did not complete");
    let result = (|| -> Result<()> {
        let artifact = report
            .artifact
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing build artifact"))?;
        ensure!(safe_relative(&artifact.path), "unsafe artifact path");
        let binary = output.join(&artifact.path).canonicalize()?;
        ensure!(
            binary.starts_with(output),
            "artifact escapes evidence directory"
        );
        ensure!(
            digest(&binary)? == artifact.sha256,
            "build artifact digest mismatch"
        );
        let version = executor.execute(
            "hdc-version",
            &args(&["hdc", "-v"]),
            Duration::from_secs(budget),
        )?;
        ensure!(
            version.status == Status::Passed,
            "hdc version discovery failed"
        );
        let targets = executor.execute(
            "targets",
            &args(&["hdc", "list", "targets"]),
            Duration::from_secs(budget),
        )?;
        ensure!(
            targets.status == Status::Passed,
            "hdc target discovery failed"
        );
        let mut guest = Guest {
            executor: &mut executor,
            config: &config,
        };
        report.runtime_environment.as_mut().unwrap().guest_identity = Some(guest.shell(
            "guest-identity",
            "uname -a; id; param get const.ohos.fullname; param get const.product.software.version",
            budget,
        )?);
        guest.shell("guest-paths", "for p in /lib /lib64 /system/lib /system/lib64 /data/local /data/local/tmp; do echo \"PATH: $p\"; ls -ld \"$p\"; done; find /lib /lib64 /system/lib /system/lib64 -name libc.so -o -name 'ld-musl*' 2>/dev/null; true", budget)?;
        guest.shell("guest-loader", "test -r /lib/ld-musl-x86_64.so.1 || { echo 'Required ELF interpreter is unavailable' >&2; exit 1; }", budget)?;
        let base = quote(&config.guest_base);
        guest.shell(
            "guest-writable-base",
            &format!("mkdir -p {base} && test -d {base} && test -w {base}"),
            budget,
        )?;
        let probe = guest.shell(
            "exit-probe",
            "sh -c 'exit 37'; rc=$?; test \"$rc\" -eq 37 && echo PULSE-EXIT-37",
            budget,
        )?;
        ensure!(probe == "PULSE-EXIT-37", "guest exit-status probe failed");
        guest.shell("mkdir", &format!("umask 077; mkdir {quoted}"), budget)?;
        created = true;
        guest.hdc(
            "deploy",
            &[
                "file",
                "send",
                &binary.to_string_lossy(),
                &format!("{directory}/fixture"),
            ],
            budget,
        )?;
        guest.shell("chmod", &format!("chmod 700 {quoted}/fixture"), budget)?;
        report
            .runtime_environment
            .as_mut()
            .unwrap()
            .guest_dependencies = Some(guest.shell(
            "guest-dependencies",
            &format!("/lib/ld-musl-x86_64.so.1 --list {quoted}/fixture"),
            budget,
        )?);
        let script = format!(
            "cd {quoted} || exit 1; ./fixture . {} >stdout 2>stderr & pid=$!; printf '%s\\n' \"$pid\" >pid; wait \"$pid\"; rc=$?; rm -f pid; printf '%s\\n' \"$rc\" >exit-code",
            quote(&report.run.id)
        );
        guest.shell("execute", &script, config.execution_timeout_seconds)?;
        stage.command = guest.executor.records.last().cloned();
        for name in ["stdout", "stderr", "exit-code"] {
            guest.receive(&directory, name)?;
        }
        let raw = fs::read_to_string(output.join("logs/guest-exit-code.log"))?;
        let raw = raw.trim();
        ensure!(
            !raw.is_empty() && raw.bytes().all(|c| c.is_ascii_digit()),
            "invalid guest exit status"
        );
        let code: u8 = raw.parse()?;
        let matched = completion_matches(
            &fs::read_to_string(output.join("logs/guest-stdout.log"))?,
            &report.run.id,
        );
        stage.guest_exit_code = Some(code);
        stage.completion_matches = Some(matched);
        stage.stdout = Some("logs/guest-stdout.log".into());
        stage.stderr = Some("logs/guest-stderr.log".into());
        stage.status = if code == 0 && matched {
            Status::Passed
        } else {
            Status::Failed
        };
        stage.reason = if stage.status == Status::Passed {
            None
        } else {
            Some("guest exit status or completion record failed acceptance".into())
        };
        guest.shell(
            "fixture-cleanup-check",
            &format!("test ! -e {quoted}/ohos-pulse-{}.tmp", report.run.id),
            budget,
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        if stage.status != Status::Failed {
            stage.status = if executor
                .records
                .last()
                .is_some_and(|c| c.status == Status::TimedOut)
            {
                Status::TimedOut
            } else {
                Status::InfraError
            };
            stage.reason = Some(error.to_string());
        }
        report.diagnostics.push(error.to_string());
    }
    if created {
        let mut guest = Guest {
            executor: &mut executor,
            config: &config,
        };
        let script = format!(
            "cd {quoted} && {{ if test -f pid; then pid=$(cat pid); if kill -0 \"$pid\" 2>/dev/null; then kill -9 \"$pid\" || exit 1; fi; fi; }} && rm -f fixture stdout stderr exit-code pid && cd / && rmdir {quoted}"
        );
        match guest.shell("cleanup", &script, budget) {
            Ok(_) => {
                report.cleanup = Stage::executed(guest.executor.records.last().unwrap().clone())
            }
            Err(error) => {
                report.cleanup = Stage::new(Status::InfraError, &error.to_string());
                report.diagnostics.push(error.to_string());
                if stage.status == Status::Passed {
                    stage.status = Status::InfraError;
                    stage.reason = Some("required guest cleanup failed".into());
                }
            }
        }
    }
    report.commands.extend(executor.records);
    report.stages.insert("runtime".into(), stage);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completion_is_bound_to_run_and_scenario() {
        let line = r#"{"scenario_id":"file-round-trip","run_id":"run-1","status":"passed"}"#;
        assert!(completion_matches(line, "run-1"));
        assert!(!completion_matches(line, "run-2"));
        assert!(!completion_matches(&format!("{line}\n{line}"), "run-1"));
        assert!(!completion_matches("", "run-1"));
        assert!(!completion_matches(
            &line.replace("file-round-trip", "other"),
            "run-1"
        ));
    }
    #[test]
    fn transport_success_without_marker_cannot_pass() {
        assert!(parse_marker("hello\n", "token").is_err());
        assert!(parse_marker("\ntoken:0\nextra", "token").is_err());
        assert_eq!(
            parse_marker("output\ntoken:37\n", "token").unwrap(),
            ("output".into(), 37)
        );
        assert!(parse_marker("\ntoken:999", "token").is_err());
    }
    #[test]
    fn remote_arguments_are_shell_quoted() {
        assert_eq!(quote("a'b"), "'a'\"'\"'b'");
    }
}
