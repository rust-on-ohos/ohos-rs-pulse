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

impl Harness {
    fn upstream_source(&self) -> PathBuf {
        use sha2::{Digest, Sha256};
        let source = self.temp.path().join("upstream");
        fs::create_dir_all(source.join("libc-test/build")).unwrap();
        fs::create_dir_all(source.join("libc-test/tests")).unwrap();
        for name in [
            "Cargo.toml",
            "build/main.rs",
            "build/target.rs",
            "tests/ctest.rs",
        ] {
            fs::write(source.join("libc-test").join(name), "test-double input").unwrap();
        }
        fs::write(source.join("Cargo.lock"), "test-double lockfile").unwrap();
        let mut config: Value = serde_json::from_slice(
            &fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("config/upstream-libc.json"))
                .unwrap(),
        )
        .unwrap();
        config["lockfile_sha256"] = json!(format!("{:x}", Sha256::digest(b"test-double lockfile")));
        config["execution_timeout_seconds"] = json!(1);
        fs::write(
            self.root.join("config/upstream-libc.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let generator = self.temp.path().join("generator");
        fs::create_dir_all(generator.join("templates")).unwrap();
        fs::write(generator.join("Cargo.toml"), "test-double manifest").unwrap();
        fs::write(
            generator.join("templates/test.rs"),
            include_bytes!("data/ctest-0.5.1/template.rs"),
        )
        .unwrap();
        let template = include_str!("data/ctest-0.5.1/template.rs");
        let start = template.find("fn main() {").unwrap();
        let end = start + template[start..].find("\n}").unwrap() + 2;
        let generated = format!(
            "mod generated_tests {{\npub fn check_type() {{}}\n}}\n{}\nfn run_all() {{\ncheck_type();\n}}\n",
            &template[start..end]
        );
        fs::write(self.temp.path().join("generated.rs"), generated).unwrap();
        executable(
            &self.tools.join("git"),
            r#"
case "$1" in
 rev-parse) echo 7b0ab5528dc7f361f3a0b4c06c2cad9971617f75;;
 status) if [ "$PULSE_TEST_MODE" = dirty-source ]; then echo ' M Cargo.lock'; fi;;
 ls-files) echo '100644 abc 0 Cargo.lock';;
 archive) : > "$4";;
 *) exit 99;;
esac
"#,
        );
        executable(&self.tools.join("clang"), "echo test-clang");
        executable(
            &self.tools.join("sha256sum"),
            r#"
if [ "$PULSE_TEST_MODE" = digest-mismatch ]; then echo 'bad-digest file'; else shasum -a 256 "$@"; fi
"#,
        );
        let sysroot = self.temp.path().join("sysroot");
        fs::create_dir(&sysroot).unwrap();
        let environment = json!({"cargo_ohos_version":"0.3.4", "sdk":{"version":"6.1.0.31","api_version":23,"sysroot":sysroot},"toolchain":{"readelf":self.tools.join("readelf"),"clang":self.tools.join("clang")}, "target":{"rust_triple":"x86_64-unknown-linux-ohos"}, "env":{"CC_x86_64_unknown_linux_ohos":self.tools.join("clang")}, "flags":{"cflags":["--target=x86_64-linux-ohos",format!("--sysroot={}",sysroot.display()),"-D__MUSL__"]}});
        fs::write(
            self.temp.path().join("environment.json"),
            serde_json::to_vec(&environment).unwrap(),
        )
        .unwrap();
        let metadata = json!({"packages":[
            {"name":"libc","version":"0.2.190","source":null,"id":"source-libc"},
            {"name":"libc-test","source":null,"id":"source-libc-test","targets":[{"name":"ctest","kind":["test"]}]},
            {"name":"ctest","version":"0.5.1","source":"registry+https://github.com/rust-lang/crates.io-index","id":"generator","manifest_path":generator.join("Cargo.toml")}
        ],"resolve":{"nodes":[{"id":"source-libc","features":["std"]}]}});
        fs::write(
            self.temp.path().join("metadata.json"),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        executable(
            &self.tools.join("cargo"),
            r#"
case "$1 $2" in
 '--version ') echo 'cargo 1.99.0';;
 'metadata --locked') cat "$PULSE_TEST_METADATA";;
 'ohos env') cat "$PULSE_TEST_ENV";;
 'ohos test')
   case " $* " in *' --no-run '*) :;; *) exit 98;; esac
   while [ "$1" != --target-dir ]; do shift; done
   base="$2/x86_64-unknown-linux-ohos/debug"
   mkdir -p "$base/build/libc-test-test/out" "$base/deps"
   cp "$PULSE_TEST_GENERATED" "$base/build/libc-test-test/out/ctest_output.rs"
   echo '/* generated C test double */' > "$base/build/libc-test-test/out/ctest_output.c"
   if [ "$PULSE_TEST_MODE" = build-failed ]; then echo upstream-compile-failed >&2; exit 17; fi
   cat > "$base/deps/ctest-test" <<'PROGRAM'
#!/bin/sh
case "$PULSE_TEST_MODE" in
 nonzero) echo 'bad ABI check' >&2; exit 37;;
 timeout) sleep 20; exit 0;;
 zero-count) printf 'RUNNING ALL TESTS\nPASSED 0 tests\n';;
 missing-summary) echo 'RUNNING ALL TESTS';;
 *) printf 'RUNNING ALL TESTS\nPASSED 2 tests\n';;
