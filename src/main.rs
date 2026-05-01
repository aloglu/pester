use anyhow::{bail, Context, Result};
use chrono::{DateTime, Days, Local, NaiveDate, Timelike};
use clap::Parser;
use pester::cli::{Cli, Command, ConfirmCommand, SystemCommand, TargetArgs, TimerArgs};
use pester::confirm::{confirm_delete, confirm_done, confirm_yes_no, done_phrase, read_phrase};
use pester::models::{Reminder, State, Timer};
use pester::schedule::{parse_repeat_interval, parse_time, parse_window_duration};
use pester::store::Store;
use pester::{daemon, notify, service, term, update, version};

const DEFAULT_WINDOW_WARNING_NOTIFICATION_LIMIT: u64 = 12;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let command = Cli::parse().command;
    if let Command::Version = command {
        return show_version();
    }

    let store = Store::new()?;

    match command {
        Command::Add {
            id,
            time,
            every,
            title,
            message,
            until,
            active_for,
            max_notifications,
        } => add_reminder(
            &store,
            AddReminderInput {
                id,
                time,
                every,
                title,
                message,
                until,
                active_for,
                max_notifications,
            },
        ),
        Command::Set {
            id,
            time,
            every,
            title,
            message,
            until,
            active_for,
            max_notifications,
            clear_until,
            clear_active_for,
            clear_max_notifications,
        } => set_reminder(
            &store,
            SetReminderInput {
                id,
                time,
                every,
                title,
                message,
                until,
                active_for,
                max_notifications,
                clear_until,
                clear_active_for,
                clear_max_notifications,
            },
        ),
        Command::Done(target) => done(&store, target),
        Command::Undone(target) => undone(&store, target),
        Command::Remove(target) => remove(&store, target),
        Command::Show(target) => show(&store, target),
        Command::Enable(target) => set_enabled(&store, target, true),
        Command::Disable(target) => set_enabled(&store, target, false),
        Command::Test(target) => test(&store, target),
        Command::Timer(args) => timer(&store, args),
        Command::Confirm { command } => match command {
            ConfirmCommand::Set { id, phrase } => set_done_phrase(&store, id, phrase),
            ConfirmCommand::Show { id } => show_done_phrase(&store, id),
            ConfirmCommand::Reset { id } => reset_done_phrase(&store, id),
        },
        Command::System { command } => match command {
            SystemCommand::Status(args) => system_status(&store, args.verbose),
            SystemCommand::Install => service::install(&store.paths),
            SystemCommand::Uninstall(args) => uninstall(&store, args.delete_data, args.yes),
            SystemCommand::Daemon => daemon::run(store),
        },
        Command::Update => update::run(&store.paths),
        Command::Version => unreachable!(),
    }
}

struct AddReminderInput {
    id: String,
    time: String,
    every: String,
    title: String,
    message: String,
    until: Option<String>,
    active_for: Option<String>,
    max_notifications: Option<u32>,
}

fn add_reminder(store: &Store, input: AddReminderInput) -> Result<()> {
    let AddReminderInput {
        id,
        time,
        every,
        title,
        message,
        until,
        active_for,
        max_notifications,
    } = input;

    validate_id(&id)?;
    parse_time(&time)?;
    parse_repeat_interval(&every)?;
    validate_window_config(
        &time,
        &every,
        until.as_deref(),
        active_for.as_deref(),
        max_notifications,
    )?;

    let mut config = store.load_config()?;
    if config.reminder(&id).is_some() {
        bail!("reminder \"{id}\" already exists");
    }

    config.reminders.push(Reminder {
        id: id.clone(),
        title,
        message,
        time: time.clone(),
        repeat_every: every,
        starts_on: Some(initial_start_date(&time)?),
        until,
        active_for,
        max_notifications,
        done_phrase: None,
        enabled: true,
    });
    store.save_config(&config)?;
    term::ok(format!("Added reminder \"{id}\"."));
    let reminder = config.reminder(&id).expect("reminder was just added");
    warn_if_default_window_is_short(reminder)?;
    Ok(())
}

