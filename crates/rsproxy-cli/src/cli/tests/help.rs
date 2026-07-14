use crate::cli::command::Cli;
use clap::{Command, CommandFactory};

#[test]
fn every_visible_command_and_argument_is_documented() {
    let mut root = Cli::command();
    root.build();
    assert_documented(&root, "rsproxy");
}

fn assert_documented(command: &Command, path: &str) {
    assert!(
        command.get_about().is_some() || command.get_long_about().is_some(),
        "{path} has no command description"
    );

    for argument in command.get_arguments() {
        let id = argument.get_id().as_str();
        if matches!(id, "help" | "version") || argument.is_hide_set() {
            continue;
        }
        assert!(
            argument.get_help().is_some() || argument.get_long_help().is_some(),
            "{path} argument {id} has no help text"
        );
    }

    for subcommand in command.get_subcommands() {
        if subcommand.get_name() == "help" || subcommand.is_hide_set() {
            continue;
        }
        assert_documented(subcommand, &format!("{path} {}", subcommand.get_name()));
    }
}

#[test]
fn root_help_contains_a_complete_first_run_and_recovery_path() {
    let help = Cli::command().render_long_help().to_string();
    for expected in [
        "QUICK START:",
        "rsproxy ca init",
        "rsproxy ca install --dry-run",
        "rsproxy start",
        "rsproxy proxy on --all --dry-run",
        "rsproxy tui",
        "rsproxy proxy off --all",
        "rsproxy stop",
        "CONFIGURATION:",
        "RSPROXY_LOG_FORMAT",
    ] {
        assert!(help.contains(expected), "root help omitted {expected:?}");
    }
}

#[test]
fn representative_help_explains_inputs_defaults_safety_and_examples() {
    let root = Cli::command();
    let cases = [
        (
            &["run"][..],
            &["Proxy listener:", "built-in default: 8mb", "EXAMPLES:"][..],
        ),
        (
            &["rules", "test"][..],
            &[
                "Absolute request URL",
                "Name: value",
                "Simulate request metadata",
                "network traffic",
            ][..],
        ),
        (
            &["values", "set"][..],
            &["stdin", "--file", "EXAMPLES:"][..],
        ),
        (
            &["trace", "clear"][..],
            &["cannot be undone", "does not prompt for confirmation"][..],
        ),
        (
            &["ca", "install"][..],
            &["elevated privileges", "--dry-run", "Only install a CA"][..],
        ),
        (
            &["proxy", "on"][..],
            &["--service", "--all", "SAFE WORKFLOW:"][..],
        ),
    ];

    for (path, expected) in cases {
        let command = command_at_path(&root, path);
        let mut command = command.clone();
        let help = command.render_long_help().to_string();
        for needle in expected {
            assert!(
                help.contains(needle),
                "rsproxy {} help omitted {needle:?}",
                path.join(" ")
            );
        }
    }
}

fn command_at_path<'a>(root: &'a Command, path: &[&str]) -> &'a Command {
    path.iter().fold(root, |command, name| {
        command
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == *name)
            .unwrap_or_else(|| panic!("missing command path component {name}"))
    })
}
