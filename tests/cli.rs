//! Host command doubles exercise the CLI contract; none are OHOS runtime evidence.
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn executable(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
struct Harness {
    temp: tempfile::TempDir,
    root: PathBuf,
    tools: PathBuf,
}
impl Harness {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        let tools = temp.path().join("tools");
        fs::create_dir_all(root.join("config")).unwrap();
        fs::create_dir(&tools).unwrap();
        let actual = Path::new(env!("CARGO_MANIFEST_DIR"));
        for name in ["feasibility.json", "runtime.json"] {
            fs::copy(
                actual.join("config").join(name),
                root.join("config").join(name),
            )
            .unwrap();
        }
        let fixture = root.join("fixtures/libc-file-io");
        fs::create_dir_all(fixture.join("src")).unwrap();
        for name in ["Cargo.toml", "Cargo.lock", "src/main.rs"] {
            fs::copy(
                actual.join("fixtures/libc-file-io").join(name),
                fixture.join(name),
            )
            .unwrap();
        }
        let mut runtime: Value =
            serde_json::from_slice(&fs::read(root.join("config/runtime.json")).unwrap()).unwrap();
        runtime["execution_timeout_seconds"] = json!(1);
        fs::write(
            root.join("config/runtime.json"),
            serde_json::to_vec(&runtime).unwrap(),
        )
        .unwrap();
        executable(
            &tools.join("git"),
            "case \"$1\" in rev-parse) echo test-commit;; status|diff) :;; esac",
        );
        executable(&tools.join("rustc"), "echo 'rustc 1.99.0 (test)' ");
        executable(&tools.join("param"), "echo OpenHarmony-test");
        executable(
            &tools.join("readelf"),
            "echo 'Advanced Micro Devices X86-64 /lib/ld-musl-x86_64.so.1 NEEDED libc.so'",
        );
        executable(&tools.join("loader"), "echo 'libc.so => guest musl'");
        executable(
            &tools.join("cargo"),
            r#"
case "$1 $2" in
  '--version ') echo 'cargo 1.99.0';;
  'metadata --locked') echo '{"packages":[{"name":"libc","version":"0.2.190","id":"libc"}],"resolve":{"nodes":[{"id":"libc","features":["default","std"]}]}}';;
  'ohos env') cat "$PULSE_TEST_ENV";;
  'ohos check') if [ "$PULSE_TEST_MODE" = check-failed ]; then echo compile-failed >&2; exit 17; fi;;
  'ohos build')
    while [ "$1" != --target-dir ]; do shift; done
    dest="$2/x86_64-unknown-linux-ohos/debug"
    mkdir -p "$dest"
    cat > "$dest/libc-file-io" <<'FIXTURE'
#!/bin/sh
case "$PULSE_TEST_MODE" in
  nonzero) echo assertion-failed >&2; exit 37;;
  wrong-id) id=stale;;
  timeout) sleep 20; exit 0;;
  *) id="$2";;
esac
printf '{"scenario_id":"file-round-trip","run_id":"%s","status":"passed"}\n' "$id"
FIXTURE
    chmod 700 "$dest/libc-file-io";;
  *) echo unexpected-cargo-command >&2; exit 99;;
esac
"#,
        );
        executable(
            &tools.join("hdc"),
            r#"
if [ "$1" = -v ]; then echo test-hdc; exit 0; fi
if [ "$1" = list ]; then echo test-target; exit 0; fi
shift 2
case "$1" in
 shell)
   script=$(printf '%s' "$2" | sed "s|/data/local/tmp|$PULSE_TEST_GUEST|g; s|/lib/ld-musl-x86_64.so.1|$PULSE_TEST_TOOLS/loader|g")
   if [ "$PULSE_TEST_MODE" = cleanup-failed ]; then
     case "$script" in *'rmdir '*) echo transport-failed >&2; exit 1;; esac
   fi
   sh -c "$script";;
 file)
   if [ "$PULSE_TEST_MODE" = receive-failed ] && [ "$2" = recv ]; then exit 1; fi
   source=$(printf '%s' "$3" | sed "s|/data/local/tmp|$PULSE_TEST_GUEST|g")
   dest=$(printf '%s' "$4" | sed "s|/data/local/tmp|$PULSE_TEST_GUEST|g")
   cp "$source" "$dest";;
 *) exit 99;;
esac
"#,
        );
        let environment = json!({"cargo_ohos_version":"0.3.4", "sdk":{"version":"6.1.0.31","api_version":23},"toolchain":{"readelf":tools.join("readelf")}});
        fs::write(
            temp.path().join("environment.json"),
            serde_json::to_vec(&environment).unwrap(),
        )
        .unwrap();
        Self { temp, root, tools }
    }
    fn invoke(&self, output: &str, mode: &str, requested: bool) -> Output {
        let mut command = self.command(mode);
        command
            .args(["run", "--output"])
            .arg(self.temp.path().join(output));
        if requested {
            command.arg("--runtime");
        }
        command.output().unwrap()
    }
    fn command(&self, mode: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ohos-pulse"));
        command
            .arg("--root")
            .arg(&self.root)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.tools.display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("PULSE_TEST_ENV", self.temp.path().join("environment.json"))
            .env("PULSE_TEST_GUEST", self.temp.path().join("guest"))
            .env("PULSE_TEST_TOOLS", &self.tools)
            .env("PULSE_TEST_MODE", mode);
        command
    }
    fn report(&self, output: &str) -> Value {
        serde_json::from_slice(&fs::read(self.temp.path().join(output).join("run.json")).unwrap())
            .unwrap()
    }
}

