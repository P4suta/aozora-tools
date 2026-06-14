//! Integration tests that shell out to the compiled `aozora-fmt` binary.

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{self, Command, Stdio};

fn aozora_fmt() -> Command {
    Command::new(env!("CARGO_BIN_EXE_aozora-fmt"))
}

#[test]
fn stdin_to_stdout_prints_canonical_form() {
    let mut child = aozora_fmt()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn aozora-fmt");
    child
        .stdin
        .as_mut()
        .expect("stdin piped")
        .write_all("日本《にほん》".as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success(), "exit: {:?}", out.status);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.starts_with('｜'),
        "expected canonical explicit delimiter, got {stdout:?}",
    );
}

#[test]
fn check_on_canonical_input_exits_zero() {
    let mut child = aozora_fmt()
        .arg("--check")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn aozora-fmt");
    child.stdin.as_mut().unwrap().write_all(b"hello\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "canonical input must pass --check");
}

#[test]
fn check_on_non_canonical_input_exits_one() {
    let mut child = aozora_fmt()
        .arg("--check")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn aozora-fmt");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all("日本《にほん》".as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "non-canonical input must fail --check",
    );
}

/// Write `contents` to a unique temp file and return its path. Each
/// test owns its file (keyed by pid + a per-test `name`, so parallel
/// tests never collide) and removes it at the end.
fn temp_file(name: &str, contents: &str) -> PathBuf {
    let mut path = env::temp_dir();
    path.push(format!("aozora-fmt-it-{}-{name}", process::id()));
    fs::write(&path, contents).expect("write temp file");
    path
}

#[test]
fn write_canonicalizes_in_place_and_is_idempotent() {
    let path = temp_file("write-roundtrip", "日本《にほん》");

    // First --write rewrites the file to canonical form and exits 0.
    let out = aozora_fmt().arg("--write").arg(&path).output().unwrap();
    assert!(
        out.status.success(),
        "first --write failed: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    let after_first = fs::read_to_string(&path).unwrap();
    assert!(
        after_first.starts_with('｜'),
        "expected canonical explicit delimiter, got {after_first:?}",
    );

    // Second --write must be a byte-identical no-op: the on-disk output
    // is a fixed point of the formatter. This exercises the happy path
    // of the pre-write idempotency guard.
    let out2 = aozora_fmt().arg("--write").arg(&path).output().unwrap();
    assert!(out2.status.success());
    let after_second = fs::read_to_string(&path).unwrap();
    assert_eq!(
        after_first, after_second,
        "second --write must leave the file byte-identical",
    );

    fs::remove_file(&path).ok();
}

#[test]
fn write_to_stdin_is_rejected_with_exit_two() {
    let mut child = aozora_fmt()
        .arg("--write")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn aozora-fmt");
    child.stdin.as_mut().unwrap().write_all(b"hello").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "--write without a file path must exit 2",
    );
}
