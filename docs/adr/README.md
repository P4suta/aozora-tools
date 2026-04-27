# Architecture Decision Records

Every ADR documents one significant architectural choice: its
context, the decision, the consequences (positive and negative),
the alternatives we considered (and why we rejected them), and
the verification we used to confirm the decision held up.

| #    | Title                                                                              | Status   |
|------|------------------------------------------------------------------------------------|----------|
| 0001 | [`ArcSwap` snapshot for wait-free LSP reads](0001-arcswap-snapshot.md)             | accepted |
| 0002 | [`ropey::Rope` buffer + tree-sitter chunked input](0002-rope-buffer.md)             | accepted |
| 0003 | [Incremental gaiji-span rebuild via `Tree::changed_ranges`](0003-incremental-gaiji-rebuild.md) | accepted |

## When to write an ADR

Write one when the decision is:

- **Architecture-shaping** — affects how multiple modules
  interact, not just an internal implementation choice. Picking a
  Rope vs a String for the document buffer is an ADR; picking
  `Vec` vs `SmallVec` for a 4-element local list is not.
- **Hard to reverse** — once committed, it'd take a substantial
  refactor to undo. The point of the ADR is so future readers
  understand *why* a "weird" choice was made before they try to
  "fix" it.
- **Bench- or data-driven** — the decision has measurable
  consequences and the data should outlive the PR description.
  Wall-time reductions, allocation shifts, lock-contention
  observations all belong here.

## When NOT to write an ADR

Skip if:

- The decision is local to one file or function.
- The decision is reversible at low cost.
- The decision is a coding convention (those go in CLAUDE.md or
  the workspace's lint config, not an ADR).

## Format

The template is straightforward; every ADR has:

1. **Status** — accepted / superseded / rejected.
2. **Context** — what problem are we solving, what bench data or
   incident motivated this.
3. **Decision** — the actual choice, with code skeleton if it
   helps.
4. **Consequences** — both directions, honestly named. Negatives
   that don't make us reverse the decision are still negatives.
5. **Alternatives considered** — the path NOT taken and why.
   Future readers need to know we did look.
6. **Verification** — bench numbers, test counts, profile output.
   The data that pinned the decision.
