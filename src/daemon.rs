use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Days, Local, NaiveDate, NaiveDateTime, NaiveTime};

use crate::models::{Config, Reminder, State};
use crate::notify;
use crate::schedule::{parse_repeat_interval, parse_window_duration};
use crate::store::Store;

pub fn run(store: Store) -> Result<()> {
    run_with_shutdown(store, |duration| {
        thread::sleep(duration);
        false
    })
}

pub fn run_with_shutdown<F>(store: Store, mut wait_for_shutdown: F) -> Result<()>
where
    F: FnMut(Duration) -> bool,
{
    tracing::info!("pester daemon started.");
    loop {
        if let Err(error) = tick(&store) {
            tracing::error!("{error:#}");
        }

        if wait_for_shutdown(duration_until_next_second(Local::now())) {
            tracing::info!("pester daemon stopped.");
            return Ok(());
        }
    }
}

fn tick(store: &Store) -> Result<()> {
    let config = store.load_config()?;
    let mut state = store.load_state()?;
    let now = Local::now();
    deliver_due_reminders(&config, &mut state, now, notify::send, |state| {
        store.save_state(state)
    })
}

fn deliver_due_reminders<F, S>(
    config: &Config,
    state: &mut State,
    now: DateTime<Local>,
    mut notify: F,
    mut save_state: S,
) -> Result<()>
where
    F: FnMut(&Reminder) -> Result<()>,
    S: FnMut(&State) -> Result<()>,
{
    for reminder_id in due_reminders(config, state, now)? {
        let reminder = config
            .reminder(&reminder_id)
            .with_context(|| format!("reminder \"{reminder_id}\" disappeared during tick"))?;
        notify(reminder)
            .with_context(|| format!("failed to send notification for {}", reminder.id))?;
        let window = active_window(reminder, now)?
            .with_context(|| format!("reminder \"{}\" is no longer active", reminder.id))?;
        state
            .entry_mut(window.state_date, &reminder.id)
            .record_notification(now.to_rfc3339());
        save_state(state)?;
    }

    Ok(())
}

pub(crate) fn due_reminders(
    config: &Config,
    state: &State,
    now: DateTime<Local>,
) -> Result<Vec<String>> {
    let mut due = Vec::new();

    for reminder in config.reminders.iter().filter(|reminder| reminder.enabled) {
        let Some(window) = active_window(reminder, now)? else {
            continue;
        };

        let today_state = state.get(window.state_date, &reminder.id);
        if today_state.map(|entry| entry.done).unwrap_or(false) {
            continue;
        }
        if let Some(max_notifications) = reminder.max_notifications {
            if today_state
                .map(|entry| entry.notification_count >= max_notifications)
                .unwrap_or(false)
            {
                continue;
            }
        }

        let last_notified_at = today_state.and_then(|entry| entry.last_notified_at.as_deref());
        if should_notify(reminder, last_notified_at, now)? {
            due.push(reminder.id.clone());
        }
    }

    Ok(due)
}

pub fn state_date_for_now(reminder: &Reminder, now: DateTime<Local>) -> Result<NaiveDate> {
    Ok(active_window(reminder, now)?
        .map(|window| window.state_date)
        .unwrap_or_else(|| now.date_naive()))
}

#[cfg(test)]
fn is_due(reminder: &Reminder, now: DateTime<Local>) -> Result<bool> {
    let scheduled = NaiveTime::parse_from_str(&reminder.time, "%H:%M")
        .with_context(|| format!("invalid time for reminder {}", reminder.id))?;
    Ok(now.time() >= scheduled)
}

#[derive(Debug, Clone, Copy)]
struct ReminderWindow {
    state_date: NaiveDate,
    starts_at: NaiveDateTime,
    ends_at: NaiveDateTime,
}

