mod app;
mod cli;
mod confirm;
mod daemon;
mod models;
mod notify;
mod paths;
mod service;
mod store;
mod term;

use anyhow::{bail, Context, Result};
use chrono::{Local, NaiveTime};
use clap::Parser;
use cli::{Cli, Command, ConfirmCommand, ConfirmDoneCommand};
use confirm::{confirm_delete, confirm_done, confirm_yes_no, read_phrase};
use models::Reminder;
use store::Store;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let store = Store::new()?;

    match cli.command {
        Command::Add {
            id,
            time,
            every,
            title,
            message,
        } => add_reminder(&store, id, time, every, title, message),
        Command::Set {
            id,
            time,
            every,
            title,
            message,
        } => set_reminder(&store, id, time, every, title, message),
        Command::Done { target } => done(&store, target),
        Command::Remove { id } => remove(&store, id),
        Command::Enable { target } => set_enabled(&store, target, true),
        Command::Disable { target } => set_enabled(&store, target, false),
        Command::List => list(&store),
        Command::Status => status(&store),
        Command::Test { id } => test(&store, id),
        Command::Confirm {
            command: ConfirmCommand::Done { command },
        } => match command {
            ConfirmDoneCommand::Set { phrase } => set_done_phrase(&store, phrase),
            ConfirmDoneCommand::Show => show_done_phrase(&store),
            ConfirmDoneCommand::Reset => reset_done_phrase(&store),
        },
        Command::Install => service::install(&store.paths),
        Command::Uninstall(args) => uninstall(&store, args.delete_data),
        Command::Daemon => daemon::run(store),
        Command::Doctor => doctor(&store),
    }
}

fn add_reminder(
    store: &Store,
    id: String,
    time: String,
    every: String,
    title: String,
    message: String,
) -> Result<()> {
    validate_id(&id)?;
    parse_time(&time)?;
    humantime::parse_duration(&every).context("repeat interval must look like 5m, 30m, or 1h")?;

    let mut config = store.load_config()?;
    if config.reminder(&id).is_some() {
        bail!("reminder \"{id}\" already exists");
    }

    config.reminders.push(Reminder {
        id: id.clone(),
        title,
        message,
        time,
        repeat_every: every,
        enabled: true,
    });
    store.save_config(&config)?;
    term::ok(format!("Added reminder \"{id}\"."));
    Ok(())
}

fn set_reminder(
    store: &Store,
    id: String,
    time: Option<String>,
    every: Option<String>,
    title: Option<String>,
    message: Option<String>,
) -> Result<()> {
    let mut config = store.load_config()?;
    let reminder = config
        .reminder_mut(&id)
        .with_context(|| format!("reminder \"{id}\" does not exist"))?;

    let mut changes = Vec::new();
    if let Some(time) = time {
        parse_time(&time)?;
        changes.push(format!("time: {} -> {}", reminder.time, time));
        reminder.time = time;
    }
    if let Some(every) = every {
        humantime::parse_duration(&every)
            .context("repeat interval must look like 5m, 30m, or 1h")?;
        changes.push(format!("repeat: {} -> {}", reminder.repeat_every, every));
        reminder.repeat_every = every;
    }
    if let Some(title) = title {
        changes.push(format!("title: {} -> {}", reminder.title, title));
        reminder.title = title;
    }
    if let Some(message) = message {
        changes.push("message updated".to_string());
        reminder.message = message;
    }

    if changes.is_empty() {
        bail!("nothing to update");
    }

    store.save_config(&config)?;
    term::ok(format!("Updated reminder \"{id}\"."));
    for change in changes {
        term::detail(change);
    }
    Ok(())
}

fn done(store: &Store, target: String) -> Result<()> {
    let config = store.load_config()?;
    if config.reminders.is_empty() {
        bail!("no reminders exist");
    }

    if target == "all" {
        confirm_done(
            &config,
            "Mark all reminders done for today? This will stop every reminder until tomorrow.",
        )?;
        let mut state = store.load_state()?;
        let today = Local::now().date_naive();
        for reminder in &config.reminders {
            state.mark_done(today, &reminder.id);
        }
        store.save_state(&state)?;
        term::ok("Marked all reminders done for today.");
        return Ok(());
    }

    let reminder = config
        .reminder(&target)
        .with_context(|| format!("reminder \"{target}\" does not exist"))?;
    confirm_done(
        &config,
        &format!("Mark \"{}\" done for today?", reminder.id),
    )?;

    let mut state = store.load_state()?;
    state.mark_done(Local::now().date_naive(), &reminder.id);
    store.save_state(&state)?;
    term::ok(format!("Marked \"{}\" done for today.", reminder.id));
    Ok(())
}

