# Release process

`aozora-tools` runs three release tracks independently:

| Track | Tag | What ships |
|---|---|---|
| Rust binaries | `vX.Y.Z`        | `aozora-fmt` + `aozora-lsp` archives on GitHub Releases |
| VS Code extension | `vscode-vX.Y.Z` | `.vsix` published to Marketplace + Open VSX |
| `aozora` parser pin | (bumped via PR) | `Cargo.toml` `tag = "vA.B.C"` field |

## Versioning contract

- **`aozora-tools`** follows [SemVer 2.0.0](https://semver.org/)
  with the 0.x major-zero contract — any `0.MINOR` bump may break
  API.
- **`aozora` parser pin** is updated as a deliberate workspace
  bump, never silently. It surfaces in `Cargo.toml` as a one-line
  `tag = "..."` diff and a corresponding `CHANGELOG.md` entry.
- **VS Code extension** versions independently on its own
  Marketplace cadence. The bundled `aozora-lsp` is whatever
  `target/<triple>/release/aozora-lsp` is current at tag time.

## Pre-flight

Before tagging, confirm:

```sh
# 1. CI is green on the commit you intend to tag.
gh run list --branch main --limit 5

# 2. Local gates pass against a clean tree.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-targets --locked
cargo doc --workspace --no-deps --document-private-items --locked
cargo deny --all-features check

# 3. CHANGELOG is up to date.
git cliff --unreleased --output - | head -30
```

## Cutting a Rust binary release

1. **Bump versions** in `Cargo.toml` (`[workspace.package].version`)
   and any per-crate overrides. Only the workspace-level field
   matters for binaries; member crates inherit it.
2. **Regenerate CHANGELOG.md** with git-cliff:
   ```sh
   git cliff --tag vX.Y.Z --output CHANGELOG.md
   ```
3. **Commit the bump** with a `chore(release): bump workspace to
   vX.Y.Z` message.
4. **Tag and push**:
   ```sh
   git tag -a vX.Y.Z -m "vX.Y.Z"
   git push origin main vX.Y.Z
   ```
5. **`release.yml` runs automatically.** It cross-compiles
   `aozora-fmt` + `aozora-lsp` to all matrix targets, packages
   `.tar.gz` / `.zip` archives + `SHA256SUMS`, regenerates the
   release notes via git-cliff, and publishes the GitHub Release.

## Cutting a VS Code extension release

1. **Bump** `editors/vscode/package.json` `version`.
2. **Commit** with `chore(vscode): bump extension to X.Y.Z`.
3. **Tag and push**:
   ```sh
   git tag -a vscode-vX.Y.Z -m "vscode-vX.Y.Z"
   git push origin main vscode-vX.Y.Z
   ```
4. **`release-vscode.yml` runs automatically.** It cross-compiles
   `aozora-lsp` for every `.vsix` platform, runs `vsce package` for
   each, then `vsce publish` to Marketplace and `ovsx publish` to
   Open VSX.

## Bumping the `aozora` parser pin

This is a routine PR, not a release event. The bump:

1. Updates the two `tag = "..."` lines in
   `[workspace.dependencies]`.
2. Runs `cargo update -p aozora -p aozora-encoding` to refresh the
   lockfile.
3. Adds a CHANGELOG entry describing the parser version change and
   any API surface that aozora-tools touches differently as a
   result.
4. Verifies the gates pass (especially `cargo test` and `cargo
   nextest`, since the parser is the largest API surface
   aozora-tools consumes).

The next aozora-tools binary release that includes the bump
mentions it in its release notes.

## Hotfix flow

If a release ships a regression that needs a same-day fix:

1. Branch from the release tag (`git switch -c hotfix-vX.Y.(Z+1) vX.Y.Z`).
2. Apply the minimal patch + a regression test that pins it.
3. Bump to `vX.Y.(Z+1)`, regenerate CHANGELOG, tag, push.
4. Open a PR back to `main` to forward-port the fix; do not let
   `main` regress.

The forward-port PR is the one place this workflow tolerates a
diverged history; everywhere else, the tags follow `main`'s linear
history.
