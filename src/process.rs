use crate::report::{CommandRecord, Status};
use anyhow::{Context, Result};
use std::os::unix::process::CommandExt;
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct Executor {
    pub output: PathBuf,
    pub cwd: PathBuf,
    pub records: Vec<CommandRecord>,
}

impl Executor {
    pub fn new(output: &Path, cwd: &Path) -> Result<Self> {
        fs::create_dir_all(output.join("logs"))?;
        Ok(Self {
            output: output.to_owned(),
            cwd: cwd.to_owned(),
            records: vec![],
        })
    }

    pub fn execute(
        &mut self,
        name: &str,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandRecord> {
        anyhow::ensure!(!args.is_empty(), "empty command");
        let start = Instant::now();
        let stdout = format!("logs/{name}.stdout.log");
        let stderr = format!("logs/{name}.stderr.log");
        let mut record = CommandRecord {
            name: name.into(),
            command: args.to_vec(),
            status: Status::InfraError,
            exit_code: None,
            elapsed_ms: 0,
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            reason: None,
        };
        let mut command = Command::new(&args[0]);
        command
            .args(&args[1..])
            .current_dir(&self.cwd)
            .stdin(Stdio::null())
            .stdout(File::create(self.output.join(&stdout))?)
            .stderr(File::create(self.output.join(&stderr))?)
            .process_group(0);
        // Credentials belong only to CI provisioning, never fixture/build execution.
        command.env_remove("GITHUB_TOKEN").env_remove("GH_TOKEN");
        match command.spawn() {
            Err(error) => {
                record.reason = Some(error.to_string());
                fs::write(self.output.join(&stderr), error.to_string())?;
            }
            Ok(mut child) => loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        record.exit_code = status.code();
                        record.status = if status.success() {
                            Status::Passed
                        } else {
                            Status::Failed
                        };
                        if !status.success() {
                            record.reason = Some(
                                "command returned nonzero; attribution requires review".into(),
                            );
                        }
                        break;
                    }
                    Ok(None) if start.elapsed() < timeout => {
                        thread::sleep(Duration::from_millis(20))
                    }
                    result => {
                        // Every child gets a fresh process group, so descendants cannot outlive a timeout.
                        unsafe {
                            libc::kill(-(child.id() as i32), libc::SIGKILL);
                        }
                        child.wait().context("reap terminated child")?;
                        if let Err(error) = result {
                            record.status = Status::InfraError;
                            record.reason = Some(error.to_string());
                        } else {
                            record.status = Status::TimedOut;
                            record.reason = Some(
                                "command exceeded its time budget; process group terminated".into(),
                            );
                        }
                        break;
                    }
                }
            },
        }
        record.elapsed_ms = start.elapsed().as_millis() as u64;
        self.records.push(record.clone());
        Ok(record)
    }

    pub fn stdout(&self, record: &CommandRecord) -> Result<String> {
        Ok(String::from_utf8_lossy(&fs::read(self.output.join(&record.stdout))?).into_owned())
    }
}

pub fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failures_preserve_separate_logs() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut executor = Executor::new(temp.path(), temp.path())?;
        let record = executor.execute(
            "failure",
            &args(&["sh", "-c", "echo out; echo err >&2; exit 37"]),
            Duration::from_secs(2),
        )?;
        assert_eq!(record.status, Status::Failed);
        assert_eq!(record.exit_code, Some(37));
        assert_eq!(executor.stdout(&record)?, "out\n");
        assert_eq!(
            fs::read_to_string(temp.path().join(record.stderr))?,
            "err\n"
        );
        Ok(())
    }
    #[test]
    fn timeout_terminates_descendants() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut executor = Executor::new(temp.path(), temp.path())?;
        let record = executor.execute(
            "timeout",
            &args(&["sh", "-c", "(sleep 1; echo survived > escaped) & wait"]),
            Duration::from_millis(100),
        )?;
        assert_eq!(record.status, Status::TimedOut);
        thread::sleep(Duration::from_millis(1100));
        assert!(!temp.path().join("escaped").exists());
        Ok(())
    }
}
