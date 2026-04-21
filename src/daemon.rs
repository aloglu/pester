use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveTime};

use crate::models::{Config, Reminder, State};
use crate::notify;
use crate::store::Store;

pub fn run(store: Store) -> Result<()> {
    println!("Pester daemon started.");
    loop {
        if let Err(error) = tick(&store) {
            tracing::error!("{error:#}");
        }
        thread::sleep(duration_until_next_second(Local::now()));
    }
}

fn tick(store: &Store) -> Result<()> {
    let config = store.load_config()?;
    let mut state = store.load_state()?;
    let now = Local::now();
    let mut changed = false;

    for reminder_id in due_reminders(&config, &state, now)? {
        let reminder = config
            .reminder(&reminder_id)
            .with_context(|| format!("reminder \"{reminder_id}\" disappeared during tick"))?;
        notify::send(reminder)?;
        state
            .entry_mut(now.date_naive(), &reminder.id)
            .last_notified_at = Some(now.to_rfc3339());
        changed = true;
    }

    if changed {
        store.save_state(&state)?;
    }

    Ok(())
}

pub(crate) fn due_reminders(
    config: &Config,
    state: &State,
    now: DateTime<Local>,
) -> Result<Vec<String>> {
    let today = now.date_naive();
    let mut due = Vec::new();

    for reminder in config.reminders.iter().filter(|reminder| reminder.enabled) {
        if !is_due(reminder, now)? {
            continue;
        }

        let today_state = state.get(today, &reminder.id);
        if today_state.map(|entry| entry.done).unwrap_or(false) {
            continue;
        }

        let last_notified_at = today_state.and_then(|entry| entry.last_notified_at.as_deref());
        if should_notify(reminder, last_notified_at, now)? {
            due.push(reminder.id.clone());
        }
    }

    Ok(due)
}

fn is_due(reminder: &Reminder, now: DateTime<Local>) -> Result<bool> {
    let scheduled = NaiveTime::parse_from_str(&reminder.time, "%H:%M")
        .with_context(|| format!("invalid time for reminder {}", reminder.id))?;
    Ok(now.time() >= scheduled)
}

fn should_notify(
    reminder: &Reminder,
    last_notified_at: Option<&str>,
    now: DateTime<Local>,
) -> Result<bool> {
    let Some(last_notified_at) = last_notified_at else {
        return Ok(true);
    };

    let last = DateTime::parse_from_rfc3339(last_notified_at)
        .with_context(|| format!("invalid last notification timestamp for {}", reminder.id))?
        .with_timezone(&Local);
    let repeat = humantime::parse_duration(&reminder.repeat_every)
        .with_context(|| format!("invalid repeat interval for {}", reminder.id))?;

    Ok(now.signed_duration_since(last).to_std().unwrap_or_default() >= repeat)
}

