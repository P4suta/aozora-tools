module completions {

  # Authoring tools for aozora-flavored-markdown
  export extern aozora [
    --help(-h)                # Print help (see more with '--help')
    --version(-V)             # Print version
  ]

  def "nu-complete aozora fmt color" [] {
    [ "auto" "always" "never" ]
  }

  # Format aozora documents (idempotent canonicaliser)
  export extern "aozora fmt" [
    --check                   # Verify inputs are already formatted; exit 1 if any would change
    --write(-w)               # Rewrite files in place (no-op when already canonical)
    --diff                    # Print a unified diff for every file that would change. Implies --check
    --list(-l)                # List only the paths that would change (gofmt -l). Combine with -w
    --json                    # Emit the --check result as machine-readable JSON. Implies --check
    --color: string@"nu-complete aozora fmt color" # When to colourise --diff output
    --help(-h)                # Print help (see more with '--help')
    --version(-V)             # Print version
    ...paths: path            # Files or directories to format. Use `-`, or omit, to read stdin
  ]

  def "nu-complete aozora lint color" [] {
    [ "auto" "always" "never" ]
  }

  # Lint aozora documents and print diagnostics to the terminal
  export extern "aozora lint" [
    --json                    # Emit machine-readable JSON instead of rendered diagnostics
    --quiet(-q)               # Print one terse line per diagnostic: `path:line:col: sev[code]: message`
    --error-on-warning(-W)    # Treat warnings as errors for the exit code
    --watch                   # Re-run on every file change (clears the screen, like a dev server)
    --stats                   # Print a one-line summary (files, diagnostics, elapsed) to stderr
    --color: string@"nu-complete aozora lint color" # When to colourise output
    --help(-h)                # Print help (see more with '--help')
    --version(-V)             # Print version
    ...paths: path            # Files or directories to lint. Use `-`, or omit, to read stdin
  ]

  def "nu-complete aozora render color" [] {
    [ "auto" "always" "never" ]
  }

  # Render an aozora document to HTML
  export extern "aozora render" [
    --output(-o): path        # Write HTML to FILE instead of stdout
    --standalone              # Wrap the fragment in a standalone HTML5 document (vertical-writing CSS)
    --open                    # Open the rendered HTML in the default browser (implies --standalone)
    --stats                   # Print render stats (bytes in/out, elapsed) to stderr
    --color: string@"nu-complete aozora render color" # When to colourise error output
    --help(-h)                # Print help (see more with '--help')
    --version(-V)             # Print version
    path?: path               # The file to render. Use `-`, or omit, to read stdin
  ]

  def "nu-complete aozora explain color" [] {
    [ "auto" "always" "never" ]
  }

  # Explain a diagnostic code (e.g. `aozora::unclosed-bracket`)
  export extern "aozora explain" [
    --color: string@"nu-complete aozora explain color" # When to colourise output
    --help(-h)                # Print help (see more with '--help')
    --version(-V)             # Print version
    code?: string             # The diagnostic code (e.g. `aozora::unclosed-bracket`, or just `unclosed-bracket`). Omit to list every code
  ]

  # Run the language server over stdio (for editors)
  export extern "aozora lsp" [
    --stdio                   # Accepted for editor compatibility; the server always speaks stdio
    --help(-h)                # Print help (see more with '--help')
    --version(-V)             # Print version
  ]

  def "nu-complete aozora completions shell" [] {
    [ "bash" "zsh" "fish" "powershell" "nushell" ]
  }

  # Print a shell completion script to stdout
  export extern "aozora completions" [
    --help(-h)                # Print help (see more with '--help')
    --version(-V)             # Print version
    shell: string@"nu-complete aozora completions shell" # The shell to generate a completion script for
  ]

  # Print a man page (troff) to stdout
  export extern "aozora man" [
    --help(-h)                # Print help
    --version(-V)             # Print version
    command?: string          # Render the page for this subcommand (e.g. `lint`). Omit for the top-level `aozora` page
  ]

  # Print this message or the help of the given subcommand(s)
  export extern "aozora help" [
  ]

  # Format aozora documents (idempotent canonicaliser)
  export extern "aozora help fmt" [
  ]

  # Lint aozora documents and print diagnostics to the terminal
  export extern "aozora help lint" [
  ]

  # Render an aozora document to HTML
  export extern "aozora help render" [
  ]

  # Explain a diagnostic code (e.g. `aozora::unclosed-bracket`)
  export extern "aozora help explain" [
  ]

  # Run the language server over stdio (for editors)
  export extern "aozora help lsp" [
  ]

  # Print a shell completion script to stdout
  export extern "aozora help completions" [
  ]

  # Print a man page (troff) to stdout
  export extern "aozora help man" [
  ]

  # Print this message or the help of the given subcommand(s)
  export extern "aozora help help" [
  ]

}

export use completions *
