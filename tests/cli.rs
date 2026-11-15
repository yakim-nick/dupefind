//! End-to-end tests that run the compiled binary.

use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn dupefind() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dupefind"))
}

#[test]
fn reports_duplicates_in_text_form() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("a.txt"), "hello world").unwrap();
    fs::write(tmp.path().join("b.txt"), "hello world").unwrap();
    fs::write(tmp.path().join("c.txt"), "something else").unwrap();

    let out = dupefind().arg(tmp.path()).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert!(
        stdout.contains("Group 1: 2 files, 11 bytes each (11 bytes wasted)"),
        "{stdout}"
    );
    assert!(stdout.contains("1 duplicate group(s), 11 bytes reclaimable, 3 file(s) scanned"));
}

#[test]
fn json_output_is_valid_and_complete() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("a"), "dup").unwrap();
    fs::write(tmp.path().join("b"), "dup").unwrap();

    let out = dupefind().arg("--json").arg(tmp.path()).output().unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["groups"].as_array().unwrap().len(), 1);
    assert_eq!(json["groups"][0]["size"], 3);
    assert_eq!(json["wasted_bytes"], 3);
}

#[test]
fn fail_on_duplicates_sets_exit_status() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("a"), "dup").unwrap();

    let ok = dupefind()
        .arg("--fail-on-duplicates")
        .arg(tmp.path())
        .status()
        .unwrap();
    assert!(ok.success(), "no duplicates yet");

    fs::write(tmp.path().join("b"), "dup").unwrap();
    let bad = dupefind()
        .arg("--fail-on-duplicates")
        .arg(tmp.path())
        .status()
        .unwrap();
    assert_eq!(bad.code(), Some(1));
}

#[test]
fn requires_at_least_one_path() {
    let out = dupefind().output().unwrap();
    assert!(!out.status.success());
}
