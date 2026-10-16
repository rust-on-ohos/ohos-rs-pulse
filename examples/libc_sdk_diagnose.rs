//! Offline classification and compile-only probes for a failed upstream ctest build.
//! No patches, skip rules, guest execution or compatibility claims are produced.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    build: PathBuf,
    #[arg(long)]
    output: PathBuf,
    /// Read-only copy of the guest libc, accompanied by independently captured identity.
    #[arg(long)]
    guest_libc: PathBuf,
    #[arg(long)]
    guest_identity: PathBuf,
}

#[derive(Debug, Serialize)]
struct Diagnostic {
    file: String,
    line: usize,
    column: usize,
    message: String,
    generated_check: Option<String>,
}

#[derive(Debug, Serialize)]
struct Probe {
    source: String,
    command: Vec<String>,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Serialize)]
struct Group {
    category: String,
    subject: String,
    diagnostics: Vec<Diagnostic>,
    sdk_header_mentions: Vec<String>,
    probe: Option<Probe>,
    probe_reproduces_reported_error: Option<bool>,
    sdk_libc_export: Option<bool>,
    guest_libc_export: Option<bool>,
}

fn quoted(message: &str) -> Vec<&str> {
    message
        .split('\'')
        .enumerate()
        .filter_map(|(i, s)| (i % 2 == 1).then_some(s))
        .collect()
}

fn classify(message: &str, check: Option<&str>) -> (String, String) {
    let q = quoted(message);
    if q.is_empty() {
        return ("unclassified".into(), message.into());
    }
    if message.starts_with("use of undeclared identifier") {
        return (
            if check.is_some_and(|c| c.starts_with("ctest_foreign_fn__")) {
                "missing-function-declaration"
            } else {
                "missing-constant"
            }
            .into(),
            q[0].into(),
        );
    }
    if message.starts_with("redefinition of") {
        return ("header-conflict".into(), q[0].into());
    }
    if (message.contains("incomplete") || message.contains("will not be visible"))
        && let Some(name) = q.iter().find_map(|s| s.strip_prefix("struct "))
    {
        return ("missing-struct".into(), name.into());
    }
    if message.starts_with("no member named") && q.len() >= 2 {
        return (
            "field-difference".into(),
            format!("{}.{}", q[1].trim_start_matches("struct "), q[0]),
        );
    }
    if let Some(field) = q.iter().find_map(|s| s.strip_prefix("ctest_field_ty__")) {
        return ("field-type-difference".into(), field.replace("__", "."));
    }
    ("unclassified".into(), message.into())
}

fn parse(log: &str, generated: &str) -> Result<BTreeMap<(String, String), Group>> {
    let lines: Vec<_> = generated.lines().collect();
    let mut groups = BTreeMap::new();
    // cc prints a prefixed copy and then repeats diagnostics in the fatal error body.
    // Select one channel only, without deduplicating genuinely repeated errors.
    for line in log.lines().filter(|l| l.starts_with("warning: libc-test@")) {
        let Some((location, message)) = line.split_once(": error: ") else {
            continue;
        };
        let location = location
            .strip_prefix("warning: libc-test@")
            .context("diagnostic prefix")?
            .split_once(": ")
            .context("cc package prefix")?
            .1;
        let mut parts = location.rsplitn(3, ':');
        let column = parts.next().context("diagnostic column")?.parse()?;
        let number: usize = parts.next().context("diagnostic line")?.parse()?;
        let file = parts.next().context("diagnostic filename")?;
        let check = if file.ends_with("/ctest_output.c") {
            ensure!(
                number > 0 && number <= lines.len(),
                "diagnostic exceeds generated source"
            );
            lines[..number].iter().rev().find_map(|line| {
                if !line.starts_with("CTEST_EXTERN ") || !line.contains('{') {
                    return None;
                }
                line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .find(|s| s.starts_with("ctest_") && s.contains("__"))
                    .map(str::to_owned)
            })
        } else {
            None
        };
        let (category, subject) = classify(message, check.as_deref());
        let group = groups
            .entry((category.clone(), subject.clone()))
            .or_insert_with(|| Group {
                category,
                subject,
                diagnostics: vec![],
                sdk_header_mentions: vec![],
                probe: None,
                probe_reproduces_reported_error: None,
                sdk_libc_export: None,
                guest_libc_export: None,
            });
        group.diagnostics.push(Diagnostic {
            file: file.into(),
            line: number,
            column,
            message: message.into(),
            generated_check: check,
        });
    }
    ensure!(!groups.is_empty(), "no cc-channel errors found");
    Ok(groups)
}

