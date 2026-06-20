module completions {

  # Language Server for aozora-flavored-markdown (speaks LSP over stdio)
  export extern aozora-lsp [
    --stdio                   # Speak LSP over stdio. Accepted for editor compatibility; this is the only supported transport, so the flag is a no-op
    --help(-h)                # Print help (see more with '--help')
    --version(-V)             # Print version
  ]

}

export use completions *
