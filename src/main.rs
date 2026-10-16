mod build;
mod config;
mod process;
mod report;
mod runtime;
mod upstream;

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use report::Report;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "ohos-pulse",
    version,
    about = "Reproducible OHOS build and runtime evidence"
)]
struct Cli {
    #[arg(long, default_value = ".", global = true)]
    root: PathBuf,
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// Compile the reviewed upstream libc ctest target without running it.
    UpstreamBuild {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// Explicit version-specific adaptation; baseline preserves upstream sources.
        #[arg(long, value_enum, default_value = "baseline")]
        profile: upstream::BuildProfile,
        /// Override build pins for a separate upstream experiment.
        #[arg(long)]
        build_config: Option<PathBuf>,
    },
    /// Run an upstream ctest build in the provisioned OH guest.
    UpstreamRuntime {
        #[arg(long)]
        build: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        runtime_config: Option<PathBuf>,
    },
    /// Check and link the reviewed fixture, optionally execute on a provisioned guest.
    Run {
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        runtime: bool,
    },
    /// Execute an existing Rust CLI build in a provisioned guest, preserving build evidence.
    Runtime {
        #[arg(long)]
        build: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Validate run.json and regenerate Markdown without executing any commands.
    Report {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
}

fn fresh_output(path: &Path) -> Result<PathBuf> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(path)
        .with_context(|| format!("output must be a new directory: {}", path.display()))?;
    Ok(path.canonicalize()?)
}
fn copy_evidence(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(!kind.is_symlink(), "evidence contains a symlink");
        let dest = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_evidence(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}
fn rebase(command: &mut report::CommandRecord) {
    command.stdout = format!("build-evidence/{}", command.stdout);
    command.stderr = format!("build-evidence/{}", command.stderr);
}
fn execute(cli: Cli) -> Result<i32> {
    let root = cli.root.canonicalize()?;
    match cli.command {
        Commands::UpstreamBuild {
            source,
            output,
            profile,
            build_config,
        } => {
            let config = upstream::load_config(&root)?;
            let source = source.canonicalize()?;
            let output = fresh_output(&output)?;
            let build_config = build_config.map(|p| root.join(p));
            upstream::build(
                &root,
                &source,
                &output,
                config,
                profile,
                build_config.as_deref(),
            )
        }
        Commands::UpstreamRuntime {
            build,
            output,
            runtime_config,
        } => {
            let build = build.canonicalize()?;
            let output = fresh_output(&output)?;
            ensure!(
                !output.starts_with(&build) && !build.starts_with(&output),
                "build/output directories overlap"
            );
            let runtime_config = runtime_config.map(|p| root.join(p));
            upstream::runtime(&root, &build, &output, runtime_config.as_deref())
        }
        Commands::Run {
            output,
            runtime: requested,
        } => {
            // Validate configuration before creating an evidence destination.
            config::load_build(&root)?;
            let runtime_config = if requested {
                Some(config::load_runtime(&root)?)
            } else {
                None
            };
            let output = fresh_output(&output)?;
            let mut report = build::run(&root, &output, requested)?;
            if let Some(config) = runtime_config {
                fs::copy(
                    root.join("config/runtime.json"),
                    output.join("config/runtime.json"),
                )?;
                runtime::run(&root, &output, &mut report, config)?;
            }
            report.write(&output)?;
            Ok(report.exit_code())
        }
        Commands::Runtime { build, output } => {
            let config = config::load_runtime(&root)?;
            let build = build.canonicalize()?;
            let mut report: Report = serde_json::from_slice(&fs::read(build.join("run.json"))?)?;
            report.validate()?;
            ensure!(
                report.stages["runtime"].status == report::Status::NotRun,
                "build evidence already requested runtime; use a fresh build"
            );
            let output = fresh_output(&output)?;
            ensure!(
                !output.starts_with(&build) && !build.starts_with(&output),
                "build/output directories overlap"
            );
            copy_evidence(&build, &output.join("build-evidence"))?;
            for command in &mut report.commands {
                rebase(command);
            }
            for stage in report.stages.values_mut() {
                if let Some(command) = &mut stage.command {
                    rebase(command);
                }
            }
            if let Some(artifact) = &mut report.artifact {
                artifact.path = format!("build-evidence/{}", artifact.path);
                rebase(&mut artifact.inspection);
            }
            report.run.id = format!("pulse-{}", uuid::Uuid::new_v4().simple());
            report.run.timestamp_utc = chrono::Utc::now().to_rfc3339();
            report.run.cli_version = env!("CARGO_PKG_VERSION").into();
            report.run.ci_run_id = std::env::var("GITHUB_RUN_ID").ok();
            report.run.ci_run_attempt = std::env::var("GITHUB_RUN_ATTEMPT").ok();
            fs::create_dir(output.join("config"))?;
            fs::copy(
                root.join("config/runtime.json"),
                output.join("config/runtime.json"),
            )?;
            runtime::run(&root, &output, &mut report, config)?;
            report.write(&output)?;
            Ok(report.exit_code())
        }
        Commands::Report { input, output } => {
            let report: Report = serde_json::from_slice(&fs::read(input)?)?;
            fs::write(output, report.markdown()?)?;
            Ok(0)
        }
    }
}
fn main() {
    let code = match execute(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("ohos-pulse: {error:#}");
            2
        }
    };
    std::process::exit(code);
}