esac
PROGRAM
   chmod 700 "$base/deps/ctest-test"
   printf '{"reason":"compiler-artifact","package_id":"source-libc-test","target":{"name":"ctest","kind":["test"]},"profile":{"test":true},"fresh":false,"executable":"%s/deps/ctest-test"}\n' "$base";;
 *) exit 99;;
esac
"#,
        );
        source
    }
    fn upstream_command(&self, mode: &str) -> Command {
        let mut command = self.command(mode);
        command
            .env(
                "PULSE_TEST_METADATA",
                self.temp.path().join("metadata.json"),
            )
            .env(
                "PULSE_TEST_GENERATED",
                self.temp.path().join("generated.rs"),
            );
        command
    }
    fn upstream_build(&self, source: &Path, mode: &str) -> Output {
        self.upstream_command(mode)
            .args(["upstream-build", "--source"])
            .arg(source)
            .arg("--output")
            .arg(self.temp.path().join("upstream-build"))
            .output()
            .unwrap()
    }
    fn upstream_runtime(&self, mode: &str) -> Output {
        self.upstream_command(mode)
            .args(["upstream-runtime", "--build"])
            .arg(self.temp.path().join("upstream-build"))
            .arg("--output")
            .arg(self.temp.path().join("upstream-runtime"))
            .output()
            .unwrap()
    }
    fn upstream_report(&self, directory: &str) -> Value {
        serde_json::from_slice(
            &fs::read(self.temp.path().join(directory).join("upstream.json")).unwrap(),
        )
        .unwrap()
    }
}

#[test]
fn upstream_build_failure_retains_generated_sources_and_blocks_runtime() {
    let h = Harness::new();
    let source = h.upstream_source();
    let result = h.upstream_build(&source, "build-failed");
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report = h.upstream_report("upstream-build");
    assert_eq!(report["phases"]["test_build"]["status"], "failed");
    assert!(
        h.temp
            .path()
            .join("upstream-build/generated/ctest_output.rs")
            .exists()
    );
    assert!(!h.temp.path().join("upstream-build/target").exists());
    assert_eq!(h.upstream_runtime("").status.code(), Some(1));
    assert_eq!(
        h.upstream_report("upstream-runtime")["phases"]["execution"]["status"],
        "blocked"
    );
}

#[test]
fn upstream_dirty_source_and_changed_harness_cannot_build_a_passing_result() {
    for mode in ["dirty-source", "changed-harness"] {
        let h = Harness::new();
        let source = h.upstream_source();
        if mode == "changed-harness" {
            let path = h.temp.path().join("generated.rs");
            let text = fs::read_to_string(&path).unwrap().replace("run_all();", "");
            fs::write(path, text).unwrap();
        }
        let result = h.upstream_build(&source, mode);
        assert_eq!(result.status.code(), Some(2));
        let report = h.upstream_report("upstream-build");
        assert!(report["artifact"].is_null());
    }
}

#[test]
fn upstream_runtime_uses_custom_harness_and_preserves_failure_boundaries() {
    for (mode, status, code) in [
        ("", "passed", 0),
        ("zero-count", "failed", 1),
        ("missing-summary", "failed", 1),
        ("nonzero", "failed", 1),
        ("receive-failed", "infra_error", 2),
        ("digest-mismatch", "infra_error", 2),
        ("cleanup-failed", "infra_error", 2),
        ("timeout", "timed_out", 1),
    ] {
        let h = Harness::new();
        let source = h.upstream_source();
        let built = h.upstream_build(&source, "");
        assert!(
            built.status.success(),
            "build: {}",
            String::from_utf8_lossy(&built.stderr)
        );
        let result = h.upstream_runtime(mode);
        assert_eq!(
            result.status.code(),
            Some(code),
            "mode {mode}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report = h.upstream_report("upstream-runtime");
        assert_eq!(
            report["phases"]["execution"]["status"], status,
            "mode {mode}: {report}"
        );
        assert_eq!(report["coverage"]["generated_function_count"], 1);
        if mode.is_empty() || mode == "cleanup-failed" {
            assert_eq!(report["coverage"]["successful_comparisons"], 2);
        }
        if mode == "nonzero" {
            assert_eq!(report["phases"]["execution"]["guest_exit_code"], 37);
        }
        if mode == "cleanup-failed" {
            assert_eq!(report["phases"]["execution"]["guest_exit_code"], 0);
        }
    }
}

#[test]
fn upstream_rejects_host_c_compiler_configuration_before_build() {
    let h = Harness::new();
    let source = h.upstream_source();
    let path = h.temp.path().join("environment.json");
    let mut environment: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    environment["env"]["CC_x86_64_unknown_linux_ohos"] = json!("/usr/bin/cc");
    fs::write(path, serde_json::to_vec(&environment).unwrap()).unwrap();
    assert_eq!(h.upstream_build(&source, "").status.code(), Some(2));
    let report = h.upstream_report("upstream-build");
    assert_eq!(report["phases"]["preparation"]["status"], "infra_error");
    assert_eq!(report["phases"]["test_build"]["status"], "blocked");
    assert!(
        !report["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "upstream-test-build")
    );
}