fn initial_start_date(time: &str) -> Result<NaiveDate> {
    let now = Local::now();
    let scheduled = parse_time(time)?;

    if now.time() <= scheduled {
        Ok(now.date_naive())
    } else {
        now.date_naive()
            .checked_add_days(Days::new(1))
            .context("could not calculate next reminder date")
    }
}

struct SetReminderInput {
    id: String,
    time: Option<String>,
    every: Option<String>,
    title: Option<String>,
    message: Option<String>,
    until: Option<String>,
    active_for: Option<String>,
    max_notifications: Option<u32>,
    clear_until: bool,
    clear_active_for: bool,
    clear_max_notifications: bool,
}

fn set_reminder(store: &Store, input: SetReminderInput) -> Result<()> {
    let SetReminderInput {
        id,
        time,
        every,
        title,
        message,
        until,
        active_for,
        max_notifications,
        clear_until,
        clear_active_for,
        clear_max_notifications,
    } = input;

    let mut config = store.load_config()?;
    let reminder = config
        .reminder_mut(&id)
        .with_context(|| format!("reminder \"{id}\" does not exist"))?;

    if until.is_some() && clear_until {
        bail!("cannot use --until and --clear-until together");
    }
    if active_for.is_some() && clear_active_for {
        bail!("cannot use --for and --clear-for together");
    }
    if max_notifications.is_some() && clear_max_notifications {
        bail!("cannot use --max and --clear-max together");
    }

    let mut changes = Vec::new();
    if let Some(time) = time {
        parse_time(&time)?;
        changes.push(format!("time: {} -> {}", reminder.time, time));
        reminder.time = time;
    }
    if let Some(every) = every {
        parse_repeat_interval(&every)?;
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
    if let Some(until) = until {
        parse_time(&until).context("--until must be in 24-hour HH:MM format")?;
        changes.push(format!(
            "until: {} -> {}",
            reminder.until.as_deref().unwrap_or("default"),
            until
        ));
        reminder.until = Some(until);
    }
    if clear_until {
        changes.push(format!(
            "until: {} -> default",
            reminder.until.as_deref().unwrap_or("default")
        ));
        reminder.until = None;
    }
    if let Some(active_for) = active_for {
        parse_window_duration(&active_for)?;
        changes.push(format!(
            "for: {} -> {}",
            reminder.active_for.as_deref().unwrap_or("default"),
            active_for
        ));
        reminder.active_for = Some(active_for);
    }
    if clear_active_for {
        changes.push(format!(
            "for: {} -> default",
            reminder.active_for.as_deref().unwrap_or("default")
        ));
        reminder.active_for = None;
    }
    if let Some(max_notifications) = max_notifications {
        validate_max_notifications(max_notifications)?;
        changes.push(format!(
            "max: {} -> {}",
            reminder
                .max_notifications
                .map(|value| value.to_string())
                .unwrap_or_else(|| "default".to_string()),
            max_notifications
        ));
        reminder.max_notifications = Some(max_notifications);
    }
    if clear_max_notifications {
        changes.push(format!(
            "max: {} -> default",
            reminder
                .max_notifications
                .map(|value| value.to_string())
                .unwrap_or_else(|| "default".to_string())
        ));
        reminder.max_notifications = None;
    }

    validate_window_config(
        &reminder.time,
        &reminder.repeat_every,
        reminder.until.as_deref(),
        reminder.active_for.as_deref(),
        reminder.max_notifications,
    )?;

    if changes.is_empty() {
        bail!("nothing to update");
    }

    let updated_reminder = reminder.clone();
    store.save_config(&config)?;
    term::ok(format!("Updated reminder \"{id}\"."));
    for change in changes {
        term::detail(change);
    }
    warn_if_default_window_is_short(&updated_reminder)?;
    Ok(())
}

fn done(store: &Store, target: TargetArgs) -> Result<()> {
    let config = store.load_config()?;
    if config.reminders.is_empty() {
        bail!("no reminders exist");
    }

    if target.all {
        confirm_done(
            &config,
            None,
            "Mark all reminders done for today? This will stop every reminder until tomorrow.",
        )?;
        let mut state = store.load_state()?;
        let now = Local::now();
        for reminder in &config.reminders {
            state.mark_done(daemon::state_date_for_now(reminder, now)?, &reminder.id);
        }
        store.save_state(&state)?;
        term::ok("Marked all reminders done for today.");
        return Ok(());
    }

    let target = target.id.expect("target id is required by clap");
    let reminder = config
        .reminder(&target)
        .with_context(|| format!("reminder \"{target}\" does not exist"))?;
    confirm_done(
        &config,
        Some(reminder),
        &format!("Mark \"{}\" done for today?", reminder.id),
    )?;

    let mut state = store.load_state()?;
    state.mark_done(
        daemon::state_date_for_now(reminder, Local::now())?,
        &reminder.id,
    );
    store.save_state(&state)?;
    term::ok(format!("Marked \"{}\" done for today.", reminder.id));
    Ok(())
}

fn undone(store: &Store, target: TargetArgs) -> Result<()> {
    let config = store.load_config()?;
    if config.reminders.is_empty() {
        bail!("no reminders exist");
    }

    let mut state = store.load_state()?;
    let now = Local::now();
    if target.all {
        if !confirm_yes_no(
            "Mark all reminders not done for today? This may restart reminder notifications.",
        )? {
            term::warn("Cancelled.");
            return Ok(());
        }

        for reminder in &config.reminders {
            state.mark_undone(daemon::state_date_for_now(reminder, now)?, &reminder.id);
        }
        store.save_state(&state)?;
        term::ok("Marked all reminders not done for today.");
        return Ok(());
    }

    let target = target.id.expect("target id is required by clap");
    let reminder = config
        .reminder(&target)
        .with_context(|| format!("reminder \"{target}\" does not exist"))?;
    state.mark_undone(daemon::state_date_for_now(reminder, now)?, &reminder.id);
    store.save_state(&state)?;
    term::ok(format!("Marked \"{}\" not done for today.", reminder.id));
    Ok(())
}

fn remove(store: &Store, target: TargetArgs) -> Result<()> {
    let mut config = store.load_config()?;
    if config.reminders.is_empty() {
        bail!("no reminders exist");
    }

    if target.all {
        confirm_delete("Remove all reminders? This cannot be undone.")?;
        let count = config.reminders.len();
        config.reminders.clear();
        store.save_config(&config)?;
        term::ok(format!("Removed {count} reminder(s)."));
        return Ok(());
    }

    let id = target.id.expect("target id is required by clap");
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

fn set_enabled(store: &Store, target: TargetArgs, enabled: bool) -> Result<()> {
    let mut config = store.load_config()?;
    if config.reminders.is_empty() {
        bail!("no reminders exist");
    }

    let action = if enabled { "enable" } else { "disable" };
    let past = if enabled { "Enabled" } else { "Disabled" };

    if target.all {
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

    let target = target.id.expect("target id is required by clap");
    let reminder = config
        .reminder_mut(&target)
        .with_context(|| format!("reminder \"{target}\" does not exist"))?;
    reminder.enabled = enabled;
    store.save_config(&config)?;
    term::ok(format!("{past} reminder \"{target}\"."));
    term::detail(format!(
        "Run `pester {action} --all` to {action} every reminder."
    ));
    Ok(())
}

fn show(store: &Store, target: TargetArgs) -> Result<()> {
    let config = store.load_config()?;
    let state = store.load_state()?;
    let now = Local::now();
    if config.reminders.is_empty() {
        term::heading("Reminders");
        term::warn("No reminders configured.");
        return Ok(());
    }

    if target.all {
        term::heading("Reminders");
        for reminder in &config.reminders {
            print_reminder(reminder, &state, now)?;
        }
        return Ok(());
    }

    let id = target.id.expect("target id is required by clap");
    let reminder = config
        .reminder(&id)
        .with_context(|| format!("reminder \"{id}\" does not exist"))?;
    term::heading("Reminder");
    print_reminder(reminder, &state, now)
}

fn print_reminder(reminder: &Reminder, state: &State, now: chrono::DateTime<Local>) -> Result<()> {
    let state_date = daemon::state_date_for_now(reminder, now)?;
    let today_state = state.get(state_date, &reminder.id);
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
    term::key_value("title", &reminder.title);
    term::key_value("time", &reminder.time);
    term::key_value("repeat", &reminder.repeat_every);
    term::key_value("window", reminder_window_description(reminder));
    if let Some(max_notifications) = reminder.max_notifications {
        let count = today_state
            .map(|entry| entry.notification_count)
            .unwrap_or_default();
        term::key_value("notifications", format!("{count}/{max_notifications}"));
    }
    Ok(())
}

fn test(store: &Store, target: TargetArgs) -> Result<()> {
    let config = store.load_config()?;
    if config.reminders.is_empty() {
        bail!("no reminders exist");
    }

    if target.all {
        if config.reminders.len() > 1
            && !confirm_yes_no(&format!(
                "Send test notifications for {} reminders?",
                config.reminders.len()
            ))?
        {
            term::warn("Cancelled.");
            return Ok(());
        }

        for reminder in &config.reminders {
            notify::send(reminder)?;
        }
        term::ok(format!(
            "Sent test notifications for {} reminder(s).",
            config.reminders.len()
        ));
        return Ok(());
    }

    let id = target.id.expect("target id is required by clap");
    let reminder = config
        .reminder(&id)
        .with_context(|| format!("reminder \"{id}\" does not exist"))?;
    notify::send(reminder)?;
    term::ok(format!("Sent test notification for \"{}\".", reminder.id));
    Ok(())
}

fn timer(store: &Store, args: TimerArgs) -> Result<()> {
    match args.args.as_slice() {
        [command] if command == "list" => {
            reject_timer_creation_flags(&args)?;
            list_timers(store)
        }
        [command, id] if command == "stop" => {
            reject_timer_creation_flags(&args)?;
            stop_timer(store, id)
        }
        [id, duration] => start_timer(store, id, duration, args.title, args.message),
        _ => bail!(
            "usage: pester timer <ID> <DURATION> [--title <TITLE>] [--message <MESSAGE>] | list | stop <ID>"
        ),
    }
}

fn reject_timer_creation_flags(args: &TimerArgs) -> Result<()> {
    if args.title.is_some() || args.message.is_some() {
        bail!("--title and --message are only valid when creating a timer");
    }
    Ok(())
}

fn start_timer(
    store: &Store,
    id: &str,
    duration: &str,
    title: Option<String>,
    message: Option<String>,
) -> Result<()> {
    validate_named_id("timer", id)?;
    let duration_value = parse_window_duration(duration)?;
    let now = Local::now();
    let ends_at = now
        .checked_add_signed(
            chrono::Duration::from_std(duration_value).context("timer duration is too large")?,
        )
        .context("timer end time is out of range")?;

    let mut state = store.load_state()?;
    if state.timers.contains_key(id) {
        bail!("timer \"{id}\" already exists");
    }

    let title = title.unwrap_or_else(|| id.to_string());
    let message = message.unwrap_or_else(|| "Timer finished.".to_string());
    state.timers.insert(
        id.to_string(),
        Timer {
            id: id.to_string(),
            title: title.clone(),
            message,
            duration: duration.to_string(),
            started_at: now.to_rfc3339(),
            ends_at: ends_at.to_rfc3339(),
            expired_at: None,
        },
    );
    store.save_state(&state)?;

    term::ok(format!(
        "Started timer \"{id}\" for {}.",
        humantime::format_duration(duration_value)
    ));
    term::detail(format!(
        "Ends at {}.",
        ends_at.format("%Y-%m-%d %H:%M:%S %Z")
    ));
    if title != id {
        term::detail(format!("Title: {title}"));
    }
    Ok(())
}

fn list_timers(store: &Store) -> Result<()> {
    let state = store.load_state()?;
    term::heading("Timers");
    if state.timers.is_empty() {
        term::warn("No timers running.");
        return Ok(());
    }

    let now = Local::now();
    for timer in state.timers.values() {
        println!();
        println!("{}", term::bold(&timer.id));
        let status = if timer.is_expired() {
            term::yellow("expired")
        } else {
            term::green("running")
        };
        term::key_value("status", status);
        term::key_value("title", &timer.title);
        term::key_value("duration", &timer.duration);
        term::key_value("ends", timer.ends_at_display()?);
        term::key_value("remaining", timer.remaining_display(now)?);
    }
    Ok(())
}

fn stop_timer(store: &Store, id: &str) -> Result<()> {
    let mut state = store.load_state()?;
    if state.timers.remove(id).is_none() {
        bail!("timer \"{id}\" does not exist");
    }
    store.save_state(&state)?;
    term::ok(format!("Stopped timer \"{id}\"."));
    Ok(())
}

trait TimerDisplayExt {
    fn ends_at_display(&self) -> Result<String>;
    fn remaining_display(&self, now: DateTime<Local>) -> Result<String>;
}

impl TimerDisplayExt for Timer {
    fn ends_at_display(&self) -> Result<String> {
        Ok(parse_rfc3339_local(&self.ends_at)?
            .format("%Y-%m-%d %H:%M:%S %Z")
            .to_string())
    }

    fn remaining_display(&self, now: DateTime<Local>) -> Result<String> {
        if self.is_expired() {
            return Ok("expired".to_string());
        }

        let ends_at = parse_rfc3339_local(&self.ends_at)?;
        let remaining = ends_at
            .signed_duration_since(now)
            .to_std()
            .unwrap_or_default();
        let rounded = std::time::Duration::from_secs(remaining.as_secs());
        Ok(humantime::format_duration(rounded).to_string())
    }
}

fn parse_rfc3339_local(value: &str) -> Result<DateTime<Local>> {
    Ok(DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("invalid timestamp: {value}"))?
        .with_timezone(&Local))
}

fn set_done_phrase(store: &Store, id: Option<String>, phrase: Option<String>) -> Result<()> {
    let mut config = store.load_config()?;
    if let Some(id) = id.as_deref() {
        config
            .reminder(id)
            .with_context(|| format!("reminder \"{id}\" does not exist"))?;
    }

    let prompt = match id.as_deref() {
        Some(id) => format!("Enter the phrase required to mark \"{id}\" done:"),
        None => "Enter the global phrase required to mark reminders done:".to_string(),
    };
    let phrase = match phrase {
        Some(phrase) => phrase,
        None => read_phrase(&prompt)?,
    };
    let phrase = phrase.trim().to_string();
    if phrase.len() < 3 {
        bail!("confirmation phrase must be at least 3 characters");
    }

    if let Some(id) = id {
        let reminder = config
            .reminder_mut(&id)
            .expect("reminder existence was already checked");
        reminder.done_phrase = Some(phrase);
        store.save_config(&config)?;
        term::ok(format!("Updated done confirmation phrase for \"{id}\"."));
        return Ok(());
    }

    config.confirmation.done_phrase = Some(phrase);
    store.save_config(&config)?;
    term::ok("Updated global done confirmation phrase.");
    Ok(())
}

fn show_done_phrase(store: &Store, id: Option<String>) -> Result<()> {
    let config = store.load_config()?;
    if let Some(id) = id {
        let reminder = config
            .reminder(&id)
            .with_context(|| format!("reminder \"{id}\" does not exist"))?;

        term::heading("Done Confirmation");
        term::key_value("reminder", &reminder.id);
        term::key_value(
            "override phrase",
            reminder.done_phrase.as_deref().unwrap_or("none"),
        );
        term::key_value(
            "effective phrase",
            done_phrase(&config, Some(reminder)).unwrap_or("yes"),
        );
        return Ok(());
    }

    term::heading("Done Confirmation");
    match config.confirmation.done_phrase.as_deref() {
        Some(phrase) => term::key_value("global phrase", phrase),
        None => term::key_value("global phrase", "yes"),
    }
    Ok(())
}

fn reset_done_phrase(store: &Store, id: Option<String>) -> Result<()> {
    let mut config = store.load_config()?;

    if let Some(id) = id {
        config
            .reminder(&id)
            .with_context(|| format!("reminder \"{id}\" does not exist"))?;
        if !confirm_yes_no(&format!(
            "Reset done confirmation phrase for \"{id}\"? It will fall back to the global phrase."
        ))? {
            term::warn("Cancelled.");
            return Ok(());
        }

        let reminder = config
            .reminder_mut(&id)
            .expect("reminder existence was already checked");
        reminder.done_phrase = None;
        store.save_config(&config)?;
        term::ok(format!("Reset done confirmation phrase for \"{id}\"."));
        return Ok(());
    }

    if !confirm_yes_no("Reset global done confirmation phrase to yes?")? {
        term::warn("Cancelled.");
        return Ok(());
    }

    config.confirmation.done_phrase = None;
    store.save_config(&config)?;
    term::ok("Reset global done confirmation phrase to yes.");
    Ok(())
}

fn uninstall(store: &Store, delete_data: bool, yes: bool) -> Result<()> {
    if delete_data && !yes {
        confirm_delete("Uninstall pester and permanently delete all reminders and state?")?;
    } else if !yes
        && !confirm_yes_no(
            "Uninstall pester and stop background reminders? Your reminders will be kept.",
        )?
    {
        term::warn("Cancelled.");
        return Ok(());
    }

    service::uninstall(&store.paths)?;
    if delete_data {
        store.delete_data()?;
    }
    let removed_binaries = store.delete_installed_binaries()?;
    if removed_binaries.is_empty() {
        term::warn("No installed binary was removed from the current development path.");
    } else {
        for path in removed_binaries {
            term::ok(format!("Removed binary at {}.", path.display()));
        }
    }
    term::ok("Uninstalled pester.");
    Ok(())
}

fn system_status(store: &Store, verbose: bool) -> Result<()> {
    term::heading("pester system");
    for line in service::diagnostics(&store.paths) {
        term::detail(line);
    }
    if verbose {
        println!();
        term::heading("Paths");
        term::key_value("config", store.paths.config_file.display());
        term::key_value("state", store.paths.state_file.display());
        term::key_value("binary", std::env::current_exe()?.display());
        println!();
        term::heading("Notifications");
        for line in notify::diagnostics() {
            term::detail(line);
        }
    }
    Ok(())
}

fn show_version() -> Result<()> {
    println!("pester {}", version::CURRENT_VERSION);
    if let Ok(status) = version::check_for_update() {
        if status.is_update_available() {
            term::warn(format!("Update available: {}", status.latest_version));
            term::detail("Run: pester update");
        }
    }
    Ok(())
}

pub(crate) fn validate_id(id: &str) -> Result<()> {
    validate_named_id("reminder", id)
}

pub(crate) fn validate_named_id(kind: &str, id: &str) -> Result<()> {
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
        "show",
        "status",
        "system",
        "test",
        "timer",
        "undone",
        "uninstall",
        "update",
        "version",
    ];

    if RESERVED.contains(&id) {
        bail!("\"{id}\" is reserved and cannot be used as a {kind} id");
    }
    if id.trim() != id || id.is_empty() {
        bail!("{kind} id cannot be empty or contain leading/trailing whitespace");
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("{kind} id may only contain letters, numbers, hyphens, and underscores");
    }
    Ok(())
}

fn validate_max_notifications(max_notifications: u32) -> Result<()> {
    if max_notifications == 0 {
        bail!("--max must be greater than zero");
    }
    Ok(())
}

fn validate_window_config(
    time: &str,
    every: &str,
    until: Option<&str>,
    active_for: Option<&str>,
    max_notifications: Option<u32>,
) -> Result<()> {
    parse_time(time)?;
    parse_repeat_interval(every)?;
    if until.is_some() && active_for.is_some() {
        bail!("use --until or --for, not both");
    }
    if let Some(until) = until {
        parse_time(until).context("--until must be in 24-hour HH:MM format")?;
    }
    if let Some(active_for) = active_for {
        parse_window_duration(active_for)?;
    }
    if let Some(max_notifications) = max_notifications {
        validate_max_notifications(max_notifications)?;
    }
    Ok(())
}

fn reminder_window_description(reminder: &Reminder) -> String {
    let mut parts = Vec::new();
    if let Some(until) = &reminder.until {
        parts.push(format!("until {until}"));
    } else if let Some(active_for) = &reminder.active_for {
        parts.push(format!("for {active_for}"));
    } else {
        parts.push("until midnight".to_string());
    }
    if let Some(max_notifications) = reminder.max_notifications {
        parts.push(format!("max {max_notifications} notifications"));
    }
    parts.join(", ")
}

fn warn_if_default_window_is_short(reminder: &Reminder) -> Result<()> {
    if reminder.until.is_some()
        || reminder.active_for.is_some()
        || reminder.max_notifications.is_some()
    {
        return Ok(());
    }
    let opportunities = default_notification_opportunities(&reminder.time, &reminder.repeat_every)?;
    if default_window_needs_warning(&reminder.time, &reminder.repeat_every)? {
        term::warn(format!(
            "Default behavior will notify at most {opportunities} time(s) before midnight."
        ));
        term::detail("Use --until HH:MM, --for 2h, or --max N to make the window explicit.");
        term::detail(format!("Example: pester set {} --until 03:00", reminder.id));
    }
    Ok(())
}

fn default_notification_opportunities(time: &str, every: &str) -> Result<u64> {
    let scheduled = parse_time(time)?;
    let repeat = parse_repeat_interval(every)?;
    let seconds_since_midnight = u64::from(scheduled.num_seconds_from_midnight());
    let seconds_until_midnight = 24 * 60 * 60 - seconds_since_midnight;
    if seconds_until_midnight == 0 {
        return Ok(0);
    }
    Ok(((seconds_until_midnight - 1) / repeat.as_secs()) + 1)
}

fn default_window_needs_warning(time: &str, every: &str) -> Result<bool> {
    Ok(default_notification_opportunities(time, every)?
        <= DEFAULT_WINDOW_WARNING_NOTIFICATION_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::{
        default_notification_opportunities, default_window_needs_warning, parse_repeat_interval,
        parse_time, parse_window_duration, validate_id, validate_window_config,
    };

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

    #[test]
    fn validates_repeat_intervals() {
        assert_eq!(
            parse_repeat_interval("5m").unwrap(),
            std::time::Duration::from_secs(300)
        );
        assert!(parse_repeat_interval("0s").is_err());
        assert!(parse_repeat_interval("soon").is_err());
    }

    #[test]
    fn validates_window_durations() {
        assert_eq!(
            parse_window_duration("3h10m").unwrap(),
            std::time::Duration::from_secs(11_400)
        );
        assert!(parse_window_duration("0s").is_err());
        assert!(parse_window_duration("24h").is_err());
        assert!(parse_window_duration("soon").is_err());
    }

    #[test]
    fn rejects_conflicting_window_limits() {
        assert!(validate_window_config("23:50", "5m", Some("03:00"), None, Some(3)).is_ok());
        assert!(validate_window_config("23:50", "5m", None, Some("3h"), Some(3)).is_ok());
        assert!(validate_window_config("23:50", "5m", Some("03:00"), Some("3h"), None).is_err());
        assert!(validate_window_config("23:50", "5m", None, None, Some(0)).is_err());
    }

    #[test]
    fn counts_default_notification_opportunities_before_midnight() {
        assert_eq!(
            default_notification_opportunities("23:00", "5m").unwrap(),
            12
        );
        assert_eq!(
            default_notification_opportunities("23:50", "5m").unwrap(),
            2
        );
        assert_eq!(
            default_notification_opportunities("23:55", "5m").unwrap(),
            1
        );
        assert_eq!(
            default_notification_opportunities("00:00", "5m").unwrap(),
            288
        );
    }

    #[test]
    fn warns_when_default_window_allows_twelve_or_fewer_notifications() {
        assert!(default_window_needs_warning("23:00", "5m").unwrap());
        assert!(default_window_needs_warning("23:50", "5m").unwrap());
        assert!(!default_window_needs_warning("22:55", "5m").unwrap());
    }
}
