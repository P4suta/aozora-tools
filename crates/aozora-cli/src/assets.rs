//! `aozora completions <shell>` and `aozora man` — runtime asset generators.
//!
//! Complementary to the committed `assets/` (xtask-generated, bundled in the
//! release archives): these serve users who installed only the binary.

use std::io::{self, Write};
use std::process::ExitCode;

use clap::CommandFactory;
use clap_complete::{Shell, generate};
use clap_complete_nushell::Nushell;
use clap_mangen::Man;

use crate::cli::{Cli, CompletionShell, CompletionsArgs, ManArgs};

/// Print a shell completion script for the `aozora` command tree to stdout.
#[must_use]
pub(crate) fn completions(args: CompletionsArgs) -> ExitCode {
    write_completions(args.shell, &mut io::stdout());
    ExitCode::SUCCESS
}

/// Print a man page (the top-level page, or a named subcommand's) to stdout.
#[must_use]
pub(crate) fn man(args: &ManArgs) -> ExitCode {
    let root = Cli::command();
    let command = if let Some(name) = &args.command {
        let Some(sub) = root.find_subcommand(name) else {
            eprintln!("aozora man: unknown subcommand `{name}`");
            return ExitCode::from(2);
        };
        sub.clone()
    } else {
        root
    };
    match write_man(command, &mut io::stdout()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("aozora man: {err}");
            ExitCode::from(2)
        }
    }
}

/// Generate `shell`'s completion script for `aozora` into `out`.
fn write_completions(shell: CompletionShell, out: &mut impl Write) {
    let mut cmd = Cli::command();
    let bin = "aozora";
    match shell {
        CompletionShell::Bash => generate(Shell::Bash, &mut cmd, bin, out),
        CompletionShell::Zsh => generate(Shell::Zsh, &mut cmd, bin, out),
        CompletionShell::Fish => generate(Shell::Fish, &mut cmd, bin, out),
        CompletionShell::Powershell => generate(Shell::PowerShell, &mut cmd, bin, out),
        CompletionShell::Nushell => generate(Nushell, &mut cmd, bin, out),
    }
}

/// Render `command`'s man page into `out`.
fn write_man(command: clap::Command, out: &mut impl Write) -> io::Result<()> {
    Man::new(command).render(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completions_render_nonempty_for_every_shell() {
        for shell in [
            CompletionShell::Bash,
            CompletionShell::Zsh,
            CompletionShell::Fish,
            CompletionShell::Powershell,
            CompletionShell::Nushell,
        ] {
            let mut buf = Vec::new();
            write_completions(shell, &mut buf);
            let text = String::from_utf8(buf).expect("utf8");
            assert!(
                text.contains("aozora"),
                "{shell:?} completion should mention the binary",
            );
        }
    }

    #[test]
    fn man_renders_the_top_level_page() {
        let mut buf = Vec::new();
        write_man(Cli::command(), &mut buf).expect("render man");
        let text = String::from_utf8(buf).expect("utf8");
        assert!(
            text.contains("aozora"),
            "man page names the binary: {text:.80}"
        );
        assert!(text.contains(".TH"), "troff header expected");
    }

    #[test]
    fn man_renders_a_subcommand_page() {
        let lint = Cli::command()
            .find_subcommand("lint")
            .expect("lint subcommand exists")
            .clone();
        let mut buf = Vec::new();
        write_man(lint, &mut buf).expect("render lint man");
        assert!(!buf.is_empty(), "subcommand man page should not be empty");
    }
}
