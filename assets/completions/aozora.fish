# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_aozora_global_optspecs
	string join \n h/help V/version
end

function __fish_aozora_needs_command
	# Figure out if the current invocation already has a command.
	set -l cmd (commandline -opc)
	set -e cmd[1]
	argparse -s (__fish_aozora_global_optspecs) -- $cmd 2>/dev/null
	or return
	if set -q argv[1]
		# Also print the command, so this can be used to figure out what it is.
		echo $argv[1]
		return 1
	end
	return 0
end

function __fish_aozora_using_subcommand
	set -l cmd (__fish_aozora_needs_command)
	test -z "$cmd"
	and return 1
	contains -- $cmd[1] $argv
end

complete -c aozora -n "__fish_aozora_needs_command" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c aozora -n "__fish_aozora_needs_command" -s V -l version -d 'Print version'
complete -c aozora -n "__fish_aozora_needs_command" -f -a "fmt" -d 'Format aozora documents (idempotent canonicaliser)'
complete -c aozora -n "__fish_aozora_needs_command" -f -a "lint" -d 'Lint aozora documents and print diagnostics to the terminal'
complete -c aozora -n "__fish_aozora_needs_command" -f -a "check" -d 'Lint aozora documents and print diagnostics to the terminal'
complete -c aozora -n "__fish_aozora_needs_command" -f -a "render" -d 'Render an aozora document to HTML'
complete -c aozora -n "__fish_aozora_needs_command" -f -a "explain" -d 'Explain a diagnostic code (e.g. `aozora::unclosed-bracket`)'
complete -c aozora -n "__fish_aozora_needs_command" -f -a "lsp" -d 'Run the language server over stdio (for editors)'
complete -c aozora -n "__fish_aozora_needs_command" -f -a "completions" -d 'Print a shell completion script to stdout'
complete -c aozora -n "__fish_aozora_needs_command" -f -a "man" -d 'Print a man page (troff) to stdout'
complete -c aozora -n "__fish_aozora_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c aozora -n "__fish_aozora_using_subcommand fmt" -l color -d 'When to colourise --diff output' -r -f -a "auto\t'Colour when stdout is a terminal (honours `NO_COLOR`)'
always\t'Always colour, even when piped'
never\t'Never colour'"
complete -c aozora -n "__fish_aozora_using_subcommand fmt" -l check -d 'Verify inputs are already formatted; exit 1 if any would change'
complete -c aozora -n "__fish_aozora_using_subcommand fmt" -s w -l write -d 'Rewrite files in place (no-op when already canonical)'
complete -c aozora -n "__fish_aozora_using_subcommand fmt" -l diff -d 'Print a unified diff for every file that would change. Implies --check'
complete -c aozora -n "__fish_aozora_using_subcommand fmt" -s l -l list -d 'List only the paths that would change (gofmt -l). Combine with -w'
complete -c aozora -n "__fish_aozora_using_subcommand fmt" -l json -d 'Emit the --check result as machine-readable JSON. Implies --check'
complete -c aozora -n "__fish_aozora_using_subcommand fmt" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c aozora -n "__fish_aozora_using_subcommand fmt" -s V -l version -d 'Print version'
complete -c aozora -n "__fish_aozora_using_subcommand lint" -l color -d 'When to colourise output' -r -f -a "auto\t'Colour when stdout is a terminal (honours `NO_COLOR`)'
always\t'Always colour, even when piped'
never\t'Never colour'"
complete -c aozora -n "__fish_aozora_using_subcommand lint" -l json -d 'Emit machine-readable JSON instead of rendered diagnostics'
complete -c aozora -n "__fish_aozora_using_subcommand lint" -s q -l quiet -d 'Print one terse line per diagnostic: `path:line:col: sev[code]: message`'
complete -c aozora -n "__fish_aozora_using_subcommand lint" -s W -l error-on-warning -d 'Treat warnings as errors for the exit code'
complete -c aozora -n "__fish_aozora_using_subcommand lint" -l watch -d 'Re-run on every file change (clears the screen, like a dev server)'
complete -c aozora -n "__fish_aozora_using_subcommand lint" -l stats -d 'Print a one-line summary (files, diagnostics, elapsed) to stderr'
complete -c aozora -n "__fish_aozora_using_subcommand lint" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c aozora -n "__fish_aozora_using_subcommand lint" -s V -l version -d 'Print version'
complete -c aozora -n "__fish_aozora_using_subcommand check" -l color -d 'When to colourise output' -r -f -a "auto\t'Colour when stdout is a terminal (honours `NO_COLOR`)'
always\t'Always colour, even when piped'
never\t'Never colour'"
complete -c aozora -n "__fish_aozora_using_subcommand check" -l json -d 'Emit machine-readable JSON instead of rendered diagnostics'
complete -c aozora -n "__fish_aozora_using_subcommand check" -s q -l quiet -d 'Print one terse line per diagnostic: `path:line:col: sev[code]: message`'
complete -c aozora -n "__fish_aozora_using_subcommand check" -s W -l error-on-warning -d 'Treat warnings as errors for the exit code'
complete -c aozora -n "__fish_aozora_using_subcommand check" -l watch -d 'Re-run on every file change (clears the screen, like a dev server)'
complete -c aozora -n "__fish_aozora_using_subcommand check" -l stats -d 'Print a one-line summary (files, diagnostics, elapsed) to stderr'
complete -c aozora -n "__fish_aozora_using_subcommand check" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c aozora -n "__fish_aozora_using_subcommand check" -s V -l version -d 'Print version'
complete -c aozora -n "__fish_aozora_using_subcommand render" -s o -l output -d 'Write HTML to FILE instead of stdout' -r -F
complete -c aozora -n "__fish_aozora_using_subcommand render" -l color -d 'When to colourise error output' -r -f -a "auto\t'Colour when stdout is a terminal (honours `NO_COLOR`)'
always\t'Always colour, even when piped'
never\t'Never colour'"
complete -c aozora -n "__fish_aozora_using_subcommand render" -l standalone -d 'Wrap the fragment in a standalone HTML5 document (vertical-writing CSS)'
complete -c aozora -n "__fish_aozora_using_subcommand render" -l open -d 'Open the rendered HTML in the default browser (implies --standalone)'
complete -c aozora -n "__fish_aozora_using_subcommand render" -l stats -d 'Print render stats (bytes in/out, elapsed) to stderr'
complete -c aozora -n "__fish_aozora_using_subcommand render" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c aozora -n "__fish_aozora_using_subcommand render" -s V -l version -d 'Print version'
complete -c aozora -n "__fish_aozora_using_subcommand explain" -l color -d 'When to colourise output' -r -f -a "auto\t'Colour when stdout is a terminal (honours `NO_COLOR`)'
always\t'Always colour, even when piped'
never\t'Never colour'"
complete -c aozora -n "__fish_aozora_using_subcommand explain" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c aozora -n "__fish_aozora_using_subcommand explain" -s V -l version -d 'Print version'
complete -c aozora -n "__fish_aozora_using_subcommand lsp" -l stdio -d 'Accepted for editor compatibility; the server always speaks stdio'
complete -c aozora -n "__fish_aozora_using_subcommand lsp" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c aozora -n "__fish_aozora_using_subcommand lsp" -s V -l version -d 'Print version'
complete -c aozora -n "__fish_aozora_using_subcommand completions" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c aozora -n "__fish_aozora_using_subcommand completions" -s V -l version -d 'Print version'
complete -c aozora -n "__fish_aozora_using_subcommand man" -s h -l help -d 'Print help'
complete -c aozora -n "__fish_aozora_using_subcommand man" -s V -l version -d 'Print version'
complete -c aozora -n "__fish_aozora_using_subcommand help; and not __fish_seen_subcommand_from fmt lint render explain lsp completions man help" -f -a "fmt" -d 'Format aozora documents (idempotent canonicaliser)'
complete -c aozora -n "__fish_aozora_using_subcommand help; and not __fish_seen_subcommand_from fmt lint render explain lsp completions man help" -f -a "lint" -d 'Lint aozora documents and print diagnostics to the terminal'
complete -c aozora -n "__fish_aozora_using_subcommand help; and not __fish_seen_subcommand_from fmt lint render explain lsp completions man help" -f -a "render" -d 'Render an aozora document to HTML'
complete -c aozora -n "__fish_aozora_using_subcommand help; and not __fish_seen_subcommand_from fmt lint render explain lsp completions man help" -f -a "explain" -d 'Explain a diagnostic code (e.g. `aozora::unclosed-bracket`)'
complete -c aozora -n "__fish_aozora_using_subcommand help; and not __fish_seen_subcommand_from fmt lint render explain lsp completions man help" -f -a "lsp" -d 'Run the language server over stdio (for editors)'
complete -c aozora -n "__fish_aozora_using_subcommand help; and not __fish_seen_subcommand_from fmt lint render explain lsp completions man help" -f -a "completions" -d 'Print a shell completion script to stdout'
complete -c aozora -n "__fish_aozora_using_subcommand help; and not __fish_seen_subcommand_from fmt lint render explain lsp completions man help" -f -a "man" -d 'Print a man page (troff) to stdout'
complete -c aozora -n "__fish_aozora_using_subcommand help; and not __fish_seen_subcommand_from fmt lint render explain lsp completions man help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
