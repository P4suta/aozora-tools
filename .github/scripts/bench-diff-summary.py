#!/usr/bin/env python3
"""Convert criterion's `--baseline` output into a compact PR-comment
markdown table.

Criterion prints, per bench:

    Benchmarking apply_changes/insert_one_char_bouten_6mb: Analyzing
    apply_changes/insert_one_char_bouten_6mb
                            time:   [148.32 ms 150.08 ms 152.07 ms]
                            change: [-3.6404% -1.6552% +0.5683%] (p = 0.15 > 0.05)
                            No change in performance detected.

We pull the bench name, the median time, the median % change, the
p-value (so reviewers can tell noise from real moves), and a verdict
emoji. Output is a one-table markdown blob suitable for posting as
a sticky PR comment.

Verdict thresholds (median % change, p < 0.05):
  | improved >= 5 % | regressed >= 5 % | regressed >= 15 % | regressed >= 25 % |
  |       OK ✅     |    NOTABLE ⚠️    |   WARNING 🚨      |   FAILURE ❌      |

Failures only set the exit code (used by CI to mark the run as
failed without blocking the comment); the actual gate live in the
workflow.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# A criterion bench result block looks like:
#
#     <bench-name>
#                             time:   [<lo> <unit> <med> <unit> <hi> <unit>]
#                             change: [<lo>% <med>% <hi>%] (p = <p> ...)
#                             <verdict line>
# Note: criterion writes a Unicode minus (U+2212, `−`) — NOT ASCII
# `-` — for negative percentages in the change line. The class
# `[-−]` covers both; the bench-name line and `time:` row use ASCII
# characters only so the standard `-` is enough there.
BENCH_RE = re.compile(
    r"^(?P<name>[A-Za-z0-9_:./\-]+)\n"
    r"\s+time:\s+\[(?P<t_lo>[\d.]+)\s+(?P<t_unit_lo>\w+)\s+"
    r"(?P<t_med>[\d.]+)\s+(?P<t_unit_med>\w+)\s+"
    r"(?P<t_hi>[\d.]+)\s+(?P<t_unit_hi>\w+)\]\n"
    r"\s+change:\s+\[(?P<c_lo>[-−]?[\d.]+)%\s+"
    r"(?P<c_med>[-−]?[\d.]+)%\s+"
    r"(?P<c_hi>[-−]?[\d.]+)%\]\s+\(p\s*=\s*(?P<p>[\d.]+)",
    re.MULTILINE,
)


def parse_signed_pct(raw: str) -> float:
    """Convert criterion's `change:` cell to a Python float, normalising
    the Unicode minus that may appear on negative percentages."""
    return float(raw.replace("−", "-"))

# Verdict thresholds (% change, signed; positive = slower).
NOTABLE_PCT = 5.0
WARNING_PCT = 15.0
FAILURE_PCT = 25.0
P_SIGNIFICANT = 0.05


def verdict(change_pct: float, p_value: float) -> tuple[str, bool]:
    """Returns (label, is_failure)."""
    if p_value >= P_SIGNIFICANT:
        return ("noise", False)
    if change_pct <= -NOTABLE_PCT:
        return ("improved ✅", False)
    if change_pct >= FAILURE_PCT:
        return ("FAILURE ❌", True)
    if change_pct >= WARNING_PCT:
        return ("regressed 🚨", False)
    if change_pct >= NOTABLE_PCT:
        return ("notable ⚠️", False)
    return ("ok", False)


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        sys.stderr.write(f"usage: {argv[0]} <bench-pr.log>\n")
        return 2

    log = Path(argv[1]).read_text(encoding="utf-8", errors="replace")
    matches = list(BENCH_RE.finditer(log))

    if not matches:
        # Either the baseline didn't exist (PR run with no main
        # baseline yet) or every bench errored. Surface this as a
        # neutral comment so the reviewer isn't confused by silence.
        print("# bench-diff\n")
        print("No bench baseline available — first run on this branch, "
              "or `criterion-baseline-main` artifact missing on `main`.\n")
        print("Once `main` is rebuilt this comment will populate "
              "with per-bench wall-time deltas vs that baseline.")
        return 0

    rows: list[str] = []
    has_failure = False
    for m in matches:
        change = parse_signed_pct(m["c_med"])
        p_value = float(m["p"])
        label, is_failure = verdict(change, p_value)
        has_failure |= is_failure
        rows.append(
            f"| `{m['name']}` "
            f"| {m['t_med']} {m['t_unit_med']} "
            f"| {change:+.2f} % "
            f"| {p_value:.3f} "
            f"| {label} |"
        )

    print("# bench-diff vs `main`\n")
    print("Wall-time per-edit deltas from criterion. Insignificant moves (p ≥ 0.05) are tagged as noise.\n")
    print("| bench | median | Δ | p | verdict |")
    print("|---|---|---|---|---|")
    print("\n".join(rows))
    print()
    print("Thresholds: improved/notable/regressed at 5/15/25 % median Δ, p < 0.05.")

    return 1 if has_failure else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
