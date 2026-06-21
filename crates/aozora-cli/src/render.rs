//! `aozora render` — render a document to HTML.
//!
//! The default output is the bare fragment `aozora::Document::parse().to_html()`
//! produces (byte-identical to the LSP preview). `--standalone` wraps it in a
//! minimal vertical-writing HTML5 document; `--open` writes that to a temp file
//! and launches the browser.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;
use std::{env, fs, process};

use anyhow::{Context, Result, anyhow, bail};

use aozora::Document;
use aozora_fmt::guard;

use crate::cli::RenderArgs;

/// Render size cap, mirroring the LSP's `MAX_DOCUMENT_BYTES` (16 MiB). Unlike
/// the LSP (which returns an inert notice to stay responsive), the one-shot CLI
/// fails loudly so a `-o` pipeline never produces a useless artifact.
const MAX_RENDER_BYTES: usize = 16 * 1024 * 1024;

/// Minimal stylesheet for `--standalone`: vertical right-to-left Japanese text,
/// matching how the editor preview renders aozora documents.
const STANDALONE_CSS: &str = "\
html { writing-mode: vertical-rl; }
body { font-family: \"Noto Serif CJK JP\", \"Hiragino Mincho ProN\", serif;
       line-height: 1.8; max-block-size: 40em; margin: 2rem auto; }
ruby rt { font-size: 0.5em; }";

/// Run `aozora render` and return the process exit code.
#[must_use]
pub(crate) fn run(args: &RenderArgs) -> ExitCode {
    match render(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("aozora render: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn render(args: &RenderArgs) -> Result<()> {
    let start = Instant::now();
    let (label, source) = read_input(args.path.as_deref())?;
    ensure_within_limit(&label, source.len())?;

    let fragment = to_fragment(&source)
        .map_err(|()| anyhow!("the parser panicked while rendering {label}; no output produced"))?;
    let html = if args.standalone || args.open {
        wrap_standalone(&label, &fragment)
    } else {
        fragment
    };

    write_output(args, &html)?;
    if args.stats {
        eprintln!(
            "aozora: rendered {label} ({} bytes in, {} bytes out) in {}ms",
            source.len(),
            html.len(),
            start.elapsed().as_millis(),
        );
    }
    Ok(())
}

/// Read the single input: a file path, or stdin for `None`/`-`.
fn read_input(path: Option<&Path>) -> Result<(String, String)> {
    match path {
        None => read_stdin(),
        Some(p) if p == Path::new("-") => read_stdin(),
        Some(p) => {
            let source =
                fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
            Ok((p.display().to_string(), source))
        }
    }
}

fn read_stdin() -> Result<(String, String)> {
    let mut source = String::new();
    io::stdin()
        .read_to_string(&mut source)
        .context("reading stdin")?;
    Ok(("<stdin>".to_owned(), source))
}

/// Fail if `len` exceeds the render cap. Split out so the limit is unit-testable
/// without allocating a 16 MiB document.
fn ensure_within_limit(label: &str, len: usize) -> Result<()> {
    if len > MAX_RENDER_BYTES {
        bail!(
            "{label} is {} MiB, above the {} MiB render limit",
            len / (1024 * 1024),
            MAX_RENDER_BYTES / (1024 * 1024),
        );
    }
    Ok(())
}

/// Render `source` to an HTML fragment under the panic guard.
fn to_fragment(source: &str) -> Result<String, ()> {
    guard(|| Document::new(source).parse().to_html()).map_err(|_| ())
}

/// Wrap an HTML fragment in a standalone, self-contained HTML5 document.
fn wrap_standalone(label: &str, fragment: &str) -> String {
    format!(
        "<!DOCTYPE html>\n\
         <html lang=\"ja\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{label}</title>\n\
         <style>\n{STANDALONE_CSS}\n</style>\n\
         </head>\n\
         <body>\n{fragment}\n</body>\n\
         </html>\n",
    )
}

fn write_output(args: &RenderArgs, html: &str) -> Result<()> {
    if args.open {
        let path = args
            .output
            .as_ref()
            .map_or_else(temp_html_path, Clone::clone);
        fs::write(&path, html).with_context(|| format!("writing {}", path.display()))?;
        opener::open(&path).with_context(|| format!("opening {} in a browser", path.display()))?;
        return Ok(());
    }
    if let Some(path) = &args.output {
        fs::write(path, html).with_context(|| format!("writing {}", path.display()))
    } else {
        io::stdout().write_all(html.as_bytes())?;
        Ok(())
    }
}

/// A scratch HTML path under the OS temp dir for `--open`. Uses the PID for
/// uniqueness (no clock / RNG needed).
fn temp_html_path() -> PathBuf {
    env::temp_dir().join(format!("aozora-render-{}.html", process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::cli::RenderArgs;

    /// A unique scratch dir for the file-based render path.
    fn scratch() -> PathBuf {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let mut dir = env::temp_dir();
        dir.push(format!("aozora-render-test-{}-{n}", process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn args_for(path: PathBuf, output: PathBuf) -> RenderArgs {
        RenderArgs {
            path: Some(path),
            output: Some(output),
            standalone: true,
            open: false,
            stats: true,
            color: aozora_fmt::ColorChoice::Never,
        }
    }

    #[test]
    fn render_reads_a_file_and_writes_standalone_html() {
        let dir = scratch();
        let input = dir.join("in.afm");
        let output = dir.join("out.html");
        fs::write(&input, "｜日本《にほん》").expect("seed input");

        render(&args_for(input, output.clone())).expect("render to file");

        let html = fs::read_to_string(&output).expect("read output");
        assert!(
            html.starts_with("<!DOCTYPE html>"),
            "standalone doc: {html:.40}"
        );
        assert!(html.contains("<ruby"), "fragment embedded: {html}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fragment_renders_ruby_markup() {
        let html = to_fragment("｜日本《にほん》").expect("render");
        assert!(html.contains("<ruby"), "expected ruby HTML, got: {html}");
        assert!(html.contains("にほん"), "reading should appear: {html}");
    }

    #[test]
    fn standalone_wraps_the_fragment() {
        let wrapped = wrap_standalone("doc.afm", "<p>本文</p>");
        assert!(wrapped.starts_with("<!DOCTYPE html>"), "{wrapped}");
        assert!(wrapped.contains("writing-mode: vertical-rl"), "{wrapped}");
        assert!(wrapped.contains("<title>doc.afm</title>"), "{wrapped}");
        assert!(
            wrapped.contains("<p>本文</p>"),
            "fragment embedded: {wrapped}"
        );
    }

    #[test]
    fn within_limit_passes_and_over_limit_fails() {
        ensure_within_limit("ok.afm", MAX_RENDER_BYTES).expect("at the cap is fine");
        let err = ensure_within_limit("big.afm", MAX_RENDER_BYTES + 1)
            .expect_err("over the cap must fail");
        assert!(err.to_string().contains("render limit"), "{err}");
    }
}
