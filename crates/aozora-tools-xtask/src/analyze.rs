//! One-shot multi-view samply trace report.
//!
//! ## Why a single comprehensive report
//!
//! Profiling pipelines that emit one view per command (top-N
//! leaves, then by-module, then by-call-chain, …) push the
//! synthesis cost onto the user: they remember which CLI to run,
//! whether they saved the previous output, what the columns mean.
//! Our analyzer instead emits **every routinely-useful view in one
//! pass**, formatted as Markdown sections so the result is both
//! human-readable and grep-able. The user runs one command and
//! gets:
//!
//! 1. **Trace metadata** — sample count, duration, sampling
//!    interval, symbol resolution status.
//! 2. **Per-thread leaf top-25** — the classic flat report.
//! 3. **By owner** — leaves rolled up into ownership categories
//!    (our code / tree-sitter runtime / ropey / `arc_swap` / std /
//!    libc / allocator / other). The column users actually want
//!    when asking "is the time in OUR code or in dependencies?"
//! 4. **Allocator activity** — focused subset of #3 because the
//!    allocator is THE most commonly investigated cost source and
//!    deserves a dedicated callout (with a malloc-versus-mmap
//!    breakdown).
//! 5. **Hot stack signatures (top-10)** — `(leaf, immediate
//!    parent)` pairs ranked by frequency, so callers of a hot
//!    function are visible without descending into the GUI's tree
//!    view.
//!
//! All percentages are computed against the same denominator (the
//! containing thread's total samples) so cross-section comparison
//! is straightforward.
//!
//! ## Pipeline
//!
//! 1. **Decompress** the `.json.gz` (Firefox-Profiler "processed
//!    profile" format).
//! 2. **Resolve symbols** via the `.syms.json` sidecar samply emits
//!    when invoked with `--unstable-presymbolicate`. Walk every
//!    leaf frame, look up the raw `0x…` hex address in the
//!    sidecar, replace `funcTable.name → stringArray` with the
//!    resolved function name.
//! 3. **Aggregate** the multiple views above and print them.
//!
//! Output is plain text on stdout — pipe to `tee` to save, run
//! twice and `diff` to compare, or `grep` for a specific section.
//!
//! ### Format docs
//!
//! - <https://github.com/firefox-devtools/profiler/blob/main/docs-developer/processed-profile-format.md>

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{self, Path, PathBuf};

use flate2::read::GzDecoder;
use serde::Deserialize;

/// How many top entries to print per view. 25 fits one screen and
/// covers the bulk of realistic traces; 10 for the more focused
/// "hot stacks" / by-owner views.
const TOP_LEAVES: usize = 25;
const TOP_OWNERS: usize = 12;
const TOP_STACKS: usize = 10;

/// Entry point: load `path`, parse, resolve symbols, and emit the
/// full multi-view report on stdout.
pub(crate) fn analyze(path: &Path) -> Result<(), String> {
    let raw = read_gz(path)?;
    let mut profile: Profile = serde_json::from_slice(&raw)
        .map_err(|e| format!("parse {} as Firefox Profiler JSON: {e}", path.display()))?;

    let symbol_db = SymbolDb::load(path)?;
    for thread in &mut profile.threads {
        symbol_db.resolve_thread(thread, &profile.libs);
    }

    let total_samples: usize = profile.threads.iter().map(|t| t.samples.length).sum();
    print_metadata(path, &profile, &symbol_db, total_samples);

    for thread in &profile.threads {
        let thread_samples = thread.samples.length;
        if thread_samples == 0 {
            continue;
        }
        println!(
            "\n# thread `{}` — {} samples ({:.1}% of trace total)",
            thread.name,
            thread_samples,
            ratio_pct(thread_samples, total_samples),
        );

        let leaves = aggregate_leaves(thread);
        print_leaves(&leaves, thread_samples);
        print_by_owner(&leaves, thread_samples);
        print_allocator_focus(&leaves, thread_samples);
        print_hot_stacks(thread, thread_samples);
    }

    Ok(())
}

fn read_gz(path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut decoder = GzDecoder::new(file);
    let mut buf = Vec::new();
    decoder
        .read_to_end(&mut buf)
        .map_err(|e| format!("gunzip {}: {e}", path.display()))?;
    Ok(buf)
}

// --- multi-view emitters ---