#[test]
fn build_only_and_report_regeneration() {
    let h = Harness::new();
    let result = h.invoke("build", "", false);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report = h.report("build");
    assert_eq!(report["stages"]["runtime"]["status"], "not_run");
    assert_eq!(report["subject"]["libc_version"], "0.2.190");
    let summary = h.temp.path().join("regenerated.md");
    let status = h
        .command("")
        .args(["report", "--input"])
        .arg(h.temp.path().join("build/run.json"))
        .arg("--output")
        .arg(&summary)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read(summary).unwrap(),
        fs::read(h.temp.path().join("build/summary.md")).unwrap()
    );
    let repeated = h.invoke("build", "", false);
    assert_eq!(repeated.status.code(), Some(2));
}

#[test]
fn failed_check_blocks_link_and_requested_runtime() {
    let h = Harness::new();
    let result = h.invoke("failed", "check-failed", true);
    assert_eq!(result.status.code(), Some(1));
    let report = h.report("failed");
    assert_eq!(report["stages"]["check"]["status"], "failed");
    assert_eq!(report["stages"]["link"]["status"], "blocked");
    assert_eq!(report["stages"]["runtime"]["status"], "blocked");
    assert!(
        !report["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "link" || c["name"] == "hdc-version")
    );
}

#[test]
fn mismatched_sdk_fails_before_compilation() {
    let h = Harness::new();
    let path = h.temp.path().join("environment.json");
    let mut environment: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    environment["sdk"]["api_version"] = json!(26);
    fs::write(path, serde_json::to_vec(&environment).unwrap()).unwrap();
    assert_eq!(h.invoke("mismatch", "", false).status.code(), Some(2));
    let report = h.report("mismatch");
    assert_eq!(report["stages"]["check"]["status"], "infra_error");
    assert!(
        !report["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "check")
    );
}

#[test]
fn guest_acceptance_failure_and_transport_boundaries() {
    for (mode, expected_status, exit) in [
        ("", "passed", 0),
        ("wrong-id", "failed", 1),
        ("nonzero", "failed", 1),
        ("receive-failed", "infra_error", 2),
        ("cleanup-failed", "infra_error", 2),
        ("timeout", "timed_out", 1),
    ] {
        let h = Harness::new();
        let result = h.invoke("runtime", mode, true);
        assert_eq!(
            result.status.code(),
            Some(exit),
            "mode {mode}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report = h.report("runtime");
        assert_eq!(
            report["stages"]["runtime"]["status"], expected_status,
            "mode {mode}: {report}"
        );
        if mode.is_empty() {
            assert_eq!(report["cleanup"]["status"], "passed");
        }
        if mode == "nonzero" {
            assert_eq!(report["stages"]["runtime"]["guest_exit_code"], 37);
        }
        if mode == "cleanup-failed" {
            assert_eq!(report["stages"]["runtime"]["guest_exit_code"], 0);
            assert_eq!(report["stages"]["runtime"]["completion_matches"], true);
        }
    }
}

#[test]
fn runtime_command_keeps_portable_build_evidence() {
    let h = Harness::new();
    assert!(h.invoke("build", "", false).status.success());
    let result = h
        .command("")
        .args(["runtime", "--build"])
        .arg(h.temp.path().join("build"))
        .arg("--output")
        .arg(h.temp.path().join("runtime"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report = h.report("runtime");
    assert!(
        report["artifact"]["path"]
            .as_str()
            .unwrap()
            .starts_with("build-evidence/")
    );
    assert!(
        h.temp
            .path()
            .join("runtime/build-evidence/run.json")
            .is_file()
    );
    assert_ne!(report["run"]["id"], h.report("build")["run"]["id"]);
}

#[test]
fn report_rejects_fabricated_runtime_success_and_unsafe_paths() {
    let h = Harness::new();
    assert!(h.invoke("build", "", false).status.success());
    for mutation in ["runtime", "path", "command"] {
        let mut report = h.report("build");
        match mutation {
            "runtime" => report["stages"]["runtime"]["status"] = json!("passed"),
            "path" => report["artifact"]["path"] = json!("../outside"),
            "command" => report["stages"]["link"]["command"]["exit_code"] = json!(37),
            _ => unreachable!(),
        }
        let input = h.temp.path().join(format!("{mutation}.json"));
        fs::write(&input, serde_json::to_vec(&report).unwrap()).unwrap();
        let output = h.temp.path().join(format!("{mutation}.md"));
        let result = h
            .command("")
            .args(["report", "--input"])
            .arg(input)
            .arg("--output")
            .arg(&output)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(!output.exists());
    }
}
