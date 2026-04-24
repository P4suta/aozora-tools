# Stage 3: core-library extraction (deferred)

This is the **deferred** stage of the rollout described in
`afm/docs/adr/0009-authoring-tools-live-in-sibling-repositories.md`.
It documents *how* to split the afm parser into its own repository
(`P4suta/afm-core`) so that both the current `afm` repo and this
`aozora-tools` repo end up as siblings of the same core library.

Stage 3 is **not** scheduled. It should be executed only after all
of the following trigger conditions are met:

1. `aozora-tools` is past its MVP and in active use (an editor has
   it installed and the LSP handles a real writing session end to
   end — not just toy inputs).
2. At least one breaking API change has shipped on the `afm`
   crates. That is concrete evidence of which parts of the surface
   actually churn and therefore want a dedicated release cadence.
3. A second non-`afm` consumer of the parser exists or is about to
   exist. Without that, splitting now is YAGNI.

The point of writing this document *before* the triggers fire is
to keep the migration path unambiguous: when the decision to split
is made, the mechanical work below is the plan, not a design
exercise.

---

## Target state

### Before (today)

```
P4suta/afm/
├── crates/afm-syntax
├── crates/afm-lexer
├── crates/afm-parser
├── crates/afm-encoding
├── crates/afm-cli
├── crates/afm-corpus
├── crates/afm-book
├── crates/xtask
├── upstream/comrak/          (vendored fork, ADR-0001)
├── spec/
│   ├── aozora/               (fixtures + spec snapshots)
│   ├── commonmark-0.31.2.json
│   └── gfm-0.29-gfm.json
└── docs/adr/                 (0001..0009)

P4suta/aozora-tools/          (this repo)
├── crates/aozora-fmt
├── crates/aozora-lsp
└── editors/vscode/
```

### After Stage 3

```
P4suta/afm-core/              (new repo, carries parser + spec + comrak)
├── crates/afm-syntax
├── crates/afm-lexer
├── crates/afm-parser
├── crates/afm-encoding
├── upstream/comrak/
├── spec/commonmark-0.31.2.json
├── spec/gfm-0.29-gfm.json
├── spec/sources/             (only parser-relevant fixtures)
├── docs/adr/                 (the ADRs that govern the parser:
│                              0001, 0004, 0006, 0007, 0008, and the
│                              new 0010 that records this split)
└── xtask (parser-side dev automation only)

P4suta/afm/                   (this repo, now a client of afm-core)
├── crates/afm-cli            (afm render / afm check binary)
├── crates/afm-corpus         (17k-work sweep source)
├── crates/afm-book           (mdbook site)
├── spec/aozora/              (aozora-specific fixtures / spec docs)
├── docs/adr/                 (0002, 0003, 0005, 0009, and the new
│                              0010 that mirrors the split note)
└── (depends on afm-core via git dep)

P4suta/aozora-tools/          (unchanged structurally)
└── (flips afm-parser dep from "../afm/..." to the afm-core tag)
```

---

## Execution plan

**Pre-flight** (do these first in a throwaway branch):

1. Write **ADR-0010** on the `afm` side titled something like
   "Parser library extraction into P4suta/afm-core" recording the
   concrete trigger that fired, and the pin of afm-core's initial
   commit that the `afm` and `aozora-tools` repos will depend on.
2. Confirm `afm` has no uncommitted changes. `jj status` / `jj log`
   on both `afm` and `aozora-tools` should be clean.
3. Snapshot the `afm` repo via `jj bookmark create pre-extraction`
   so you can walk back if the migration hits a snag.

**Step 1 — create afm-core with history**

```bash
cd ~/projects
git clone --no-local afm afm-core
cd afm-core

# Preserve only the paths that belong to afm-core. git filter-repo
# rewrites history so that commits that never touched these paths
# become empty (and are dropped); commits that touched both these
# paths and removed ones keep the relevant diff only.
git filter-repo \
    --path crates/afm-syntax \
    --path crates/afm-lexer \
    --path crates/afm-parser \
    --path crates/afm-encoding \
    --path upstream/comrak \
    --path spec/commonmark-0.31.2.json \
    --path spec/gfm-0.29-gfm.json \
    --path spec/sources \
    --path docs/adr/0001-fork-comrak-vendor-in-tree.md \
    --path docs/adr/0004-accent-decomposition-preparse.md \
    --path docs/adr/0006-lint-profile-policy.md \
    --path docs/adr/0007-corpus-sweep-strategy.md \
    --path docs/adr/0008-aozora-first-lexer.md \
    --path crates/afm-test-utils \
    --path Cargo.toml \
    --path Justfile \
    --path rust-toolchain.toml \
    --path lefthook.yml \
    --path clippy.toml \
    --path deny.toml \
    --path .gitignore \
    --path README.md \
    --path LICENSE-APACHE \
    --path LICENSE-MIT
```

(Adjust the path list against `ls` in the current `afm/` tree at
extraction time; new top-level files may have appeared between now
and then.)

