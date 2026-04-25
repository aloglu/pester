use clap::builder::{
    styling::{AnsiColor, Effects},
    Styles,
};
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "pester")]
#[command(about = "Reminder notifications that repeat until you mark them done.")]
#[command(styles = cli_styles())]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

fn cli_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Blue.on_default() | Effects::BOLD)
        .usage(AnsiColor::Blue.on_default() | Effects::BOLD)
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Yellow.on_default())
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Add a reminder.
    #[command(
        override_usage = "pester add <ID> --time <TIME> --title <TITLE> --message <MESSAGE> [OPTIONS]"
    )]
    Add {
        id: String,
        #[arg(long)]
        time: String,
        #[arg(long, default_value = "5m")]
        every: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        message: String,
        #[arg(long, value_name = "HH:MM", help = "Repeat until this local time")]
        until: Option<String>,
        #[arg(
            long = "for",
            value_name = "DURATION",
            help = "Repeat for this long after the scheduled time"
        )]
        active_for: Option<String>,
        #[arg(
            long = "max",
            value_name = "COUNT",
            help = "Limit notifications per reminder window"
        )]
        max_notifications: Option<u32>,
    },
    /// Change an existing reminder.
    #[command(override_usage = "pester set <ID> [OPTIONS]")]
    Set {
        id: String,
        #[arg(long)]
        time: Option<String>,
        #[arg(long)]
        every: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        message: Option<String>,
        #[arg(long, value_name = "HH:MM", help = "Repeat until this local time")]
        until: Option<String>,
        #[arg(
            long = "for",
            value_name = "DURATION",
            help = "Repeat for this long after the scheduled time"
        )]
        active_for: Option<String>,
        #[arg(
            long = "max",
            value_name = "COUNT",
            help = "Limit notifications per reminder window"
        )]
        max_notifications: Option<u32>,
        #[arg(long, help = "Reset the explicit until time")]
        clear_until: bool,
        #[arg(long = "clear-for", help = "Reset the explicit duration window")]
        clear_active_for: bool,
        #[arg(long = "clear-max", help = "Reset the notification count limit")]
        clear_max_notifications: bool,
    },
    /// Mark a reminder done for today.
    #[command(override_usage = "pester done <ID> | --all")]
    Done(TargetArgs),
    /// Mark a reminder not done for today.
    #[command(override_usage = "pester undone <ID> | --all")]
    Undone(TargetArgs),
    /// Remove a reminder.
    #[command(override_usage = "pester remove <ID> | --all")]
    Remove(TargetArgs),
    /// Show configured reminders.
    #[command(override_usage = "pester show <ID> | --all")]
    Show(TargetArgs),
    /// Enable a reminder.
    #[command(override_usage = "pester enable <ID> | --all")]
    Enable(TargetArgs),
    /// Disable a reminder.
    #[command(override_usage = "pester disable <ID> | --all")]
    Disable(TargetArgs),
    /// Send a test notification.
    #[command(override_usage = "pester test <ID> | --all")]
    Test(TargetArgs),
    /// Manage confirmation settings.
    Confirm {
        #[command(subcommand)]
        command: ConfirmCommand,
    },
    /// Manage pester system integration.
    System {
        #[command(subcommand)]
        command: SystemCommand,
    },
}

#[derive(Debug, Args)]
pub struct TargetArgs {
    #[arg(
        value_name = "ID",
        required_unless_present = "all",
        conflicts_with = "all"
    )]
    pub id: Option<String>,
    #[arg(long, conflicts_with = "id", help = "Target every reminder")]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct SystemStatusArgs {
    #[arg(long, help = "Show paths and notification diagnostics")]
    pub verbose: bool,
}

#[derive(Debug, Subcommand)]
pub enum SystemCommand {
    /// Show system setup.
    Status(SystemStatusArgs),
    /// Install and start the background service.
    Install,
    /// Uninstall pester.
    Uninstall(UninstallArgs),
    /// Run the daemon in the foreground.
    Daemon,
}

#[derive(Debug, Args)]
pub struct UninstallArgs {
    #[arg(long, help = "Skip the uninstall confirmation prompt")]
    pub yes: bool,
    #[arg(long, help = "Permanently delete reminders and state")]
    pub delete_data: bool,
}

