use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn invoke(directory: &Path, id: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_libc-file-io"))
        .arg(directory)
        .arg(id)
        .output()
        .unwrap()
}

#[test]
fn host_round_trip_and_failure_contract() {
    let directory = tempfile::tempdir().unwrap();
    let result = invoke(directory.path(), "host-contract");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let record: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        record,
        serde_json::json!({"scenario_id":"file-round-trip","run_id":"host-contract","status":"passed"})
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    let existing = directory.path().join("ohos-pulse-collision.tmp");
    fs::write(&existing, b"preserve existing content").unwrap();
    let collision = invoke(directory.path(), "collision");
    assert!(!collision.status.success());
    assert!(collision.stdout.is_empty());
    assert_eq!(fs::read(existing).unwrap(), b"preserve existing content");
    for id in ["../invalid", ""] {
        let result = invoke(directory.path(), id);
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
    }
    let result = invoke(&directory.path().join("missing"), "missing-directory");
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
}