fn duration_until_next_second(now: DateTime<Local>) -> Duration {
    let nanos = now.timestamp_subsec_nanos();
    if nanos == 0 {
        Duration::from_secs(1)
    } else {
        Duration::from_nanos(1_000_000_000 - u64::from(nanos))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeZone, Timelike};

    use super::{due_reminders, duration_until_next_second, is_due, should_notify};
    use crate::models::{Config, Reminder, State};

    fn reminder(time: &str, repeat_every: &str) -> Reminder {
        Reminder {
            id: "winddown".to_string(),
            title: "Wind down".to_string(),
            message: "No exciting stuff now.".to_string(),
            time: time.to_string(),
            repeat_every: repeat_every.to_string(),
            enabled: true,
        }
    }

    fn reminder_with_id(id: &str, time: &str, repeat_every: &str) -> Reminder {
        Reminder {
            id: id.to_string(),
            title: id.to_string(),
            message: "Test".to_string(),
            time: time.to_string(),
            repeat_every: repeat_every.to_string(),
            enabled: true,
        }
    }

    fn config(reminders: Vec<Reminder>) -> Config {
        Config {
            reminders,
            confirmation: Default::default(),
        }
    }

    #[test]
    fn due_when_local_time_has_reached_schedule() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 22, 0, 0).unwrap();
        assert!(is_due(&reminder("22:00", "5m"), now).unwrap());
        assert!(is_due(&reminder("21:59", "5m"), now).unwrap());
        assert!(!is_due(&reminder("22:01", "5m"), now).unwrap());
    }

    #[test]
    fn notify_when_never_notified() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 22, 0, 0).unwrap();
        assert!(should_notify(&reminder("22:00", "5m"), None, now).unwrap());
    }

    #[test]
    fn notify_after_repeat_interval_elapsed() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 22, 5, 0).unwrap();
        let last = Local
            .with_ymd_and_hms(2026, 4, 21, 22, 0, 0)
            .unwrap()
            .to_rfc3339();

        assert!(should_notify(&reminder("22:00", "5m"), Some(&last), now).unwrap());
    }

    #[test]
    fn does_not_notify_before_repeat_interval_elapsed() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 22, 4, 59).unwrap();
        let last = Local
            .with_ymd_and_hms(2026, 4, 21, 22, 0, 0)
            .unwrap()
            .to_rfc3339();

        assert!(!should_notify(&reminder("22:00", "5m"), Some(&last), now).unwrap());
    }

    #[test]
    fn due_reminders_skips_before_scheduled_time() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 21, 59, 0).unwrap();
        let config = config(vec![reminder_with_id("winddown", "22:00", "5m")]);
        let state = State::default();

        assert!(due_reminders(&config, &state, now).unwrap().is_empty());
    }

    #[test]
    fn due_reminders_includes_due_enabled_pending_reminder() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 22, 0, 0).unwrap();
        let config = config(vec![reminder_with_id("winddown", "22:00", "5m")]);
        let state = State::default();

        assert_eq!(
            due_reminders(&config, &state, now).unwrap(),
            vec!["winddown"]
        );
    }

    #[test]
    fn due_reminders_skips_disabled_reminder() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 22, 0, 0).unwrap();
        let mut disabled = reminder_with_id("winddown", "22:00", "5m");
        disabled.enabled = false;
        let config = config(vec![disabled]);
        let state = State::default();

        assert!(due_reminders(&config, &state, now).unwrap().is_empty());
    }

    #[test]
    fn due_reminders_skips_done_reminder_for_today() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 22, 0, 0).unwrap();
        let config = config(vec![reminder_with_id("winddown", "22:00", "5m")]);
        let mut state = State::default();
        state.mark_done(now.date_naive(), "winddown");

        assert!(due_reminders(&config, &state, now).unwrap().is_empty());
    }

    #[test]
    fn due_reminders_done_state_is_date_scoped() {
        let now = Local.with_ymd_and_hms(2026, 4, 22, 22, 0, 0).unwrap();
        let yesterday = Local.with_ymd_and_hms(2026, 4, 21, 22, 0, 0).unwrap();
        let config = config(vec![reminder_with_id("winddown", "22:00", "5m")]);
        let mut state = State::default();
        state.mark_done(yesterday.date_naive(), "winddown");

        assert_eq!(
            due_reminders(&config, &state, now).unwrap(),
            vec!["winddown"]
        );
    }

    #[test]
    fn due_reminders_respects_repeat_interval_per_reminder() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 22, 4, 59).unwrap();
        let config = config(vec![reminder_with_id("winddown", "22:00", "5m")]);
        let mut state = State::default();
        state
            .entry_mut(now.date_naive(), "winddown")
            .last_notified_at = Some(
            Local
                .with_ymd_and_hms(2026, 4, 21, 22, 0, 0)
                .unwrap()
                .to_rfc3339(),
        );

        assert!(due_reminders(&config, &state, now).unwrap().is_empty());
    }

    #[test]
    fn due_reminders_returns_multiple_due_reminders() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 22, 0, 0).unwrap();
        let config = config(vec![
            reminder_with_id("meds", "14:00", "5m"),
            reminder_with_id("winddown", "22:00", "5m"),
        ]);
        let state = State::default();

        assert_eq!(
            due_reminders(&config, &state, now).unwrap(),
            vec!["meds", "winddown"]
        );
    }

    #[test]
    fn daemon_sleep_aligns_to_next_second() {
        let now = Local
            .with_ymd_and_hms(2026, 4, 21, 22, 0, 0)
            .unwrap()
            .with_nanosecond(250_000_000)
            .unwrap();

        assert_eq!(
            duration_until_next_second(now),
            std::time::Duration::from_millis(750)
        );
    }

    #[test]
    fn daemon_sleep_waits_one_second_when_already_aligned() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 22, 0, 0).unwrap();

        assert_eq!(
            duration_until_next_second(now),
            std::time::Duration::from_secs(1)
        );
    }
}