fn headers(path: &Path, root: &Path, output: &mut Vec<(String, String)>) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            headers(&entry.path(), root, output)?;
        } else if kind.is_file() && entry.path().extension().is_some_and(|e| e == "h") {
            output.push((
                entry
                    .path()
                    .strip_prefix(root)?
                    .to_string_lossy()
                    .into_owned(),
                fs::read_to_string(entry.path())?,
            ));
        }
    }
    Ok(())
}

fn mentions(content: &str, name: &str) -> bool {
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    content.match_indices(name).any(|(index, _)| {
        (index == 0 || !word(content.as_bytes()[index - 1]))
            && content
                .as_bytes()
                .get(index + name.len())
                .is_none_or(|b| !word(*b))
    })
}

fn kernel_header(name: &str) -> Option<&'static str> {
    let mappings = [
        ("CAN_ERR_", "linux/can/error.h"),
        ("CAN_ERROR_", "linux/can/error.h"),
        ("CAN_BUS_OFF_", "linux/can/error.h"),
        ("CAN_CTRLMODE_", "linux/can/netlink.h"),
        ("CAN_RAW_", "linux/can/raw.h"),
        ("CAN", "linux/can.h"),
        ("J1939_", "linux/can/j1939.h"),
        ("FUTEX_", "linux/futex.h"),
        ("PACKET_", "linux/if_packet.h"),
        ("MOVE_MOUNT_", "linux/mount.h"),
        ("MOUNT_ATTR_", "linux/mount.h"),
        ("TLS_", "linux/tls.h"),
        ("MADV_", "sys/mman.h"),
        ("IFF_", "linux/if_tun.h"),
        ("TUN_", "linux/if_tun.h"),
        ("MPOL_", "linux/mempolicy.h"),
        ("SECCOMP_", "linux/seccomp.h"),
        ("CLOSE_RANGE_", "linux/close_range.h"),
        ("RESOLVE_", "linux/openat2.h"),
        ("NFNL_", "linux/netfilter/nfnetlink.h"),
        ("NFQA_", "linux/netfilter/nfnetlink_queue.h"),
        ("NF_NETDEV_", "linux/netfilter.h"),
        ("NLM_", "linux/netlink.h"),
        ("RTNLGRP_", "linux/rtnetlink.h"),
        ("SOF_", "linux/net_tstamp.h"),
        ("MFD_", "linux/memfd.h"),
        ("ALG_", "linux/if_alg.h"),
        ("XDP_", "linux/if_xdp.h"),
    ];
    mappings
        .iter()
        .find_map(|(prefix, header)| name.starts_with(prefix).then_some(*header))
}

fn function_header(name: &str) -> Option<&'static str> {
    match name {
        n if n.starts_with("pthread_") => Some("pthread.h"),
        n if n.starts_with("posix_spawn_") => Some("spawn.h"),
        "getspent" | "getspnam" | "setspent" | "endspent" => Some("shadow.h"),
        "getutxent" | "getutxid" | "getutxline" | "pututxline" | "setutxent" | "endutxent"
        | "utmpxname" => Some("utmpx.h"),
        "addmntent" => Some("mntent.h"),
        "adjtime" => Some("sys/time.h"),
        "explicit_bzero" => Some("string.h"),
        "euidaccess" | "gethostid" | "vhangup" => Some("unistd.h"),
        _ => None,
    }
}