fn print_metadata(path: &Path, profile: &Profile, db: &SymbolDb, total_samples: usize) {
    println!("# samply trace summary: {}", path.display());
    println!();
    println!("- threads: {}", profile.threads.len());
    println!("- total samples: {total_samples}");
    println!("- sampling interval: {} ms", profile.meta.interval);
    if let Some(start) = profile.meta.start_time
        && let Some(end) = profile.meta.end_time
    {
        let dur_ms = (end - start).max(0.0);
        println!("- wall duration: {dur_ms:.1} ms");
        if dur_ms > 0.0 {
            let rate =
                f64::from(u32::try_from(total_samples).unwrap_or(u32::MAX)) / (dur_ms / 1000.0);
            println!("- effective sample rate: {rate:.0} Hz");
        }
    }
    if db.is_loaded() {
        println!("- symbol resolution: ✓ (sidecar `{}`)", db.sidecar_path());
    } else {
        println!(
            "- symbol resolution: ✗ NO sidecar at `{}` — leaf names will be raw `0x…`",
            db.sidecar_path(),
        );
    }
}

fn print_leaves(leaves: &[(String, usize)], thread_samples: usize) {
    println!();
    println!("## flat top {TOP_LEAVES} leaves by self-time");
    let rows: Vec<(&str, usize)> = leaves
        .iter()
        .take(TOP_LEAVES)
        .map(|(s, c)| (s.as_str(), *c))
        .collect();
    print_count_table(rows, thread_samples);
}

fn print_by_owner(leaves: &[(String, usize)], thread_samples: usize) {
    let mut by_owner: HashMap<&'static str, usize> = HashMap::new();
    for (name, count) in leaves {
        *by_owner.entry(classify_owner(name)).or_insert(0) += count;
    }
    let mut sorted: Vec<(&'static str, usize)> = by_owner.into_iter().collect();
    sorted.sort_by_key(|(_, c)| Reverse(*c));
    println!();
    println!("## time by owner (rolled up)");
    let rows: Vec<(&'static str, usize)> = sorted.into_iter().take(TOP_OWNERS).collect();
    print_count_table(rows, thread_samples);
}

fn print_allocator_focus(leaves: &[(String, usize)], thread_samples: usize) {
    let mut malloc_total = 0usize;
    let mut mmap_total = 0usize;
    let mut free_total = 0usize;
    let mut other_alloc = 0usize;
    for (name, count) in leaves {
        match classify_alloc(name) {
            AllocKind::Malloc => malloc_total += count,
            AllocKind::Mmap => mmap_total += count,
            AllocKind::Free => free_total += count,
            AllocKind::Other => other_alloc += count,
            AllocKind::None => {}
        }
    }
    let total = malloc_total + mmap_total + free_total + other_alloc;
    if total == 0 {
        return;
    }
    println!();
    println!("## allocator activity");
    println!("  {:<12}  {:>8}  {:>5}", "category", "samples", "%");
    let rows = [
        ("malloc/realloc", malloc_total),
        ("mmap/munmap", mmap_total),
        ("free", free_total),
        ("other libc", other_alloc),
        ("total", total),
    ];
    for (label, count) in rows {
        if count == 0 && label != "total" {
            continue;
        }
        println!(
            "  {label:<12}  {count:>8}  {:>4.1}%",
            ratio_pct(count, thread_samples),
        );
    }
}

fn print_hot_stacks(thread: &Thread, thread_samples: usize) {
    let mut counts: HashMap<(String, String), usize> = HashMap::new();
    for &maybe_stack in &thread.samples.stack {
        let Some(stack_idx) = maybe_stack else {
            continue;
        };
        if stack_idx >= thread.stack_table.length {
            continue;
        }
        let leaf_frame = thread.stack_table.frame[stack_idx];
        let parent_stack = thread.stack_table.prefix[stack_idx];
        let leaf_name = function_name_for_frame(thread, leaf_frame).unwrap_or("?");
        let parent_name = parent_stack
            .and_then(|i| (i < thread.stack_table.length).then(|| thread.stack_table.frame[i]))
            .and_then(|f| function_name_for_frame(thread, f))
            .unwrap_or("(root)");
        *counts
            .entry((leaf_name.to_owned(), parent_name.to_owned()))
            .or_insert(0) += 1;
    }
    let mut sorted: Vec<((String, String), usize)> = counts.into_iter().collect();
    sorted.sort_by_key(|(_, c)| Reverse(*c));
    println!();
    println!("## hot stack signatures — top {TOP_STACKS} (leaf ← parent)");
    println!("  {:>8}  {:>5}  leaf ← parent", "samples", "%");
    for ((leaf, parent), count) in sorted.iter().take(TOP_STACKS) {
        println!(
            "  {count:>8}  {:>4.1}%  {leaf} ← {parent}",
            ratio_pct(*count, thread_samples)
        );
    }
}

fn print_count_table<I, S>(rows: I, denom: usize)
where
    I: IntoIterator<Item = (S, usize)>,
    S: AsRef<str>,
{
    println!("  {:>8}  {:>5}  name", "samples", "%");
    let collected: Vec<(S, usize)> = rows.into_iter().collect();
    let max_width = collected
        .iter()
        .map(|(s, _)| s.as_ref().chars().count())
        .max()
        .unwrap_or(0)
        .min(120);
    for (name, count) in &collected {
        println!(
            "  {count:>8}  {:>4.1}%  {:<width$}",
            ratio_pct(*count, denom),
            name.as_ref(),
            width = max_width,
        );
    }
}

fn ratio_pct(num: usize, denom: usize) -> f64 {
    if denom == 0 {
        return 0.0;
    }
    let n = u32::try_from(num).unwrap_or(u32::MAX);
    let d = u32::try_from(denom).unwrap_or(u32::MAX);
    f64::from(n) / f64::from(d) * 100.0
}

// --- aggregation ---

fn aggregate_leaves(thread: &Thread) -> Vec<(String, usize)> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let stack_count = thread.stack_table.length;
    let frame_count = thread.frame_table.length;
    for &maybe_stack in &thread.samples.stack {
        let Some(stack_idx) = maybe_stack else {
            continue;
        };
        if stack_idx >= stack_count {
            continue;
        }
        let frame_idx = thread.stack_table.frame[stack_idx];
        if frame_idx >= frame_count {
            continue;
        }
        let func_idx = thread.frame_table.func[frame_idx];
        let Some(name) = function_name(thread, func_idx) else {
            continue;
        };
        *counts.entry(name).or_insert(0) += 1;
    }
    let mut out: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(name, count)| (name.to_owned(), count))
        .collect();
    out.sort_by_key(|(_, c)| Reverse(*c));
    out
}

