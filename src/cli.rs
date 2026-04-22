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
    Done { target: String },
    /// Remove a reminder.
    Remove { id: String },
    /// Enable a reminder.
    Enable { target: String },
    /// Disable a reminder.
    Disable { target: String },
    /// List configured reminders.
    List,
    /// Show reminder status.
    Status,
    /// Send a test notification.
    Test { id: String },
    /// Manage confirmation settings.
    Confirm {
        #[command(subcommand)]
        command: ConfirmCommand,
    },
    /// Install and start the background service.
    Install,
    /// Uninstall Pester.
    Uninstall(UninstallArgs),
    /// Run the daemon in the foreground.
    Daemon,
    /// Diagnose notification and service setup.
    Doctor,
}

#[derive(Debug, Args)]
pub struct UninstallArgs {
    #[arg(long)]
    pub delete_data: bool,
}

#[derive(Debug, Subcommand)]
pub enum ConfirmCommand {
    /// Manage confirmation for done commands.
    Done {
        #[command(subcommand)]
        command: ConfirmDoneCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfirmDoneCommand {
    /// Set the phrase required by done commands.
    Set { phrase: Option<String> },
    /// Show the current done confirmation phrase.
    Show,
    /// Reset done confirmation to yes.
    Reset,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

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
}
