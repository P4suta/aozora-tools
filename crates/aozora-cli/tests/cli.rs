//! Drives the compiled `aozora` umbrella binary to cover the argv surface and
//! per-subcommand exit codes that in-process unit tests can't reach. Uses the
//! raw `std::process::Command` idiom (matching `aozora-lsp/tests/cli.rs`), not
//! `assert_cmd`.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn aozora() -> Command {
    Command::new(env!("CARGO_BIN_EXE_aozora"))
}

/// Run `aozora <args>` feeding `input` on stdin, capturing the output.
fn run_stdin(args: &[&str], input: &str) -> Output {
    let mut child = aozora()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn aozora");
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(input.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait for aozora")
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf8 stdout")
}

#[test]
fn version_prints_pinned_rev() {
    let out = aozora().arg("--version").output().expect("spawn --version");
    assert!(out.status.success());
    let stdout = stdout_of(&out);
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "{stdout:?}");
    assert!(
        stdout.contains("aozora "),
        "names the parser rev: {stdout:?}"
    );
}

#[test]
fn help_lists_every_subcommand() {
    let out = aozora().arg("--help").output().expect("spawn --help");
    assert!(out.status.success());
    let stdout = stdout_of(&out);
    for sub in ["fmt", "lint", "render", "explain", "lsp"] {
        assert!(stdout.contains(sub), "help should list `{sub}`: {stdout}");
    }
}

#[test]
fn unknown_subcommand_suggests_the_nearest() {
    let out = aozora().arg("lnt").output().expect("spawn typo");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).expect("utf8");
    assert!(stderr.contains("lint"), "should suggest `lint`: {stderr}");
}

#[test]
fn lint_clean_input_exits_zero() {
    let out = run_stdin(&["lint", "-"], "｜日本《にほん》");
    assert_eq!(out.status.code(), Some(0), "stderr: {:?}", out.stderr);
}

#[test]
fn lint_diagnostics_exit_one() {
    let out = run_stdin(&["lint", "--color", "never", "-"], "本文［＃改ページ");
    assert_eq!(out.status.code(), Some(1));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("aozora::unclosed-bracket"), "{stdout}");
}

#[test]
fn lint_json_is_machine_readable() {
    let out = run_stdin(&["lint", "--json", "-"], "本文［＃改ページ");
    assert_eq!(out.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    assert_eq!(value["version"], 1);
    assert_eq!(value["ok"], false);
    assert_eq!(
        value["files"][0]["diagnostics"][0]["code"],
        "aozora::unclosed-bracket"
    );
}

#[test]
fn check_is_an_alias_for_lint() {
    let out = run_stdin(&["check", "-"], "｜日本《にほん》");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn explain_known_code_exits_zero() {
    let out = aozora()
        .args(["explain", "aozora::unclosed-bracket"])
        .output()
        .expect("spawn explain");
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout_of(&out).contains("閉じ"),
        "explanation prose expected"
    );
}

#[test]
fn explain_unknown_code_exits_two() {
    let out = aozora()
        .args(["explain", "no-such-code"])
        .output()
        .expect("spawn explain");
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn explain_with_no_argument_lists_codes() {
    let out = aozora().arg("explain").output().expect("spawn explain");
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout_of(&out).contains("aozora::unclosed-bracket"),
        "lists codes",
    );
}

#[test]
fn explain_ambiguous_prefix_exits_two() {
    // `reg` prefixes both registry-* codes.
    let out = aozora().args(["explain", "reg"]).output().expect("spawn");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).expect("utf8");
    assert!(stderr.contains("ambiguous"), "{stderr}");
}

#[test]
fn lint_watch_without_paths_exits_two() {
    // The empty-path guard returns immediately (no watch loop, no hang).
    let out = run_stdin(&["lint", "--watch"], "");
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn render_outputs_html_fragment() {
    let out = run_stdin(&["render", "-"], "｜日本《にほん》");
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout_of(&out).contains("<ruby"), "ruby HTML expected");
}

#[test]
fn fmt_canonicalises_stdin() {
    let out = run_stdin(&["fmt", "-"], "日本《にほん》");
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout_of(&out).starts_with('｜'),
        "fmt should add the explicit ruby delimiter",
    );
}

#[test]
fn completions_bash_is_nonempty() {
    let out = aozora()
        .args(["completions", "bash"])
        .output()
        .expect("spawn completions");
    assert!(out.status.success());
    assert!(stdout_of(&out).contains("_aozora"), "bash completion fn");
}

#[test]
fn man_renders_a_page() {
    let out = aozora().arg("man").output().expect("spawn man");
    assert!(out.status.success());
    assert!(stdout_of(&out).contains(".TH"), "troff header expected");
}

#[test]
fn man_unknown_subcommand_exits_two() {
    let out = aozora().args(["man", "nope"]).output().expect("spawn man");
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn lint_missing_file_is_an_error() {
    let out = aozora()
        .args(["lint", "/no/such/aozora-file.afm"])
        .output()
        .expect("spawn lint");
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn render_missing_file_is_an_error() {
    let out = aozora()
        .args(["render", "/no/such/aozora-file.afm"])
        .output()
        .expect("spawn render");
    assert_eq!(out.status.code(), Some(2));
}
