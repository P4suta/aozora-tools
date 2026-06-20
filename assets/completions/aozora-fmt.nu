module completions {

  def "nu-complete aozora-fmt color" [] {
    [ "auto" "always" "never" ]
  }

  # Idempotent formatter for aozora-flavored-markdown
  export extern aozora-fmt [
    --check                   # Verify inputs are already formatted; exit 1 if any would change
    --write(-w)               # Rewrite files in place (no-op when already canonical)
    --diff                    # Print a unified diff for every file that would change. Implies --check
    --list(-l)                # List only the paths that would change (gofmt -l). Combine with -w
    --json                    # Emit the --check result as machine-readable JSON. Implies --check
    --color: string@"nu-complete aozora-fmt color" # When to colourise --diff output
    --help(-h)                # Print help (see more with '--help')
    --version(-V)             # Print version
    ...paths: path            # Files or directories to format. Use `-`, or omit, to read stdin
  ]

}

export use completions *