fn function_name(thread: &Thread, func_idx: usize) -> Option<&str> {
    if func_idx >= thread.func_table.length {
        return None;
    }
    let name_idx = thread.func_table.name[func_idx];
    thread.string_array.get(name_idx).map(String::as_str)
}

fn function_name_for_frame(thread: &Thread, frame_idx: usize) -> Option<&str> {
    if frame_idx >= thread.frame_table.length {
        return None;
    }
    let func_idx = thread.frame_table.func[frame_idx];
    function_name(thread, func_idx)
}

// --- function-owner classification ---

/// Bucket a function name into a coarse "owner" category. Used by
/// the by-owner roll-up section. Patterns are checked in priority
/// order; the first match wins.
///
/// Categories cover both Rust mangled names (`aozora_lsp::…`) and
/// the C symbols that show up from tree-sitter (`ts_*`, `subtree_*`,
/// `iterator_*`, `stack_*`) and from libc / the allocator
/// (`mmap`, `_libc_*`, `_default_morecore`, etc).
fn classify_owner(name: &str) -> &'static str {
    if name.starts_with("aozora_lsp::") || name.contains("aozora_lsp::") {
        "aozora_lsp"
    } else if name.starts_with("aozora_") || name.contains("aozora_") {
        "aozora_*"
    } else if is_tree_sitter_runtime(name) {
        "tree_sitter (C)"
    } else if name.starts_with("<tree_sitter") || name.contains("tree_sitter::") {
        "tree_sitter (rust)"
    } else if name.starts_with("ropey::") || name.contains("ropey::") {
        "ropey"
    } else if name.starts_with("<arc_swap") || name.contains("arc_swap::") {
        "arc_swap"
    } else if name.starts_with("std::") || name.starts_with("core::") || name.starts_with("alloc::")
    {
        "std/core/alloc"
    } else if matches!(classify_alloc(name), AllocKind::None) {
        if name.starts_with("0x") {
            "unresolved"
        } else {
            "other"
        }
    } else {
        "allocator/libc"
    }
}

fn is_tree_sitter_runtime(name: &str) -> bool {
    name.starts_with("ts_")
        || name.starts_with("iterator_")
        || name.starts_with("stack_")
        || name.starts_with("subtree_")
        || name == "stack__iter"
        || name.starts_with("array__")
}

