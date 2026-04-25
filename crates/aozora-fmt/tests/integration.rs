//! Integration tests that shell out to the compiled `aozora-fmt` binary.

use std::io::Write;
use std::process::{Command, Stdio};

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