#[derive(Debug, Subcommand)]
pub enum ConfirmCommand {
    /// Set a global or reminder-specific done confirmation phrase.
    #[command(override_usage = "pester confirm set [ID] [--phrase <PHRASE>]")]
    Set {
        #[arg(value_name = "ID")]
        id: Option<String>,
        #[arg(long, value_name = "PHRASE")]
        phrase: Option<String>,
    },
    /// Show a global or reminder-specific done confirmation phrase.
    Show {
        #[arg(value_name = "ID")]
        id: Option<String>,
    },
    /// Reset a global or reminder-specific done confirmation phrase.
    Reset {
        #[arg(value_name = "ID")]
        id: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{Cli, Command, ConfirmCommand, SystemCommand};

    fn subcommand_help(name: &str) -> String {
        let mut command = Cli::command();
        command
            .find_subcommand_mut(name)
            .expect("subcommand exists")
            .render_help()
            .to_string()
    }

    #[test]
    fn add_usage_places_id_before_options() {
        let help = subcommand_help("add");

        assert!(help.contains(
            "Usage: pester add <ID> --time <TIME> --title <TITLE> --message <MESSAGE> [OPTIONS]"
        ));
    }

    #[test]
    fn set_usage_places_id_before_options() {
        let help = subcommand_help("set");

        assert!(help.contains("Usage: pester set <ID> [OPTIONS]"));
    }

    #[test]
    fn remove_usage_requires_id_or_all() {
        let help = subcommand_help("remove");

        assert!(help.contains("Usage: pester remove <ID> | --all"));
    }

    #[test]
    fn target_commands_require_id_or_all() {
        for command in [
            "remove", "show", "test", "done", "undone", "enable", "disable",
        ] {
            assert!(
                Cli::try_parse_from(["pester", command]).is_err(),
                "{command} should require an id or --all"
            );
            assert!(
                Cli::try_parse_from(["pester", command, "winddown", "--all"]).is_err(),
                "{command} should reject combining an id with --all"
            );
            assert!(
                Cli::try_parse_from(["pester", command, "winddown"]).is_ok(),
                "{command} should accept an id target"
            );
            assert!(
                Cli::try_parse_from(["pester", command, "--all"]).is_ok(),
                "{command} should accept --all"
            );
        }
    }

    #[test]
    fn old_system_commands_are_not_top_level_commands() {
        for command in ["list", "status", "doctor", "install", "uninstall", "daemon"] {
            assert!(
                Cli::try_parse_from(["pester", command]).is_err(),
                "{command} should not be a top-level command"
            );
        }
        assert!(Cli::try_parse_from(["pester", "confirm", "done", "set"]).is_err());
        assert!(Cli::try_parse_from(["pester", "system", "install"]).is_ok());
    }

    #[test]
    fn system_commands_parse_under_system_namespace() {
        assert!(Cli::try_parse_from(["pester", "system", "status"]).is_ok());
        assert!(Cli::try_parse_from(["pester", "system", "status", "--verbose"]).is_ok());
        assert!(Cli::try_parse_from(["pester", "system", "install"]).is_ok());
        assert!(Cli::try_parse_from(["pester", "system", "uninstall"]).is_ok());
        assert!(Cli::try_parse_from(["pester", "system", "uninstall", "--yes"]).is_ok());
        assert!(Cli::try_parse_from(["pester", "system", "uninstall", "--delete-data"]).is_ok());
        assert!(Cli::try_parse_from(["pester", "system", "daemon"]).is_ok());
    }

    #[test]
    fn system_uninstall_yes_flag_parses() {
        let cli = Cli::parse_from(["pester", "system", "uninstall", "--yes"]);

        let Command::System {
            command: SystemCommand::Uninstall(args),
        } = cli.command
        else {
            panic!("expected system uninstall command");
        };
        assert!(args.yes);
        assert!(!args.delete_data);
    }

    #[test]
    fn system_status_verbose_flag_parses() {
        let cli = Cli::parse_from(["pester", "system", "status", "--verbose"]);

        let Command::System {
            command: SystemCommand::Status(args),
        } = cli.command
        else {
            panic!("expected system status command");
        };
        assert!(args.verbose);
    }

    #[test]
    fn confirm_commands_parse_optional_id_and_phrase_flag() {
        assert!(Cli::try_parse_from(["pester", "confirm", "set"]).is_ok());
        assert!(Cli::try_parse_from(["pester", "confirm", "set", "meds"]).is_ok());
        assert!(
            Cli::try_parse_from(["pester", "confirm", "set", "--phrase", "global phrase"]).is_ok()
        );
        assert!(Cli::try_parse_from([
            "pester",
            "confirm",
            "set",
            "meds",
            "--phrase",
            "I took my medication"
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["pester", "confirm", "show"]).is_ok());
        assert!(Cli::try_parse_from(["pester", "confirm", "show", "meds"]).is_ok());
        assert!(Cli::try_parse_from(["pester", "confirm", "reset"]).is_ok());
        assert!(Cli::try_parse_from(["pester", "confirm", "reset", "meds"]).is_ok());
    }

    #[test]
    fn confirm_set_captures_reminder_id_and_phrase() {
        let cli = Cli::parse_from([
            "pester",
            "confirm",
            "set",
            "meds",
            "--phrase",
            "I took my medication",
        ]);
        let Command::Confirm {
            command: ConfirmCommand::Set { id, phrase },
        } = cli.command
        else {
            panic!("expected confirm set command");
        };
        assert_eq!(id.as_deref(), Some("meds"));
        assert_eq!(phrase.as_deref(), Some("I took my medication"));
    }
}