fn remove(store: &Store, id: String) -> Result<()> {
    let mut config = store.load_config()?;
    if config.reminder(&id).is_none() {
        bail!("reminder \"{id}\" does not exist");
    }

    if !confirm_yes_no(&format!("Remove reminder \"{id}\"?"))? {
        term::warn("Cancelled.");
        return Ok(());
    }

    config.reminders.retain(|reminder| reminder.id != id);
    store.save_config(&config)?;
    term::ok(format!("Removed reminder \"{id}\"."));
    Ok(())
}

fn set_enabled(store: &Store, target: String, enabled: bool) -> Result<()> {
    let mut config = store.load_config()?;
    if config.reminders.is_empty() {
        bail!("no reminders exist");
    }

    let action = if enabled { "enable" } else { "disable" };
    let past = if enabled { "Enabled" } else { "Disabled" };

    if target == "all" {
        if !enabled
            && !confirm_yes_no(
                "Disable all reminders? This will stop every reminder until re-enabled.",
            )?
        {
            term::warn("Cancelled.");
            return Ok(());
        }

        for reminder in &mut config.reminders {
            reminder.enabled = enabled;
        }
        store.save_config(&config)?;
        term::ok(format!("{past} all reminders."));
        return Ok(());
    }

    let reminder = config
        .reminder_mut(&target)
        .with_context(|| format!("reminder \"{target}\" does not exist"))?;
    reminder.enabled = enabled;
    store.save_config(&config)?;
    term::ok(format!("{past} reminder \"{target}\"."));
    term::detail(format!(
        "Run `pester {action} all` to {action} every reminder."
    ));
    Ok(())
}

fn list(store: &Store) -> Result<()> {
    let config = store.load_config()?;
    if config.reminders.is_empty() {
        term::heading("Reminders");
        term::warn("No reminders configured.");
        return Ok(());
    }

    term::heading("Reminders");
    for reminder in &config.reminders {
        let state = if reminder.enabled {
            term::green("enabled")
        } else {
            term::yellow("disabled")
        };
        println!();
        println!("{}", term::bold(&reminder.id));
        term::key_value("title", &reminder.title);
        term::key_value("time", &reminder.time);
        term::key_value("repeat", &reminder.repeat_every);
        term::key_value("state", state);
    }
    Ok(())
}

fn status(store: &Store) -> Result<()> {
    let config = store.load_config()?;
    let state = store.load_state()?;
    let today = Local::now().date_naive();

    term::heading("Pester Status");
    term::key_value("config", store.paths.config_file.display());
    term::key_value("state", store.paths.state_file.display());
    term::key_value("today", today);

    if config.reminders.is_empty() {
        println!();
        term::warn("No reminders configured.");
        return Ok(());
    }

    println!();
    term::heading("Reminders");
    for reminder in &config.reminders {
        let today_state = state.get(today, &reminder.id);
        let done = today_state.map(|entry| entry.done).unwrap_or(false);
        let status = if !reminder.enabled {
            "disabled"
        } else if done {
            "done"
        } else {
            "pending"
        };
        let status = match status {
            "disabled" => term::yellow(status),
            "done" => term::green(status),
            _ => term::blue(status),
        };
        println!();
        println!("{}", term::bold(&reminder.id));
        term::key_value("status", status);
        term::key_value("time", &reminder.time);
        term::key_value("repeat", &reminder.repeat_every);
    }
    Ok(())
}

fn test(store: &Store, id: String) -> Result<()> {
    let config = store.load_config()?;
    let reminder = config
        .reminder(&id)
        .with_context(|| format!("reminder \"{id}\" does not exist"))?;
    notify::send(reminder)?;
    term::ok(format!("Sent test notification for \"{}\".", reminder.id));
    Ok(())
}

