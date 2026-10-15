use crate::{
    config,
    process::{Executor, args},
    report::*,
};
use anyhow::{Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::Path, time::Duration};

pub fn digest(path: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}

pub fn run(root: &Path, output: &Path, runtime_requested: bool) -> Result<Report> {
    let inputs = config::load_build(root)?;
    let fixture = root.join("fixtures/libc-file-io").canonicalize()?;
    ensure!(fixture.starts_with(root), "fixture escapes repository");
    let mut executor = Executor::new(output, &fixture)?;
    let mut hash = Sha256::new();
    for name in ["Cargo.lock", "Cargo.toml", "src/main.rs"] {
        let source = fixture.join(name).canonicalize()?;
        ensure!(
            source.starts_with(&fixture),
            "fixture source escapes package"
        );
        let content = fs::read(&source)?;
        let relative = format!("fixtures/libc-file-io/{name}");
        hash.update((relative.len() as u64).to_be_bytes());
        hash.update(relative.as_bytes());
        hash.update((content.len() as u64).to_be_bytes());
        hash.update(&content);
        let dest = output.join("fixture").join(name);
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::write(dest, content)?;
    }
    fs::create_dir_all(output.join("config"))?;
    fs::copy(
        root.join("config/feasibility.json"),
        output.join("config/feasibility.json"),
    )?;
    let mut stages = BTreeMap::new();
    stages.insert(
        "check".into(),
        Stage::new(Status::InfraError, "preflight did not complete"),
    );
    stages.insert(
        "link".into(),
        Stage::new(Status::Blocked, "preflight did not pass"),
    );
    stages.insert(
        "runtime".into(),
        Stage::new(
            if runtime_requested {
                Status::Blocked
            } else {
                Status::NotRun
            },
            if runtime_requested {
                "link did not pass"
            } else {
                "runtime was not requested"
            },
        ),
    );
    stages.insert(
        "integration".into(),
        Stage::new(Status::NotRun, "ArkTS integration is outside MVP scope"),
    );
    let mut report = Report {
        schema_version: 1,
        run: RunIdentity { id: format!("pulse-{}", uuid::Uuid::new_v4().simple()), timestamp_utc: chrono::Utc::now().to_rfc3339(), cli_version: env!("CARGO_PKG_VERSION").into(), source_commit: String::new(), source_dirty: false, ci_run_id: env::var("GITHUB_RUN_ID").ok(), ci_run_attempt: env::var("GITHUB_RUN_ATTEMPT").ok() },
        subject: Subject { fixture_id: "libc-file-io".into(), scenario_id: "file-round-trip".into(), artifact_kind: "executable".into(), fixture_sha256: format!("{:x}", hash.finalize()), lockfile_sha256: digest(&fixture.join("Cargo.lock"))?, libc_version: None, features: vec![] },
        environment: Environment { host: format!("{}-{}", env::consts::OS, env::consts::ARCH), inputs: inputs.clone(), rustc: None, cargo: None, sdk_and_tools: None, sdk_provenance: "SDK archive digest/attestation verification is unresolved; installed version/API are checked.".into() },
        runtime_environment: None, stages, artifact: None, commands: vec![], cleanup: Stage::new(Status::NotRun, "guest directory was not created"), diagnostics: vec![],
    };
    let outcome = (|| -> Result<()> {
        executor.cwd = root.to_owned();
        let git = executor.execute(
            "source-commit",
            &args(&["git", "rev-parse", "HEAD"]),
            Duration::from_secs(30),
        )?;
        ensure!(
            git.status == Status::Passed,
            "cannot identify source commit"
        );
        report.run.source_commit = executor.stdout(&git)?.trim().into();
        let dirty = executor.execute(
            "source-status",
            &args(&["git", "status", "--porcelain"]),
            Duration::from_secs(30),
        )?;
        ensure!(
            dirty.status == Status::Passed,
            "cannot identify source dirty state"
        );
        report.run.source_dirty = !executor.stdout(&dirty)?.trim().is_empty();
        if report.run.source_dirty {
            // Preserve tracked edits; fixture inputs are always separately snapshotted.
            let patch = executor.execute(
                "source-patch",
                &args(&["git", "diff", "HEAD", "--binary"]),
                Duration::from_secs(30),
            )?;
            ensure!(
                patch.status == Status::Passed,
                "cannot preserve dirty source patch"
            );
        }
        executor.cwd = fixture.clone();
        let rust = executor.execute(
            "rustc-version",
            &args(&["rustc", "-Vv"]),
            Duration::from_secs(30),
        )?;
        ensure!(rust.status == Status::Passed, "rustc preflight failed");
        let rust_text = executor.stdout(&rust)?;
        ensure!(
            rust_text.starts_with(&format!("rustc {} ", inputs.rust_version)),
            "Rust version differs from pin"
        );
        report.environment.rustc = Some(rust_text);
        let cargo = executor.execute(
            "cargo-version",
            &args(&["cargo", "--version"]),
            Duration::from_secs(30),
        )?;
        ensure!(cargo.status == Status::Passed, "Cargo preflight failed");
        report.environment.cargo = Some(executor.stdout(&cargo)?);
        let preflight = executor.execute(
            "preflight",
            &args(&[
                "cargo",
                "ohos",
                "env",
                "-t",
                &inputs.target,
                "--format",
                "json",
            ]),
            Duration::from_secs(60),
        )?;
        ensure!(
            preflight.status == Status::Passed,
            "cargo-ohos environment discovery failed"
        );
        let environment: Value = serde_json::from_str(&executor.stdout(&preflight)?)?;
        report.environment.sdk_and_tools = Some(environment.clone());
        ensure!(
            environment["cargo_ohos_version"] == inputs.cargo_ohos_version,
            "cargo-ohos version differs from pin"
        );
        ensure!(
            environment["sdk"]["version"] == inputs.sdk_version
                && environment["sdk"]["api_version"] == inputs.sdk_api_version,
            "SDK version/API differs from pin"
        );
        let readelf = environment["toolchain"]["readelf"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("SDK readelf missing"))?;
        ensure!(Path::new(readelf).is_file(), "SDK readelf unavailable");
        let metadata = executor.execute(
            "cargo-metadata",
            &args(&["cargo", "metadata", "--locked", "--format-version", "1"]),
            Duration::from_secs(120),
        )?;
        ensure!(
            metadata.status == Status::Passed,
            "dependency metadata unavailable"
        );
        let graph: Value = serde_json::from_str(&executor.stdout(&metadata)?)?;
        let libc = graph["packages"]
            .as_array()
            .and_then(|p| p.iter().find(|p| p["name"] == "libc"))
            .ok_or_else(|| anyhow::anyhow!("libc not resolved"))?;
        report.subject.libc_version = libc["version"].as_str().map(str::to_owned);
        let node = graph["resolve"]["nodes"]
            .as_array()
            .and_then(|n| n.iter().find(|n| n["id"] == libc["id"]))
            .ok_or_else(|| anyhow::anyhow!("libc resolved features missing"))?;
        report.subject.features = serde_json::from_value(node["features"].clone())?;
        for (stage, subcommand) in [("check", "check"), ("link", "build")] {
            if stage == "link" && report.stages["check"].status != Status::Passed {
                report.stages.insert(
                    "link".into(),
                    Stage::new(Status::Blocked, "check did not pass"),
                );
                break;
            }
            let target_dir = output.join("build");
            let record = executor.execute(
                stage,
                &args(&[
                    "cargo",
                    "ohos",
                    subcommand,
                    "-t",
                    &inputs.target,
                    "--locked",
                    "--bin",
                    "libc-file-io",
                    "--target-dir",
                    &target_dir.to_string_lossy(),
                ]),
                Duration::from_secs(600),
            )?;
            report.stages.insert(stage.into(), Stage::executed(record));
        }
        if report.stages["link"].status == Status::Passed {
            let inspection = (|| -> Result<Artifact> {
                fs::create_dir(output.join("artifacts"))?;
                let copied = output.join("artifacts/libc-file-io");
                fs::copy(
                    output
                        .join("build")
                        .join(&inputs.target)
                        .join("debug/libc-file-io"),
                    &copied,
                )?;
                let record = executor.execute(
                    "artifact-inspection",
                    &args(&[readelf, "-h", "-l", "-d", &copied.to_string_lossy()]),
                    Duration::from_secs(30),
                )?;
                let header = executor.stdout(&record)?;
                ensure!(
                    record.status == Status::Passed
                        && header.contains("Advanced Micro Devices X86-64")
                        && header.contains("/lib/ld-musl-x86_64.so.1"),
                    "artifact architecture/interpreter inspection failed"
                );
                Ok(Artifact {
                    path: "artifacts/libc-file-io".into(),
                    sha256: digest(&copied)?,
                    architecture: "x86_64".into(),
                    inspection: record,
                })
            })();
            match inspection {
                Ok(artifact) => report.artifact = Some(artifact),
                Err(error) => {
                    report.stages.get_mut("link").unwrap().status = Status::InfraError;
                    report.stages.get_mut("link").unwrap().reason = Some(error.to_string());
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = outcome {
        report.diagnostics.push(error.to_string());
        report.stages.insert(
            "check".into(),
            Stage::new(Status::InfraError, &error.to_string()),
        );
    }
    if report.stages["link"].status == Status::Passed && runtime_requested {
        report.stages.insert(
            "runtime".into(),
            Stage::new(Status::Blocked, "requested guest execution has not started"),
        );
    }
    report.commands = executor.records;
    // Only remove intermediate objects created in this fresh output directory.
    if output.join("build").exists() {
        fs::remove_dir_all(output.join("build"))?;
    }
    Ok(report)
}