fn active_window(reminder: &Reminder, now: DateTime<Local>) -> Result<Option<ReminderWindow>> {
    let today = now.date_naive();
    let yesterday = today
        .checked_sub_days(Days::new(1))
        .context("could not calculate yesterday")?;

    for date in [today, yesterday] {
        let window = reminder_window_for_date(reminder, date)?;
        if reminder
            .starts_on
            .map(|starts_on| window.state_date < starts_on)
            .unwrap_or(false)
        {
            continue;
        }
        let now = now.naive_local();
        if now >= window.starts_at && now < window.ends_at {
            return Ok(Some(window));
        }
    }

    Ok(None)
}

fn reminder_window_for_date(reminder: &Reminder, date: NaiveDate) -> Result<ReminderWindow> {
    let scheduled = NaiveTime::parse_from_str(&reminder.time, "%H:%M")
        .with_context(|| format!("invalid time for reminder {}", reminder.id))?;
    let starts_at = date.and_time(scheduled);
    let ends_at = if let Some(active_for) = &reminder.active_for {
        let duration = chrono::Duration::from_std(parse_window_duration(active_for)?)
            .context("window duration is too large")?;
        starts_at
            .checked_add_signed(duration)
            .context("window end is out of range")?
    } else if let Some(until) = &reminder.until {
        let until = NaiveTime::parse_from_str(until, "%H:%M")
            .with_context(|| format!("invalid until time for reminder {}", reminder.id))?;
        let end_date = if until > scheduled {
            date
        } else {
            date.checked_add_days(Days::new(1))
                .context("could not calculate next day")?
        };
        end_date.and_time(until)
    } else {
        date.checked_add_days(Days::new(1))
            .context("could not calculate next day")?
            .and_time(NaiveTime::MIN)
    };

    Ok(ReminderWindow {
        state_date: date,
        starts_at,
        ends_at,
    })
}

