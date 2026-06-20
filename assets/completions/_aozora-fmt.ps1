
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'aozora-fmt' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'aozora-fmt'
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
        'aozora-fmt' {
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
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
