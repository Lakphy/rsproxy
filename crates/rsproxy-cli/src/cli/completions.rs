const COMMANDS: &str =
    "run start stop restart status rules values trace tui replay ca proxy completions help";

pub(super) fn completions_cmd(args: Vec<String>) -> Result<(), String> {
    let shell = args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .ok_or_else(|| "completions requires bash, zsh, fish, or powershell".to_string())?;
    let script = match shell.to_ascii_lowercase().as_str() {
        "bash" => bash(),
        "zsh" => zsh(),
        "fish" => fish(),
        "powershell" | "pwsh" => powershell(),
        _ => return Err(format!("unsupported completion shell `{shell}`")),
    };
    print!("{script}");
    Ok(())
}

fn bash() -> String {
    format!(
        r#"# bash completion for rsproxy
_rsproxy() {{
  local current previous
  current="${{COMP_WORDS[COMP_CWORD]}}"
  previous="${{COMP_WORDS[COMP_CWORD-1]}}"
  case "$previous" in
    rules) COMPREPLY=( $(compgen -W "check ls cat edit set rm enable disable stats bench test" -- "$current") ); return ;;
    values) COMPREPLY=( $(compgen -W "ls cat set rm" -- "$current") ); return ;;
    trace) COMPREPLY=( $(compgen -W "ls get follow stats clear export" -- "$current") ); return ;;
    ca) COMPREPLY=( $(compgen -W "init status export issue install uninstall" -- "$current") ); return ;;
    proxy) COMPREPLY=( $(compgen -W "status on off" -- "$current") ); return ;;
    completions) COMPREPLY=( $(compgen -W "bash zsh fish powershell" -- "$current") ); return ;;
  esac
  COMPREPLY=( $(compgen -W "{COMMANDS}" -- "$current") )
}}
complete -F _rsproxy rsproxy
"#
    )
}

fn zsh() -> String {
    format!(
        r#"#compdef rsproxy
_rsproxy() {{
  local -a commands
  commands=({})
  if (( CURRENT == 2 )); then
    _describe 'command' commands
    return
  fi
  case $words[2] in
    rules) _values 'rules command' check ls cat edit set rm enable disable stats bench test ;;
    values) _values 'values command' ls cat set rm ;;
    trace) _values 'trace command' ls get follow stats clear export ;;
    ca) _values 'ca command' init status export issue install uninstall ;;
    proxy) _values 'proxy command' status on off ;;
    completions) _values 'shell' bash zsh fish powershell ;;
  esac
}}
_rsproxy "$@"
"#,
        COMMANDS.replace(' ', "\n    ")
    )
}

fn fish() -> String {
    let mut script = String::from("# fish completion for rsproxy\ncomplete -c rsproxy -f\n");
    for command in COMMANDS.split_whitespace() {
        script.push_str(&format!(
            "complete -c rsproxy -n '__fish_use_subcommand' -a '{command}'\n"
        ));
    }
    for (parent, children) in [
        (
            "rules",
            "check ls cat edit set rm enable disable stats bench test",
        ),
        ("values", "ls cat set rm"),
        ("trace", "ls get follow stats clear export"),
        ("ca", "init status export issue install uninstall"),
        ("proxy", "status on off"),
        ("completions", "bash zsh fish powershell"),
    ] {
        script.push_str(&format!(
            "complete -c rsproxy -n '__fish_seen_subcommand_from {parent}' -a '{children}'\n"
        ));
    }
    script
}

fn powershell() -> String {
    format!(
        r#"# PowerShell completion for rsproxy
Register-ArgumentCompleter -Native -CommandName rsproxy -ScriptBlock {{
  param($wordToComplete, $commandAst, $cursorPosition)
  $commands = '{COMMANDS}'.Split(' ')
  $commands | Where-Object {{ $_ -like "$wordToComplete*" }} | ForEach-Object {{
    [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
  }}
}}
"#
    )
}