fn should_notify(
    reminder: &Reminder,
    last_notified_at: Option<&str>,
    now: DateTime<Local>,
) -> Result<bool> {
    let Some(last_notified_at) = last_notified_at else {
        parse_repeat_interval(&reminder.repeat_every)
            .with_context(|| format!("invalid repeat interval for {}", reminder.id))?;
        return Ok(true);
    };

    let repeat = parse_repeat_interval(&reminder.repeat_every)
        .with_context(|| format!("invalid repeat interval for {}", reminder.id))?;
    let last = DateTime::parse_from_rfc3339(last_notified_at)
        .with_context(|| format!("invalid last notification timestamp for {}", reminder.id))?
        .with_timezone(&Local);

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
    use anyhow::bail;
    use chrono::{Local, TimeZone, Timelike};

    use super::{
        deliver_due_reminders, due_reminders, duration_until_next_second, is_due, should_notify,
        state_date_for_now,
    };
    use crate::models::{Config, Reminder, State};

    fn reminder(time: &str, repeat_every: &str) -> Reminder {
        Reminder {
            id: "winddown".to_string(),
            title: "Wind down".to_string(),
            message: "No exciting stuff now.".to_string(),
            time: time.to_string(),
            repeat_every: repeat_every.to_string(),
            starts_on: None,
            until: None,
            active_for: None,
            max_notifications: None,
            done_phrase: None,
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
            starts_on: None,
            until: None,
            active_for: None,
            max_notifications: None,
            done_phrase: None,
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
    fn rejects_invalid_repeat_interval_before_first_notification() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 22, 0, 0).unwrap();

        assert!(should_notify(&reminder("22:00", "0s"), None, now).is_err());
        assert!(should_notify(&reminder("22:00", "soon"), None, now).is_err());
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
    fn default_window_stops_at_midnight() {
        let now = Local.with_ymd_and_hms(2026, 4, 22, 0, 0, 0).unwrap();
        let config = config(vec![reminder_with_id("winddown", "23:50", "5m")]);
        let state = State::default();

        assert!(due_reminders(&config, &state, now).unwrap().is_empty());
    }

    #[test]
    fn newly_added_reminder_does_not_notify_for_an_already_started_window() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 23, 49, 0).unwrap();
        let mut reminder = reminder_with_id("overnight", "01:00", "5m");
        reminder.starts_on = Some(
            Local
                .with_ymd_and_hms(2026, 4, 22, 0, 0, 0)
                .unwrap()
                .date_naive(),
        );
        let config = config(vec![reminder]);
        let state = State::default();

        assert!(due_reminders(&config, &state, now).unwrap().is_empty());
    }

    #[test]
    fn newly_added_cross_midnight_reminder_waits_for_its_first_future_occurrence() {
        let now = Local.with_ymd_and_hms(2026, 4, 22, 0, 30, 0).unwrap();
        let mut reminder = reminder_with_id("overnight", "23:50", "5m");
        reminder.until = Some("03:00".to_string());
        reminder.starts_on = Some(now.date_naive());
        let config = config(vec![reminder]);
        let state = State::default();

        assert!(due_reminders(&config, &state, now).unwrap().is_empty());
    }

    #[test]
    fn until_window_can_cross_midnight() {
        let now = Local.with_ymd_and_hms(2026, 4, 22, 0, 0, 0).unwrap();
        let mut reminder = reminder_with_id("winddown", "23:50", "5m");
        reminder.until = Some("03:00".to_string());
        let config = config(vec![reminder]);
        let state = State::default();

        assert_eq!(
            due_reminders(&config, &state, now).unwrap(),
            vec!["winddown"]
        );
        assert_eq!(
            state_date_for_now(&config.reminders[0], now).unwrap(),
            Local
                .with_ymd_and_hms(2026, 4, 21, 23, 50, 0)
                .unwrap()
                .date_naive()
        );
    }

    #[test]
    fn until_window_stops_at_until_time() {
        let now = Local.with_ymd_and_hms(2026, 4, 22, 3, 0, 0).unwrap();
        let mut reminder = reminder_with_id("winddown", "23:50", "5m");
        reminder.until = Some("03:00".to_string());
        let config = config(vec![reminder]);
        let state = State::default();

        assert!(due_reminders(&config, &state, now).unwrap().is_empty());
    }

    #[test]
    fn for_window_can_cross_midnight() {
        let now = Local.with_ymd_and_hms(2026, 4, 22, 1, 0, 0).unwrap();
        let mut reminder = reminder_with_id("winddown", "23:50", "5m");
        reminder.active_for = Some("3h10m".to_string());
        let config = config(vec![reminder]);
        let state = State::default();

        assert_eq!(
            due_reminders(&config, &state, now).unwrap(),
            vec!["winddown"]
        );
    }

    #[test]
    fn max_notifications_limits_active_window() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 22, 10, 0).unwrap();
        let mut reminder = reminder_with_id("winddown", "22:00", "5m");
        reminder.max_notifications = Some(2);
        let config = config(vec![reminder]);
        let mut state = State::default();
        state
            .entry_mut(now.date_naive(), "winddown")
            .notification_count = 2;

        assert!(due_reminders(&config, &state, now).unwrap().is_empty());
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
    fn saves_state_after_each_successful_notification_before_later_failure() {
        let now = Local.with_ymd_and_hms(2026, 4, 21, 22, 0, 0).unwrap();
        let config = config(vec![
            reminder_with_id("meds", "14:00", "5m"),
            reminder_with_id("winddown", "22:00", "5m"),
        ]);
        let mut state = State::default();
        let mut sent = Vec::new();
        let mut saved = Vec::new();

        let error = deliver_due_reminders(
            &config,
            &mut state,
            now,
            |reminder| {
                if reminder.id == "winddown" {
                    bail!("notification backend failed");
                }
                sent.push(reminder.id.clone());
                Ok(())
            },
            |state| {
                saved.push(state.clone());
                Ok(())
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("failed to send notification"));
        assert_eq!(sent, vec!["meds"]);
        assert_eq!(saved.len(), 1);
        assert!(saved[0]
            .get(now.date_naive(), "meds")
            .and_then(|entry| entry.last_notified_at.as_deref())
            .is_some());
        assert_eq!(
            saved[0]
                .get(now.date_naive(), "meds")
                .map(|entry| entry.notification_count),
            Some(1)
        );
        assert!(saved[0].get(now.date_naive(), "winddown").is_none());
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
