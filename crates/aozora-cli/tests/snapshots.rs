//! Snapshot tests that pin the *rendered* shape of the CLI's output — the
//! caret-underlined lint snippet, the `--json` report, the `explain` prose, and
//! the HTML fragment — so a regression in any of them is a reviewable diff.
//!
//! Everything is driven through the compiled binary with `--color never` and a
//! `<stdin>` label, so the output is deterministic (no ANSI, no paths, no
//! clock). Update with `cargo insta review` (or `INSTA_UPDATE=always`).

use std::io::Write;
use std::process::{Command, Stdio};

fn aozora() -> Command {
    Command::new(env!("CARGO_BIN_EXE_aozora"))
}

/// Run `aozora <args>` with `input` on stdin and return stdout.
fn run_stdin(args: &[&str], input: &str) -> String {
    let mut child = aozora()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn aozora");
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(input.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait for aozora");
    String::from_utf8(out.stdout).expect("utf8 stdout")
}

/// Run `aozora <args>` with no stdin and return stdout.
fn run(args: &[&str]) -> String {
    let out = aozora().args(args).output().expect("spawn aozora");
    String::from_utf8(out.stdout).expect("utf8 stdout")
}

/// An unclosed bracket on an all-full-width line — exercises the width-aware
/// caret alignment.
const UNCLOSED: &str = "本文［＃改ページ";

#[test]
fn snapshot_lint_human() {
    insta::assert_snapshot!(run_stdin(&["lint", "--color", "never", "-"], UNCLOSED));
}

#[test]
fn snapshot_lint_quiet() {
    insta::assert_snapshot!(run_stdin(
        &["lint", "--quiet", "--color", "never", "-"],
        UNCLOSED
    ));
}

#[test]
fn snapshot_lint_json() {
    insta::assert_snapshot!(run_stdin(&["lint", "--json", "-"], UNCLOSED));
}

#[test]
fn snapshot_explain_unclosed_bracket() {
    insta::assert_snapshot!(run(&["explain", "--color", "never", "unclosed-bracket"]));
}

#[test]
fn snapshot_explain_list() {
    insta::assert_snapshot!(run(&["explain", "--color", "never"]));
}

#[test]
fn snapshot_render_fragment() {
    insta::assert_snapshot!(run_stdin(&["render", "-"], "｜日本《にほん》"));
}
