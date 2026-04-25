//! Shell completion generation for agentic-loop.

/// Generate shell completions for the given shell.
pub fn generate_completions(shell: &str) -> String {
    let commands = [
        "list", "show", "run", "resume", "sessions", "providers",
        "daemon", "schedule", "sync", "tools", "completions", "help",
    ];
    let subcommands: &[(&str, &[&str])] = &[
        ("providers", &["list", "show", "refresh"]),
        ("schedule", &["add", "list", "remove"]),
        ("sync", &["add", "pull", "status"]),
    ];

    match shell {
        "bash" => generate_bash(&commands, subcommands),
        "zsh" => generate_zsh(&commands, subcommands),
        "fish" => generate_fish(&commands, subcommands),
        _ => format!("# Unsupported shell: {}. Supported: bash, zsh, fish\n", shell),
    }
}

fn generate_bash(commands: &[&str], subcommands: &[(&str, &[&str])]) -> String {
    let cmds = commands.join(" ");
    let mut subs = String::new();
    for (parent, subs_list) in subcommands {
        let sl = subs_list.join(" ");
        subs.push_str(&format!(
            r#"
  if [ "${{COMP_WORDS[1]}}" = "{p}" ] && [ $COMP_CWORD -eq 2 ]; then
    COMPREPLY=($(compgen -W '{sl}' -- $cur))
    return 0
  fi
"#,
            p = parent,
            sl = sl,
        ));
    }

    format!(
        r#"#!/bin/bash
# agentic-loop bash completions

_agentic_loop_completions() {{
  local cur
  COMPREPLY=()
  cur="${{COMP_WORDS[COMP_CWORD]}}"

  if [ $COMP_CWORD -eq 1 ]; then
    COMPREPLY=($(compgen -W '{cmds}' -- $cur))
    return 0
  fi
{subs}}}

complete -F _agentic_loop_completions agentic-loop
"#,
        cmds = cmds,
        subs = subs,
    )
}

fn generate_zsh(commands: &[&str], subcommands: &[(&str, &[&str])]) -> String {
    let cmds: Vec<String> = commands.iter().map(|c| format!("'{}'", c)).collect();
    let cmds_list = cmds.join("\n    ");

    let mut cases = String::new();
    for (parent, subs) in subcommands {
        let sl = subs.join(" ");
        cases.push_str(&format!(
            r#"        {p})
          _arguments '1:subcommand:({sl})'
          ;;
"#,
            p = parent,
            sl = sl,
        ));
    }

    format!(
        r#"#compdef agentic-loop
# agentic-loop zsh completions

_agentic_loop() {{
  local -a commands
  commands=(
    {cmds}
  )

  _arguments -C \
    '1:command:->command' \
    '*::arg:->args'

  case $state in
    command)
      _describe 'command' commands
      ;;
    args)
      case $words[1] in
{cases}      esac
      ;;
  esac
}}

_agentic_loop "$@"
"#,
        cmds = cmds_list,
        cases = cases,
    )
}

fn generate_fish(commands: &[&str], subcommands: &[(&str, &[&str])]) -> String {
    let mut script = String::from("# agentic-loop fish completions\n\n");

    for cmd in commands {
        script.push_str(&format!(
            "complete -c agentic-loop -n '__fish_use_subcommand' -a '{}'\n",
            cmd
        ));
    }

    for (parent, subs) in subcommands {
        for sub in *subs {
            script.push_str(&format!(
                "complete -c agentic-loop -n '__fish_seen_subcommand_from {}' -a '{}'\n",
                parent, sub
            ));
        }
    }

    script
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bash_completions() {
        let script = generate_completions("bash");
        assert!(script.contains("#!/bin/bash"));
        assert!(script.contains("_agentic_loop_completions"));
        assert!(script.contains("complete -F"));
        assert!(script.contains("daemon"));
        assert!(script.contains("providers"));
    }

    #[test]
    fn test_zsh_completions() {
        let script = generate_completions("zsh");
        assert!(script.contains("#compdef agentic-loop"));
        assert!(script.contains("_agentic_loop"));
        assert!(script.contains("daemon"));
    }

    #[test]
    fn test_fish_completions() {
        let script = generate_completions("fish");
        assert!(script.contains("agentic-loop"));
        assert!(script.contains("__fish_use_subcommand"));
        assert!(script.contains("daemon"));
    }

    #[test]
    fn test_unsupported_shell() {
        let script = generate_completions("powershell");
        assert!(script.contains("Unsupported"));
    }

    #[test]
    fn test_bash_has_subcommands() {
        let script = generate_completions("bash");
        assert!(script.contains("providers"));
        assert!(script.contains("schedule"));
        assert!(script.contains("sync"));
    }

    #[test]
    fn test_zsh_has_subcommands() {
        let script = generate_completions("zsh");
        assert!(script.contains("add"));
        assert!(script.contains("pull"));
        assert!(script.contains("status"));
    }

    #[test]
    fn test_fish_has_subcommands() {
        let script = generate_completions("fish");
        assert!(script.contains("__fish_seen_subcommand_from providers"));
        assert!(script.contains("__fish_seen_subcommand_from schedule"));
    }
}