Edit `afm-core/Cargo.toml` so `[workspace] members` lists only the
4 library crates + `afm-test-utils` + a slim `xtask`. Move
`crates/xtask/src/main.rs`'s subcommands so only the parser-side
ones (`upstream-diff`, `upstream-sync`, `spec-refresh`, `new-adr`)
remain; the corpus / book subcommands stay on the `afm` side.

Commit the afm-core-side README that describes it as "parser +
spec + vendored comrak for aozora-flavored-markdown".

**Step 2 — shrink the current afm repo**

Back in `~/projects/afm/`, delete the extracted paths. Do this
inside a working copy commit so the history records the
subtraction as a deliberate split point:

```bash
cd ~/projects/afm
jj new -m "afm: extract parser into P4suta/afm-core (ADR-0010)"

rm -rf crates/afm-syntax crates/afm-lexer crates/afm-parser \
       crates/afm-encoding upstream/comrak
rm spec/commonmark-0.31.2.json spec/gfm-0.29-gfm.json
rm -rf spec/sources
# Keep spec/aozora/ — it belongs to the authoring surface.
# Keep docs/adr/0002, 0003, 0005, 0009.
# Delete the ADRs that moved to afm-core (0001, 0004, 0006-0008).
rm docs/adr/0001-fork-comrak-vendor-in-tree.md \
   docs/adr/0004-accent-decomposition-preparse.md \
   docs/adr/0006-lint-profile-policy.md \
   docs/adr/0007-corpus-sweep-strategy.md \
   docs/adr/0008-aozora-first-lexer.md

# Flip Cargo dependencies to afm-core
# (edit crates/afm-cli/Cargo.toml and crates/afm-corpus/Cargo.toml)
# afm-parser = { git = "https://github.com/P4suta/afm-core", tag = "v0.1.0" }
```

Write **ADR-0010** on the `afm` side mirroring the split note. Run
`just ci` — most tests move to afm-core, what's left on afm side
is `afm-cli` integration + `afm-corpus` sweep + book build.

**Step 3 — flip aozora-tools to afm-core**

```bash
cd ~/projects/aozora-tools
jj new -m "aozora-tools: switch afm deps to P4suta/afm-core"

# Edit Cargo.toml:
# [workspace.dependencies]
# afm-parser   = { git = "https://github.com/P4suta/afm-core", tag = "v0.1.0" }
# afm-lexer    = { git = "https://github.com/P4suta/afm-core", tag = "v0.1.0" }
# afm-syntax   = { git = "https://github.com/P4suta/afm-core", tag = "v0.1.0" }
# afm-encoding = { git = "https://github.com/P4suta/afm-core", tag = "v0.1.0" }

cargo build --workspace && cargo test --workspace
```

**Step 4 — consolidate the new topology**

- Tag `afm-core` at `v0.1.0` (the same tag the ADR-0010 pin refers to).
- Tag `afm` at `v0.2.0` (breaking: moved parser away).
- Push all three repos (when / if public distribution happens).

---

## Things that break

Anticipated breakage that has to be handled explicitly during the
cut-over:

- **Corpus sweep** (`crates/afm-parser/tests/corpus_sweep.rs`) uses
  `afm-corpus` as a dev-dep. After extraction the test file moves
  to `afm-core` but `afm-corpus` stays on `afm`. Resolution: move
  the sweep invariants into `afm-core` with an in-memory minimal
  corpus, and keep the 17k-work real sweep on the `afm` side as a
  downstream integration test that depends on both `afm-core` and
  `afm-corpus`.
- **Golden 56656** (`crates/afm-parser/tests/golden_56656.rs`) uses
  `spec/aozora/fixtures/56656/…`. The fixture stays under
  `spec/aozora/`, which belongs on the `afm` side. Resolution: the
  golden moves to an `afm-cli` integration test, not a parser-crate
  test.
- **xtask subcommands** are currently a flat set in
  `crates/xtask/src/main.rs`. `upstream-diff` / `upstream-sync` /
  `spec-refresh` belong on `afm-core`; `new-adr` is useful on both;
  corpus / book commands stay on `afm`.
- **Memory files** (`~/.claude/projects/-home-yasunobu-projects-afm/memory/`)
  reference `P4suta/afm` paths. After extraction, the `afm` memory
  is unchanged, but you may want a new per-project memory dir for
  `afm-core`.

---

## Rollback

If Stage 3 goes sideways during execution:

1. `afm-core` is a fresh clone-derived repo, discard it.
2. The `afm` working copy change from Step 2 can be undone with
   `jj abandon @` to drop the "extract" commit before anyone else
   pulls it, or `jj new @-` + reverse-delete if it has already been
   advertised.
3. `aozora-tools`'s Cargo.toml flip is a single PR, revertible.

The reason to preserve `jj bookmark pre-extraction` before starting
is exactly this: a three-command rollback to the state before any
extraction work happened.
