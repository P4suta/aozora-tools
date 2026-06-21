//! `aozora lint --watch` — re-run lint on every file change, like a dev server.
//!
//! The event source (a debounced `notify` watcher) is kept at the edge; the
//! loop itself ([`watch_loop`]) takes an injectable channel + stop flag + a
//! re-run closure, so it is unit-testable with no real filesystem or clock.

use std::error::Error;
use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use notify_debouncer_mini::notify::RecursiveMode;
use notify_debouncer_mini::{DebounceEventResult, new_debouncer};

use crate::cli::LintArgs;
use crate::lint;

/// Debounce window: editors emit several FS events per save; coalesce them.
const DEBOUNCE: Duration = Duration::from_millis(200);

/// Run `aozora lint --watch` and return the process exit code (0 on Ctrl-C).
#[must_use]
pub(crate) fn run(args: &LintArgs) -> ExitCode {
    if args.paths.is_empty() {
        eprintln!("aozora lint: --watch needs at least one path to watch");
        return ExitCode::from(2);
    }
    let stop = Arc::new(AtomicBool::new(false));
    let handler_flag = Arc::clone(&stop);
    if let Err(err) = ctrlc::set_handler(move || handler_flag.store(true, Ordering::Relaxed)) {
        eprintln!("aozora lint: could not install the Ctrl-C handler: {err}");
        return ExitCode::from(2);
    }
    match start(args, &stop) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("aozora lint: watch failed: {err}");
            ExitCode::from(2)
        }
    }
}

/// Set up the watcher, run an initial pass, then loop until stopped.
fn start(args: &LintArgs, stop: &AtomicBool) -> Result<(), Box<dyn Error>> {
    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(DEBOUNCE, move |res: DebounceEventResult| {
        // The receiver only cares *that* something changed, not what; a send
        // failure just means the loop already exited, which is fine.
        if res.is_ok() {
            tx.send(()).unwrap_or_default();
        }
    })?;
    for path in &args.paths {
        debouncer.watcher().watch(path, RecursiveMode::Recursive)?;
    }

    run_pass(args);
    watch_loop(&rx, stop, || run_pass(args));

    eprintln!("\naozora lint: stopped watching.");
    Ok(())
}

/// The injectable core loop: block on `rx`, run `on_change` per coalesced batch,
/// exit when `stop` is set or the channel disconnects.
fn watch_loop(rx: &Receiver<()>, stop: &AtomicBool, mut on_change: impl FnMut()) {
    while !stop.load(Ordering::Relaxed) {
        match rx.recv_timeout(DEBOUNCE) {
            Ok(()) => {
                drain(rx);
                on_change();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Swallow any extra signals queued behind the one we just handled, so a burst
/// of saves triggers a single re-run.
fn drain(rx: &Receiver<()>) {
    while rx.try_recv().is_ok() {}
}

/// Clear the screen (TTY only), print a timestamped banner, and re-lint.
fn run_pass(args: &LintArgs) {
    if io::stdout().is_terminal() {
        // Clear scrollback + screen + home the cursor.
        print!("\x1b[2J\x1b[3J\x1b[H");
        io::stdout().flush().unwrap_or_default();
    }
    let n = args.paths.len();
    let plural = if n == 1 { "" } else { "s" };
    eprintln!(
        "[{}] アオゾラ watching {n} path{plural} — Ctrl-C to exit",
        now_hms()
    );
    // Outcome is informational in watch mode; surface any run error and keep going.
    if let Err(err) = lint::lint_paths(args) {
        eprintln!("aozora lint: {err:#}");
    }
}

/// Current UTC time-of-day as `HH:MM:SS` (no timezone/clock crate needed).
fn now_hms() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let day = secs % 86_400;
    format!("{:02}:{:02}:{:02}", day / 3600, (day % 3600) / 60, day % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_runs_on_change_then_stops() {
        let (tx, rx) = channel();
        let stop = AtomicBool::new(false);
        tx.send(()).expect("send change");
        let mut count = 0;
        watch_loop(&rx, &stop, || {
            count += 1;
            // Stop after the first re-run so the loop terminates deterministically.
            stop.store(true, Ordering::Relaxed);
        });
        assert_eq!(count, 1, "exactly one re-run for the queued change");
    }

    #[test]
    fn loop_exits_immediately_when_already_stopped() {
        let (_tx, rx) = channel::<()>();
        let stop = AtomicBool::new(true);
        let mut count = 0;
        watch_loop(&rx, &stop, || count += 1);
        assert_eq!(count, 0, "a pre-set stop flag means no re-runs");
    }

    #[test]
    fn loop_exits_on_disconnect() {
        let (tx, rx) = channel::<()>();
        let stop = AtomicBool::new(false);
        drop(tx); // disconnect: recv_timeout returns Disconnected
        let mut count = 0;
        watch_loop(&rx, &stop, || count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn now_hms_is_well_formed() {
        let stamp = now_hms();
        assert_eq!(stamp.len(), 8, "HH:MM:SS: {stamp}");
        assert_eq!(stamp.as_bytes()[2], b':');
        assert_eq!(stamp.as_bytes()[5], b':');
    }

    #[test]
    fn drain_empties_the_channel() {
        let (tx, rx) = channel();
        for _ in 0..5 {
            tx.send(()).unwrap();
        }
        drain(&rx);
        assert!(rx.try_recv().is_err(), "drain should empty the queue");
    }

    #[test]
    fn run_pass_lints_a_file_once_without_blocking() {
        use std::sync::atomic::AtomicU32;
        use std::{env, fs, process};

        static SEQ: AtomicU32 = AtomicU32::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let mut path = env::temp_dir();
        path.push(format!("aozora-watch-test-{}-{n}.afm", process::id()));
        fs::write(&path, "本文［＃改ページ").expect("seed file");

        let args = LintArgs {
            paths: vec![path.clone()],
            json: false,
            quiet: true,
            error_on_warning: false,
            watch: true,
            stats: false,
            color: aozora_fmt::ColorChoice::Never,
        };
        // A single pass returns immediately (it does not enter the watch loop).
        run_pass(&args);
        fs::remove_file(&path).ok();
    }
}