fn probe_source(group: &Group) -> Option<String> {
    let name = &group.subject;
    let body = match group.category.as_str() {
        "missing-function-declaration" => format!(
            "#include <{}>\nvoid probe(void) {{ (void)&{name}; }}\n",
            function_header(name)?
        ),
        "missing-constant" => format!(
            "#include <{}>\nvoid probe(void) {{ (void){name}; }}\n",
            kernel_header(name)?
        ),
        "missing-struct" => {
            let header = match name.as_str() {
                "canxl_frame" => "linux/can.h",
                "futex_waitv" => "linux/futex.h",
                "mount_attr" => "linux/mount.h",
                n if n.starts_with("tls12_") => "linux/tls.h",
                _ => return None,
            };
            format!(
                "#include <{header}>\n_Static_assert(sizeof(struct {name}) > 0, \"complete type\");\n"
            )
        }
        "header-conflict" if name == "__packed" => "#include <linux/if_pppox.h>\n".into(),
        "header-conflict" if name == "sysinfo" => {
            "#include <linux/kernel.h>\n#include <sys/sysinfo.h>\n".into()
        }
        "field-difference" | "field-type-difference" => {
            let (structure, field) = name.split_once('.')?;
            let header = match structure {
                "can_frame" => "linux/can.h",
                "ptp_clock_caps" => "linux/ptp_clock.h",
                "sysinfo" => "sys/sysinfo.h",
                _ => return None,
            };
            match (structure, field) {
                ("ptp_clock_caps", "rsv") => format!(
                    "#include <{header}>\n_Static_assert(sizeof(((struct {structure} *)0)->rsv) == 11 * sizeof(int), \"Rust reserved array length\");\n"
                ),
                ("sysinfo", "uptime") => format!(
                    "#include <{header}>\n_Static_assert(_Generic(((struct sysinfo *)0)->uptime, unsigned long: 1, default: 0), \"Rust uptime type\");\n"
                ),
                _ => format!(
                    "#include <{header}>\nvoid probe(void) {{ (void)offsetof(struct {structure}, {field}); }}\n"
                ),
            }
        }
        _ => return None,
    };
    Some(format!("#define _GNU_SOURCE\n#include <stddef.h>\n{body}"))
}

fn run_probe(
    output: &Path,
    stem: &str,
    source: &str,
    clang: &str,
    flags: &[String],
) -> Result<Probe> {
    let path = format!("probes/{stem}.c");
    fs::write(output.join(&path), source)?;
    let mut command = vec![clang.to_owned()];
    command.extend_from_slice(flags);
    command.extend([
        "-Werror".into(),
        "-ferror-limit=0".into(),
        "-fsyntax-only".into(),
        output.join(&path).to_string_lossy().into_owned(),
    ]);
    let result = Command::new(&command[0]).args(&command[1..]).output()?;
    let stdout = format!("probes/{stem}.stdout.log");
    let stderr = format!("probes/{stem}.stderr.log");
    fs::write(output.join(&stdout), result.stdout)?;
    fs::write(output.join(&stderr), result.stderr)?;
    Ok(Probe {
        source: path,
        command,
        exit_code: result.status.code(),
        stdout,
        stderr,
    })
}

fn exports(readelf: &str, path: &Path, output: &Path, name: &str) -> Result<BTreeSet<String>> {
    let result = Command::new(readelf)
        .args(["--dyn-syms", "--wide"])
        .arg(path)
        .output()?;
    fs::write(
        output.join(format!("{name}-symbols.stderr.log")),
        &result.stderr,
    )?;
    ensure!(
        result.status.success(),
        "cannot inspect {name} dynamic symbols"
    );
    fs::write(
        output.join(format!("{name}-symbols.stdout.log")),
        &result.stdout,
    )?;
    let symbols = parse_exports(&String::from_utf8_lossy(&result.stdout));
    ensure!(!symbols.is_empty(), "empty {name} export inventory");
    Ok(symbols)
}