#[derive(Clone, Copy)]
enum AllocKind {
    Malloc,
    Mmap,
    Free,
    Other,
    None,
}

fn classify_alloc(name: &str) -> AllocKind {
    if name == "malloc"
        || name == "_libc_malloc"
        || name == "realloc"
        || name == "calloc"
        || name == "_libc_calloc"
        || name == "_default_morecore"
    {
        AllocKind::Malloc
    } else if name == "_mmap" || name == "_munmap" || name == "mmap" || name == "munmap" {
        AllocKind::Mmap
    } else if name == "free" || name == "_libc_free" {
        AllocKind::Free
    } else if name.starts_with("_libc_") || name == "brk" || name == "sbrk" || name == "_mprotect" {
        AllocKind::Other
    } else {
        AllocKind::None
    }
}

// --- symbol resolution via the .syms.json sidecar ---

struct SymbolDb {
    sidecar_path: PathBuf,
    by_debug_name: HashMap<String, BinarySymbols>,
}

struct BinarySymbols {
    /// `(rva, size, name)` entries sorted ascending by `rva`.
    entries: Vec<(u32, u32, String)>,
}

impl SymbolDb {
    fn load(trace_path: &Path) -> Result<Self, String> {
        let sidecar = sidecar_path_for(trace_path);
        if !sidecar.exists() {
            return Ok(Self {
                sidecar_path: sidecar,
                by_debug_name: HashMap::new(),
            });
        }
        let raw =
            fs::read_to_string(&sidecar).map_err(|e| format!("read {}: {e}", sidecar.display()))?;
        let parsed: SymsFile = serde_json::from_str(&raw)
            .map_err(|e| format!("parse {} as samply syms.json: {e}", sidecar.display()))?;
        let mut by_debug_name: HashMap<String, BinarySymbols> = HashMap::new();
        for entry in parsed.data {
            let mut entries: Vec<(u32, u32, String)> = entry
                .symbol_table
                .into_iter()
                .filter_map(|s| {
                    let name = parsed.string_table.get(s.symbol)?.clone();
                    Some((s.rva, s.size, name))
                })
                .collect();
            entries.sort_by_key(|&(rva, _, _)| rva);
            by_debug_name.insert(entry.debug_name, BinarySymbols { entries });
        }
        Ok(Self {
            sidecar_path: sidecar,
            by_debug_name,
        })
    }

    fn is_loaded(&self) -> bool {
        !self.by_debug_name.is_empty()
    }

    fn sidecar_path(&self) -> path::Display<'_> {
        self.sidecar_path.display()
    }

    fn resolve_thread(&self, thread: &mut Thread, libs: &[Lib]) {
        if self.by_debug_name.is_empty() {
            return;
        }
        for func_idx in 0..thread.func_table.length {
            let name_str_idx = thread.func_table.name[func_idx];
            let Some(name) = thread.string_array.get(name_str_idx) else {
                continue;
            };
            let Some(rva) = parse_hex_addr(name) else {
                continue;
            };
            let res_idx = thread.func_table.resource[func_idx];
            let Some(lib_idx) = resource_lib(&thread.resource_table, res_idx) else {
                continue;
            };
            let Some(lib) = libs.get(lib_idx) else {
                continue;
            };
            let Some(syms) = self.by_debug_name.get(&lib.debug_name) else {
                continue;
            };
            let Some(sym_name) = lookup_symbol(syms, rva) else {
                continue;
            };
            thread.string_array[name_str_idx] = sym_name;
        }
    }
}

fn sidecar_path_for(trace_path: &Path) -> PathBuf {
    let mut s = trace_path.to_string_lossy().into_owned();
    if let Some(stripped) = s.strip_suffix(".gz") {
        let owned = stripped.to_owned();
        s = owned;
    }
    if let Some(stripped) = s.strip_suffix(".json") {
        s = format!("{stripped}.json.syms.json");
    } else {
        s.push_str(".syms.json");
    }
    PathBuf::from(s)
}

fn parse_hex_addr(s: &str) -> Option<u32> {
    let stripped = s.strip_prefix("0x")?;
    if stripped.is_empty() || !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(stripped, 16).ok()
}

fn resource_lib(table: &ResourceTable, res_idx: i64) -> Option<usize> {
    let i = usize::try_from(res_idx).ok()?;
    if i >= table.length {
        return None;
    }
    let raw = (*table.lib.get(i)?)?;
    usize::try_from(raw).ok()
}

