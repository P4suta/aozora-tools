# Security policy

## Reporting a vulnerability

If you discover a security vulnerability in aozora-tools — an LSP
crash on untrusted input, a panic in the formatter on adversarial
syntax, a memory-safety issue, an HTML-injection in the VS Code
preview pane, or anything with exploitative potential — **do not open
a public issue**. Instead:

1. Preferred: open a private report via
   [GitHub Security Advisories](https://github.com/P4suta/aozora-tools/security/advisories/new).
   This lets us discuss and patch before disclosure.
2. Alternative: email the maintainer at
   `42543015+P4suta@users.noreply.github.com` with the subject
   `[aozora-tools security] <short summary>`.

Please include:

- The shortest input or reproduction steps that trigger the issue.
- The aozora-tools version / commit hash and the Rust toolchain version.
- Whether the issue is reachable via untrusted input (e.g. opening a
  document in VS Code, parsing a file the user did not author).
- Your proposed CVSS severity, if you have one in mind.

## Response expectations

- We acknowledge reports within **7 days**.
- Triage, patch, and coordinated disclosure typically complete within
  **30–60 days** for high-severity issues, faster for critical ones.
- Credits (unless you prefer anonymity) are noted in `CHANGELOG.md`
  once the fix ships.

## Scope

aozora-tools consumes the [`aozora`](https://github.com/P4suta/aozora)
parser as a tag-pinned git dependency. Vulnerabilities in the parser
itself should be reported against that repository
([Security Advisories](https://github.com/P4suta/aozora/security/advisories/new))
so the fix lands at the source layer; we will pick up the new tag
in aozora-tools as a follow-up.