fn parse_exports(table: &str) -> BTreeSet<String> {
    let mut symbols = BTreeSet::new();
    for line in table.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() >= 8
            && matches!(fields[4], "GLOBAL" | "WEAK")
            && matches!(fields[5], "DEFAULT" | "PROTECTED")
            && fields[6] != "UND"
        {
            symbols.insert(fields[7].split('@').next().unwrap().to_owned());
        }
    }
    symbols
}

fn hash(path: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        !args.output.exists(),
        "choose a fresh diagnostic output directory"
    );
    let build = args.build.canonicalize()?;
    fs::create_dir_all(&args.output)?;
    let output = args.output.canonicalize()?;
    ensure!(
        !output.starts_with(&build) && !build.starts_with(&output),
        "build/output overlap"
    );
    fs::copy(&args.guest_libc, output.join("guest-libc.so"))?;
    fs::copy(&args.guest_identity, output.join("guest-identity.log"))?;
    let driver = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/libc_sdk_diagnose.rs");
    fs::copy(&driver, output.join("diagnostic-driver.rs"))?;
    let input: Value = serde_json::from_slice(&fs::read(build.join("upstream.json"))?)?;
    ensure!(
        input["build_pins"]["target"] == "aarch64-unknown-linux-ohos"
            && input["build_pins"]["sdk_api_version"] == 26,
        "only reviewed ARM64 API 26 evidence supported"
    );
    let env = &input["environment"];
    let clang = env["toolchain"]["clang"]
        .as_str()
        .context("missing clang")?;
    let readelf = env["toolchain"]["readelf"]
        .as_str()
        .context("missing readelf")?;
    let sysroot = Path::new(env["sdk"]["sysroot"].as_str().context("missing sysroot")?);
    let native = Path::new(
        env["sdk"]["native_root"]
            .as_str()
            .context("missing native root")?,
    );
    let package: Value = serde_json::from_slice(&fs::read(native.join("oh-uni-package.json"))?)?;
    ensure!(
        package["version"] == input["build_pins"]["sdk_version"],
        "installed SDK changed"
    );
    let flags: Vec<String> = serde_json::from_value(env["flags"]["cflags"].clone())?;
    ensure!(
        flags.contains(&"--target=aarch64-linux-ohos".into())
            && flags.contains(&format!("--sysroot={}", sysroot.display())),
        "unreviewed C flags"
    );
    let log_path = input["phases"]["test_build"]["command"]["stderr"]
        .as_str()
        .context("missing compiler log")?;
    ensure!(
        Path::new(log_path)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_))),
        "unsafe compiler log path"
    );
    let generated_path = build.join("generated/ctest_output.c");
    let mut groups = parse(
        &fs::read_to_string(build.join(log_path))?,
        &fs::read_to_string(&generated_path)?,
    )?;
    fs::create_dir(output.join("probes"))?;
    let mut sdk_headers = vec![];
    let include = sysroot.join("usr/include");
    headers(&include, &include, &mut sdk_headers)?;
    let sdk_symbols = exports(
        readelf,
        &sysroot.join("usr/lib/aarch64-linux-ohos/libc.so"),
        &output,
        "sdk-libc",
    )?;
    let guest_symbols = exports(readelf, &args.guest_libc, &output, "guest-libc")?;
    for (index, group) in groups.values_mut().enumerate() {
        let token = group.subject.split('.').next().unwrap();
        group.sdk_header_mentions = sdk_headers
            .iter()
            .filter(|(_, text)| mentions(text, token))
            .map(|(name, _)| name.clone())
            .collect();
        group.sdk_header_mentions.sort();
        if let Some(source) = probe_source(group) {
            group.probe = Some(run_probe(
                &output,
                &format!("{index:03}"),
                &source,
                clang,
                &flags,
            )?);
            let probe = group.probe.as_ref().unwrap();
            let stderr = fs::read_to_string(output.join(&probe.stderr))?;
            let marker = match group.category.as_str() {
                "missing-function-declaration" | "missing-constant" => {
                    format!("use of undeclared identifier '{}'", group.subject)
                }
                "missing-struct" => format!("incomplete type 'struct {}'", group.subject),
                "header-conflict" => format!("redefinition of '{}'", group.subject),
                "field-difference" => format!(
                    "no member named '{}'",
                    group.subject.split_once('.').unwrap().1
                ),
                "field-type-difference" => "static assertion failed".into(),
                _ => String::new(),
            };
            group.probe_reproduces_reported_error =
                Some(probe.exit_code == Some(1) && !marker.is_empty() && stderr.contains(&marker));
        }
        if group.category == "missing-function-declaration" {
            group.sdk_libc_export = Some(sdk_symbols.contains(&group.subject));
            group.guest_libc_export = Some(guest_symbols.contains(&group.subject));
        }
    }
    let controls = [
        ("packed-defined", "#include <stddef.h>\n#define __packed __attribute__((__packed__))\n#include <linux/if_pppox.h>\n_Static_assert(_Alignof(struct sockaddr_pppox) == 1, \"packed alignment\");\n"),
        ("sysinfo-libc", "#include <sys/sysinfo.h>\n_Static_assert(sizeof(((struct sysinfo *)0)->__reserved) == 256, \"libc reserved field\");\n_Static_assert(_Generic(((struct sysinfo *)0)->uptime, unsigned long: 1, default: 0), \"libc uptime\");\n"),
        ("legacy-layout-sizes", "#include <linux/can.h>\n#include <linux/ptp_clock.h>\n_Static_assert(sizeof(struct can_frame) == 16, \"CAN frame size\");\n_Static_assert(sizeof(struct ptp_clock_caps) == 80, \"PTP caps size\");\n"),
    ].into_iter().map(|(stem, source)| run_probe(&output, stem, source, clang, &flags)).collect::<Result<Vec<_>>>()?;
    let mut link_probes = vec![];
    for name in ["explicit_bzero", "pthread_tryjoin_np"] {
        // Synthetic declaration to isolate symbol resolution; never execute this program.
        let source = format!(
            "extern void {name}(void);\nvoid (*volatile probe)(void) = {name};\nint main(void) {{ return probe == 0; }}\n"
        );
        let stem = format!("symbol-link-{name}");
        let path = format!("probes/{stem}.c");
        fs::write(output.join(&path), source)?;
        let mut command = vec![clang.to_owned()];
        command.extend_from_slice(&flags);
        command.extend([
            "-fuse-ld=lld".into(),
            output.join(&path).to_string_lossy().into_owned(),
            "-o".into(),
            output
                .join(format!("probes/{stem}"))
                .to_string_lossy()
                .into_owned(),
        ]);
        let result = Command::new(&command[0]).args(&command[1..]).output()?;
        let stdout = format!("probes/{stem}.stdout.log");
        let stderr = format!("probes/{stem}.stderr.log");
        fs::write(output.join(&stdout), result.stdout)?;
        fs::write(output.join(&stderr), result.stderr)?;
        link_probes.push(Probe {
            source: path,
            command,
            exit_code: result.status.code(),
            stdout,
            stderr,
        });
    }
    let mut counts = BTreeMap::<String, usize>::new();
    for group in groups.values() {
        *counts.entry(group.category.clone()).or_default() += group.diagnostics.len();
    }
    let mut summary = String::from(
        "# ARM64 SDK 26 full-target diagnostics\n\nCounts refer to compiler diagnostics and grouped subjects, not independent defects.\nThis report inventories the current C compilation failure only; later Rust/link/runtime blockers remain unknown.\n\n| Category | Diagnostics | Subjects |\n|---|---:|---:|\n",
    );
    for (category, count) in &counts {
        summary.push_str(&format!(
            "| {category} | {count} | {} |\n",
            groups.values().filter(|g| &g.category == category).count()
        ));
    }
    summary.push_str("\n| Subject | Category | Diagnostics | Isolated C exit | SDK export | Guest export |\n|---|---|---:|---|---|---|\n");
    for group in groups.values() {
        summary.push_str(&format!(
            "| {} | {} | {} | {:?} | {:?} | {:?} |\n",
            group.subject,
            group.category,
            group.diagnostics.len(),
            group.probe.as_ref().and_then(|p| p.exit_code),
            group.sdk_libc_export,
            group.guest_libc_export
        ));
    }
    summary.push_str("\nHeader mentions are lexical evidence, not declarations. Probes check declaration/type availability only; export presence does not establish signature compatibility or behavior. Missing symbols refer to these two libc files, not every guest library or kernel capability.\n");
    let report = serde_json::json!({
        "format": "ohos-pulse-sdk-diagnostics-1", "build_run": input["run"],
        "build_pins": input["build_pins"], "build_report_sha256": hash(&build.join("upstream.json"))?,
        "compiler_log_sha256": hash(&build.join(log_path))?, "generated_c_sha256": hash(&generated_path)?,
        "sdk_libc_sha256": hash(&sysroot.join("usr/lib/aarch64-linux-ohos/libc.so"))?,
        "guest_libc_sha256": hash(&args.guest_libc)?, "category_counts": counts,
        "guest_identity": fs::read_to_string(&args.guest_identity)?,
        "diagnostic_driver_sha256": hash(&driver)?, "controls": controls, "symbol_link_probes": link_probes,
        "groups": groups.values().collect::<Vec<_>>(),
        "scope": "Current C diagnostics and compile-time header checks; no source/header changes or guest test execution. Guest libc identity must be established by the acquisition record."
    });
    fs::write(
        output.join("diagnostics.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    fs::write(output.join("summary.md"), &summary)?;
    println!(
        "{} diagnostics in {} grouped subjects; report: {}",
        counts.values().sum::<usize>(),
        groups.len(),
        output.join("summary.md").display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_cascades_without_double_counting_cc_repetition() {
        let source = "CTEST_EXTERN int ctest_size_of__foo(void) { return sizeof(struct foo); }";
        let error = "/out/ctest_output.c:1:65: error: invalid application of 'sizeof' to an incomplete type 'struct foo'";
        let groups = parse(
            &format!("warning: libc-test@0.1.0: {error}\n{error}\n"),
            source,
        )
        .unwrap();
        assert_eq!(groups.len(), 1);
        let group = groups.values().next().unwrap();
        assert_eq!(group.category, "missing-struct");
        assert_eq!(group.subject, "foo");
        assert_eq!(group.diagnostics.len(), 1);
    }
    #[test]
    fn separates_functions_constants_and_field_types() {
        assert_eq!(
            classify(
                "use of undeclared identifier 'f'",
                Some("ctest_foreign_fn__f")
            )
            .0,
            "missing-function-declaration"
        );
        assert_eq!(
            classify("use of undeclared identifier 'C'", Some("ctest_const__C")).0,
            "missing-constant"
        );
        assert_eq!(
            classify(
                "incompatible result type 'ctest_field_ty__ptp_clock_caps__rsv'",
                None
            )
            .1,
            "ptp_clock_caps.rsv"
        );
        assert_eq!(classify("unknown diagnostic", None).0, "unclassified");
        assert_eq!(
            classify("use of undeclared identifier", None).0,
            "unclassified"
        );
    }

    #[test]
    fn header_search_matches_identifier_boundaries() {
        assert!(mentions("int adjtime(void);", "adjtime"));
        assert!(!mentions("int adjtimex(void);", "adjtime"));
        assert!(!mentions("int __adjtime64(void);", "adjtime"));
    }

    #[test]
    fn exports_require_definition_and_external_visibility() {
        let symbols = parse_exports(
            "\
1: 0010 4 FUNC GLOBAL DEFAULT 12 visible@@V1\n\
2: 0020 4 FUNC WEAK PROTECTED 12 protected\n\
3: 0030 4 FUNC GLOBAL HIDDEN 12 hidden\n\
4: 0040 4 FUNC GLOBAL INTERNAL 12 internal\n\
5: 0000 0 FUNC GLOBAL DEFAULT UND missing\n\
6: 0050 4 FUNC LOCAL DEFAULT 12 local\n",
        );
        assert_eq!(
            symbols,
            BTreeSet::from(["visible".into(), "protected".into()])
        );
    }
}