fn lookup_symbol(syms: &BinarySymbols, rva: u32) -> Option<String> {
    let i = syms.entries.partition_point(|&(start, _, _)| start <= rva);
    if i == 0 {
        return None;
    }
    let (start, size, ref name) = syms.entries[i - 1];
    // `start + size` would overflow when a symbol sits near u32::MAX
    // — saturate so the check still returns the correct bool without
    // panicking in debug builds.
    let end = start.saturating_add(size);
    (rva >= start && rva < end).then(|| name.clone())
}

// --- minimal slice of the Firefox Profiler "processed profile" format ---

#[derive(Deserialize)]
struct Profile {
    meta: Meta,
    libs: Vec<Lib>,
    threads: Vec<Thread>,
}

#[derive(Deserialize)]
struct Meta {
    interval: f64,
    #[serde(rename = "startTime")]
    start_time: Option<f64>,
    #[serde(rename = "endTime")]
    end_time: Option<f64>,
}

#[derive(Deserialize)]
struct Lib {
    #[serde(rename = "debugName")]
    debug_name: String,
}

#[derive(Deserialize)]
struct Thread {
    name: String,
    samples: SamplesTable,
    #[serde(rename = "stackTable")]
    stack_table: StackTable,
    #[serde(rename = "frameTable")]
    frame_table: FrameTable,
    #[serde(rename = "funcTable")]
    func_table: FuncTable,
    #[serde(rename = "resourceTable")]
    resource_table: ResourceTable,
    #[serde(rename = "stringArray")]
    string_array: Vec<String>,
}

#[derive(Deserialize)]
struct SamplesTable {
    length: usize,
    stack: Vec<Option<usize>>,
}

#[derive(Deserialize)]
struct StackTable {
    length: usize,
    frame: Vec<usize>,
    prefix: Vec<Option<usize>>,
}

#[derive(Deserialize)]
struct FrameTable {
    length: usize,
    func: Vec<usize>,
}

#[derive(Deserialize)]
struct FuncTable {
    length: usize,
    name: Vec<usize>,
    resource: Vec<i64>,
}

#[derive(Deserialize)]
struct ResourceTable {
    length: usize,
    lib: Vec<Option<i64>>,
}

// --- .syms.json sidecar format ---

#[derive(Deserialize)]
struct SymsFile {
    string_table: Vec<String>,
    data: Vec<SymsLib>,
}

#[derive(Deserialize)]
struct SymsLib {
    debug_name: String,
    symbol_table: Vec<SymsEntry>,
}

#[derive(Deserialize)]
struct SymsEntry {
    rva: u32,
    size: u32,
    symbol: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_addr_accepts_lowercase_hex() {
        assert_eq!(parse_hex_addr("0x1f547"), Some(0x1f547));
        assert_eq!(parse_hex_addr("0xaB"), Some(0xab));
    }

    #[test]
    fn parse_hex_addr_rejects_non_addresses() {
        assert_eq!(parse_hex_addr("malloc"), None);
        assert_eq!(parse_hex_addr("0x"), None);
        assert_eq!(parse_hex_addr("0xZZZZ"), None);
        assert_eq!(parse_hex_addr("0x100000000000"), None);
    }

    #[test]
    fn lookup_symbol_finds_entry_in_range() {
        let syms = BinarySymbols {
            entries: vec![
                (100, 50, "alpha".to_owned()),
                (200, 30, "beta".to_owned()),
                (500, 100, "gamma".to_owned()),
            ],
        };
        assert_eq!(lookup_symbol(&syms, 100).as_deref(), Some("alpha"));
        assert_eq!(lookup_symbol(&syms, 149).as_deref(), Some("alpha"));
        assert_eq!(lookup_symbol(&syms, 215).as_deref(), Some("beta"));
        assert_eq!(lookup_symbol(&syms, 599).as_deref(), Some("gamma"));
    }

    #[test]
    fn lookup_symbol_returns_none_in_gaps() {
        let syms = BinarySymbols {
            entries: vec![(100, 50, "alpha".to_owned()), (200, 30, "beta".to_owned())],
        };
        assert_eq!(lookup_symbol(&syms, 50), None);
        assert_eq!(lookup_symbol(&syms, 175), None);
        assert_eq!(lookup_symbol(&syms, 250), None);
    }