fn set_done_phrase(store: &Store, phrase: Option<String>) -> Result<()> {
    let phrase = match phrase {
        Some(phrase) => phrase,
        None => read_phrase("Enter the phrase required to mark reminders done:")?,
    };
    let phrase = phrase.trim().to_string();
    if phrase.len() < 3 {
        bail!("confirmation phrase must be at least 3 characters");
    }

    let mut config = store.load_config()?;
    config.confirmation.done_phrase = Some(phrase);
    store.save_config(&config)?;
    term::ok("Updated done confirmation phrase.");
    Ok(())
}

fn show_done_phrase(store: &Store) -> Result<()> {
    let config = store.load_config()?;
    match config.confirmation.done_phrase {
        Some(phrase) => println!("{phrase}"),
        None => term::warn("No custom done confirmation phrase is set. The default is yes."),
    }
    Ok(())
}

fn reset_done_phrase(store: &Store) -> Result<()> {
    if !confirm_yes_no("Reset done confirmation phrase to yes?")? {
        term::warn("Cancelled.");
        return Ok(());
    }

    let mut config = store.load_config()?;
    config.confirmation.done_phrase = None;
    store.save_config(&config)?;
    term::ok("Reset done confirmation phrase to yes.");
    Ok(())
}

fn uninstall(store: &Store, delete_data: bool) -> Result<()> {
    if delete_data {
        confirm_delete(
            "This will uninstall Pester and permanently delete all reminders and state.",
        )?;
    } else if !confirm_yes_no(
        "Uninstall Pester and stop background reminders? Your reminders will be kept.",
    )? {
        term::warn("Cancelled.");
        return Ok(());
    }

    service::uninstall(&store.paths)?;
    if delete_data {
        store.delete_data()?;
    }
    match store.delete_installed_binary()? {
        Some(path) => term::ok(format!("Removed binary at {}.", path.display())),
        None => term::warn("No installed binary was removed from the current development path."),
    }
    term::ok("Uninstalled Pester.");
    Ok(())
}

fn doctor(store: &Store) -> Result<()> {
    term::heading("Pester Doctor");
    term::key_value("config", store.paths.config_file.display());
    term::key_value("state", store.paths.state_file.display());
    term::key_value("binary", std::env::current_exe()?.display());
    for line in notify::diagnostics() {
        term::detail(line);
    }
    for line in service::diagnostics(&store.paths) {
        term::detail(line);
    }
    Ok(())
}

pub(crate) fn validate_id(id: &str) -> Result<()> {
    const RESERVED: &[&str] = &[
        "all",
        "add",
        "confirm",
        "daemon",
        "disable",
        "doctor",
        "done",
        "enable",
        "help",
        "install",
        "list",
        "remove",
        "set",
        "status",
        "test",
        "uninstall",
    ];

    if RESERVED.contains(&id) {
        bail!("\"{id}\" is reserved and cannot be used as a reminder id");
    }
    if id.trim() != id || id.is_empty() {
        bail!("reminder id cannot be empty or contain leading/trailing whitespace");
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("reminder id may only contain letters, numbers, hyphens, and underscores");
    }
    Ok(())
}

pub(crate) fn parse_time(time: &str) -> Result<NaiveTime> {
    if time.len() != 5 || time.as_bytes().get(2) != Some(&b':') {
        bail!("time must be in 24-hour HH:MM format");
    }

    NaiveTime::parse_from_str(time, "%H:%M").context("time must be in 24-hour HH:MM format")
}

#[cfg(test)]
mod tests {
    use super::{parse_time, validate_id};

    #[test]
    fn validates_reminder_ids() {
        assert!(validate_id("winddown").is_ok());
        assert!(validate_id("meds-afternoon").is_ok());
        assert!(validate_id("meds_afternoon").is_ok());
        assert!(validate_id("all").is_err());
        assert!(validate_id("done").is_err());
        assert!(validate_id("").is_err());
        assert!(validate_id(" winddown").is_err());
        assert!(validate_id("wind down").is_err());
        assert!(validate_id("winddown!").is_err());
    }

    #[test]
    fn parses_24_hour_times() {
        assert!(parse_time("00:00").is_ok());
        assert!(parse_time("22:00").is_ok());
        assert!(parse_time("23:59").is_ok());
        assert!(parse_time("24:00").is_err());
        assert!(parse_time("9:00").is_err());
        assert!(parse_time("09:0").is_err());
    }
}
