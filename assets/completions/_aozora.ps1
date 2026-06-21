
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'aozora' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'aozora'
        for ($i = 1; $i -lt $commandElements.Count; $i++) {
            $element = $commandElements[$i]
            if ($element -isnot [StringConstantExpressionAst] -or
                $element.StringConstantType -ne [StringConstantType]::BareWord -or
                $element.Value.StartsWith('-') -or
                $element.Value -eq $wordToComplete) {
                break
        }
        $element.Value
    }) -join ';'

    $completions = @(switch ($command) {
        'aozora' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('fmt', 'fmt', [CompletionResultType]::ParameterValue, 'Format aozora documents (idempotent canonicaliser)')
            [CompletionResult]::new('lint', 'lint', [CompletionResultType]::ParameterValue, 'Lint aozora documents and print diagnostics to the terminal')
            [CompletionResult]::new('check', 'check', [CompletionResultType]::ParameterValue, 'Lint aozora documents and print diagnostics to the terminal')
            [CompletionResult]::new('render', 'render', [CompletionResultType]::ParameterValue, 'Render an aozora document to HTML')
            [CompletionResult]::new('explain', 'explain', [CompletionResultType]::ParameterValue, 'Explain a diagnostic code (e.g. `aozora::unclosed-bracket`)')
            [CompletionResult]::new('lsp', 'lsp', [CompletionResultType]::ParameterValue, 'Run the language server over stdio (for editors)')
            [CompletionResult]::new('completions', 'completions', [CompletionResultType]::ParameterValue, 'Print a shell completion script to stdout')
            [CompletionResult]::new('man', 'man', [CompletionResultType]::ParameterValue, 'Print a man page (troff) to stdout')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'aozora;fmt' {
            [CompletionResult]::new('--color', '--color', [CompletionResultType]::ParameterName, 'When to colourise --diff output')
            [CompletionResult]::new('--check', '--check', [CompletionResultType]::ParameterName, 'Verify inputs are already formatted; exit 1 if any would change')
            [CompletionResult]::new('-w', '-w', [CompletionResultType]::ParameterName, 'Rewrite files in place (no-op when already canonical)')
            [CompletionResult]::new('--write', '--write', [CompletionResultType]::ParameterName, 'Rewrite files in place (no-op when already canonical)')
            [CompletionResult]::new('--diff', '--diff', [CompletionResultType]::ParameterName, 'Print a unified diff for every file that would change. Implies --check')
            [CompletionResult]::new('-l', '-l', [CompletionResultType]::ParameterName, 'List only the paths that would change (gofmt -l). Combine with -w')
            [CompletionResult]::new('--list', '--list', [CompletionResultType]::ParameterName, 'List only the paths that would change (gofmt -l). Combine with -w')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Emit the --check result as machine-readable JSON. Implies --check')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            break
        }
        'aozora;lint' {
            [CompletionResult]::new('--color', '--color', [CompletionResultType]::ParameterName, 'When to colourise output')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Emit machine-readable JSON instead of rendered diagnostics')
            [CompletionResult]::new('-q', '-q', [CompletionResultType]::ParameterName, 'Print one terse line per diagnostic: `path:line:col: sev[code]: message`')
            [CompletionResult]::new('--quiet', '--quiet', [CompletionResultType]::ParameterName, 'Print one terse line per diagnostic: `path:line:col: sev[code]: message`')
            [CompletionResult]::new('-W', '-W ', [CompletionResultType]::ParameterName, 'Treat warnings as errors for the exit code')
            [CompletionResult]::new('--error-on-warning', '--error-on-warning', [CompletionResultType]::ParameterName, 'Treat warnings as errors for the exit code')
            [CompletionResult]::new('--watch', '--watch', [CompletionResultType]::ParameterName, 'Re-run on every file change (clears the screen, like a dev server)')
            [CompletionResult]::new('--stats', '--stats', [CompletionResultType]::ParameterName, 'Print a one-line summary (files, diagnostics, elapsed) to stderr')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            break
        }
        'aozora;check' {
            [CompletionResult]::new('--color', '--color', [CompletionResultType]::ParameterName, 'When to colourise output')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Emit machine-readable JSON instead of rendered diagnostics')
            [CompletionResult]::new('-q', '-q', [CompletionResultType]::ParameterName, 'Print one terse line per diagnostic: `path:line:col: sev[code]: message`')
            [CompletionResult]::new('--quiet', '--quiet', [CompletionResultType]::ParameterName, 'Print one terse line per diagnostic: `path:line:col: sev[code]: message`')
            [CompletionResult]::new('-W', '-W ', [CompletionResultType]::ParameterName, 'Treat warnings as errors for the exit code')
            [CompletionResult]::new('--error-on-warning', '--error-on-warning', [CompletionResultType]::ParameterName, 'Treat warnings as errors for the exit code')
            [CompletionResult]::new('--watch', '--watch', [CompletionResultType]::ParameterName, 'Re-run on every file change (clears the screen, like a dev server)')
            [CompletionResult]::new('--stats', '--stats', [CompletionResultType]::ParameterName, 'Print a one-line summary (files, diagnostics, elapsed) to stderr')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            break
        }
        'aozora;render' {
            [CompletionResult]::new('-o', '-o', [CompletionResultType]::ParameterName, 'Write HTML to FILE instead of stdout')
            [CompletionResult]::new('--output', '--output', [CompletionResultType]::ParameterName, 'Write HTML to FILE instead of stdout')
            [CompletionResult]::new('--color', '--color', [CompletionResultType]::ParameterName, 'When to colourise error output')
            [CompletionResult]::new('--standalone', '--standalone', [CompletionResultType]::ParameterName, 'Wrap the fragment in a standalone HTML5 document (vertical-writing CSS)')
            [CompletionResult]::new('--open', '--open', [CompletionResultType]::ParameterName, 'Open the rendered HTML in the default browser (implies --standalone)')
            [CompletionResult]::new('--stats', '--stats', [CompletionResultType]::ParameterName, 'Print render stats (bytes in/out, elapsed) to stderr')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            break
        }
        'aozora;explain' {
            [CompletionResult]::new('--color', '--color', [CompletionResultType]::ParameterName, 'When to colourise output')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            break
        }
        'aozora;lsp' {
            [CompletionResult]::new('--stdio', '--stdio', [CompletionResultType]::ParameterName, 'Accepted for editor compatibility; the server always speaks stdio')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            break
        }
        'aozora;completions' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            break
        }
        'aozora;man' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            break
        }
        'aozora;help' {
            [CompletionResult]::new('fmt', 'fmt', [CompletionResultType]::ParameterValue, 'Format aozora documents (idempotent canonicaliser)')
            [CompletionResult]::new('lint', 'lint', [CompletionResultType]::ParameterValue, 'Lint aozora documents and print diagnostics to the terminal')
            [CompletionResult]::new('render', 'render', [CompletionResultType]::ParameterValue, 'Render an aozora document to HTML')
            [CompletionResult]::new('explain', 'explain', [CompletionResultType]::ParameterValue, 'Explain a diagnostic code (e.g. `aozora::unclosed-bracket`)')
            [CompletionResult]::new('lsp', 'lsp', [CompletionResultType]::ParameterValue, 'Run the language server over stdio (for editors)')
            [CompletionResult]::new('completions', 'completions', [CompletionResultType]::ParameterValue, 'Print a shell completion script to stdout')
            [CompletionResult]::new('man', 'man', [CompletionResultType]::ParameterValue, 'Print a man page (troff) to stdout')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'aozora;help;fmt' {
            break
        }
        'aozora;help;lint' {
            break
        }
        'aozora;help;render' {
            break
        }
        'aozora;help;explain' {
            break
        }
        'aozora;help;lsp' {
            break
        }
        'aozora;help;completions' {
            break
        }
        'aozora;help;man' {
            break
        }
        'aozora;help;help' {
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