    /// Regression: a symbol whose `start + size` overflows `u32`
    /// would panic in debug builds before the saturating-add fix.
    /// Real linkers don't emit such monsters but parser fuzz / a
    /// truncated sidecar can produce them, and an analyser that
    /// crashes on bad input is worse than one that returns `None`.
    #[test]
    fn lookup_symbol_does_not_panic_on_size_overflow() {
        let syms = BinarySymbols {
            entries: vec![(u32::MAX - 5, u32::MAX, "monster".to_owned())],
        };
        // Should match (rva sits inside the saturated range)…
        assert_eq!(
            lookup_symbol(&syms, u32::MAX - 1).as_deref(),
            Some("monster"),
        );
        // …and should also gracefully return None for an rva below
        // the symbol's start without panicking on the overflow check.
        assert_eq!(lookup_symbol(&syms, 0), None);
    }

    /// `lookup_symbol` correctly resolves the very last byte of a
    /// symbol's range — `start + size - 1`. The earlier `<` check
    /// already had this right; pin it explicitly so a future refactor
    /// to `<=` doesn't accidentally include the next symbol's first
    /// byte.
    #[test]
    fn lookup_symbol_excludes_byte_after_size() {
        let syms = BinarySymbols {
            entries: vec![(100, 50, "alpha".to_owned())],
        };
        assert_eq!(lookup_symbol(&syms, 149).as_deref(), Some("alpha"));
        assert_eq!(lookup_symbol(&syms, 150), None);
    }

    #[test]
    fn sidecar_path_for_strips_gz_and_appends_syms() {
        let trace = Path::new("/tmp/aozora-lsp-burst-1234-5678.json.gz");
        let sidecar = sidecar_path_for(trace);
        assert_eq!(
            sidecar.to_string_lossy(),
            "/tmp/aozora-lsp-burst-1234-5678.json.syms.json"
        );
    }

    #[test]
    fn classify_owner_pins_categories() {
        assert_eq!(
            classify_owner("aozora_lsp::state::DocState::new"),
            "aozora_lsp"
        );
        assert_eq!(classify_owner("ts_parser_parse"), "tree_sitter (C)");
        assert_eq!(classify_owner("subtree_compress"), "tree_sitter (C)");
        assert_eq!(classify_owner("iterator_descend"), "tree_sitter (C)");
        assert_eq!(classify_owner("stack__iter"), "tree_sitter (C)");
        assert_eq!(classify_owner("ropey::Rope::insert"), "ropey");
        assert_eq!(classify_owner("std::vec::Vec::push"), "std/core/alloc");
        assert_eq!(classify_owner("malloc"), "allocator/libc");
        assert_eq!(classify_owner("mmap"), "allocator/libc");
        assert_eq!(classify_owner("0x12345"), "unresolved");
        assert_eq!(classify_owner("some_random_func"), "other");
    }

    #[test]
    fn classify_alloc_pins_categories() {
        // `matches!` returns a bool; without `assert!` it's a no-op.
        // The earlier form silently passed regardless of the function's
        // output — pin every category explicitly so a regression in
        // `classify_alloc` actually fails the test.
        assert!(matches!(classify_alloc("malloc"), AllocKind::Malloc));
        assert!(matches!(classify_alloc("realloc"), AllocKind::Malloc));
        assert!(matches!(classify_alloc("calloc"), AllocKind::Malloc));
        assert!(matches!(classify_alloc("_libc_malloc"), AllocKind::Malloc));
        assert!(matches!(classify_alloc("_mmap"), AllocKind::Mmap));
        assert!(matches!(classify_alloc("mmap"), AllocKind::Mmap));
        assert!(matches!(classify_alloc("munmap"), AllocKind::Mmap));
        assert!(matches!(classify_alloc("free"), AllocKind::Free));
        assert!(matches!(classify_alloc("_libc_free"), AllocKind::Free));
        assert!(matches!(classify_alloc("brk"), AllocKind::Other));
        assert!(matches!(classify_alloc("sbrk"), AllocKind::Other));
        assert!(matches!(classify_alloc("_mprotect"), AllocKind::Other));
        assert!(matches!(classify_alloc("aozora_lsp::foo"), AllocKind::None));
        assert!(matches!(classify_alloc(""), AllocKind::None));
    }

    #[test]
    fn ratio_pct_handles_zero_denom() {
        // f64 strict-eq is fine here: ratio_pct's `denom == 0` branch
        // returns the literal `0.0_f64`; no arithmetic involved.
        assert!((ratio_pct(5, 0) - 0.0).abs() < f64::EPSILON);
    }
}
