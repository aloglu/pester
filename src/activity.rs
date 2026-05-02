use anyhow::Result;
use chrono::{DateTime, Days, Local, NaiveTime};

use crate::daemon;
use crate::models::{Config, State};
use crate::schedule::parse_time;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    Hidden,
    Active,
    Alert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeActivity {
    pub tray_state: TrayState,
    pub tray_reminders: Vec<TrayReminder>,
    pub timers: Vec<ActiveTimer>,
}

impl RuntimeActivity {
    pub fn collect(config: &Config, state: &State, now: DateTime<Local>) -> Result<Self> {
        let mut tray_reminders = Vec::new();
        for reminder in config.reminders.iter().filter(|reminder| reminder.enabled) {
            let active_window = daemon::active_window(reminder, now)?;
            let state_date = daemon::state_date_for_now(reminder, now)?;
            let day_state = state.get(state_date, &reminder.id);
            let done = day_state.map(|entry| entry.done).unwrap_or(false);
            let (next_state, next_change_at) =
                reminder_tray_state(reminder, now, active_window, done)?;

            tray_reminders.push(TrayReminder {
                id: reminder.id.clone(),
                title: reminder.title.clone(),
                state: next_state,
                relevant_at: next_change_at,
                last_notified_at: day_state.and_then(|entry| entry.last_notified_at.clone()),
            });
        }

        let mut timers = state
            .timers
            .values()
            .map(|timer| {
                let ends_at = daemon::parse_timer_end(timer)?;
                Ok(ActiveTimer {
                    id: timer.id.clone(),
                    title: timer.title.clone(),
                    ends_at,
                    expired: timer.is_expired(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        timers.sort_by(|left, right| {
            left.ends_at
                .cmp(&right.ends_at)
                .then_with(|| left.id.cmp(&right.id))
        });

        let tray_state = if timers.iter().any(|timer| timer.expired) {
            TrayState::Alert
        } else if !timers.is_empty() || !tray_reminders.is_empty() {
            TrayState::Active
        } else {
            TrayState::Hidden
        };

        Ok(Self {
            tray_state,
            tray_reminders,
            timers,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayReminder {
    pub id: String,
    pub title: String,
    pub state: ReminderTrayState,
    pub relevant_at: DateTime<Local>,
    pub last_notified_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderTrayState {
    ActiveWindow,
    Scheduled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveTimer {
    pub id: String,
    pub title: String,
    pub ends_at: DateTime<Local>,
    pub expired: bool,
}

fn reminder_tray_state(
    reminder: &crate::models::Reminder,
    now: DateTime<Local>,
    active_window: Option<crate::daemon::ReminderWindow>,
    done: bool,
) -> Result<(ReminderTrayState, DateTime<Local>)> {
    if let Some(window) = active_window {
        if !done {
            return Ok((
                ReminderTrayState::ActiveWindow,
                window.ends_at.and_local_timezone(Local).single().unwrap(),
            ));
        }
    }

    Ok((
        ReminderTrayState::Scheduled,
        next_reminder_start(reminder, now)?,
    ))
}

fn next_reminder_start(
    reminder: &crate::models::Reminder,
    now: DateTime<Local>,
) -> Result<DateTime<Local>> {
    let scheduled = parse_time(&reminder.time)?;
    let today = now.date_naive();
    let mut date = reminder.starts_on.unwrap_or(today).max(today);

    if date == today && now.time() >= scheduled {
        date = date
            .checked_add_days(Days::new(1))
            .expect("next reminder day should be representable");
    }

    local_datetime(date, scheduled)
}

fn local_datetime(date: chrono::NaiveDate, time: NaiveTime) -> Result<DateTime<Local>> {
    date.and_time(time)
        .and_local_timezone(Local)
        .single()
        .ok_or_else(|| anyhow::anyhow!("could not resolve local reminder time"))
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Local, NaiveDate, TimeZone};

    use super::{ReminderTrayState, RuntimeActivity, TrayState};
    use crate::models::{Config, Reminder, ReminderDayState, State, Timer};

    fn reminder(id: &str, time: &str) -> Reminder {
        Reminder {
            id: id.to_string(),
            title: id.to_string(),
            message: format!("{id} message"),
            time: time.to_string(),
            repeat_every: "5m".to_string(),
            starts_on: Some(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()),
            until: Some("23:59".to_string()),
            active_for: None,
            max_notifications: None,
            done_phrase: None,
            enabled: true,
        }
    }

    #[test]
    fn hides_tray_when_nothing_is_active() {
        let activity =
            RuntimeActivity::collect(&Config::default(), &State::default(), Local::now()).unwrap();

        assert_eq!(activity.tray_state, TrayState::Hidden);
        assert!(activity.tray_reminders.is_empty());
        assert!(activity.timers.is_empty());
    }

    #[test]
    fn surfaces_active_reminders() {
        let now = Local
            .with_ymd_and_hms(2026, 5, 1, 22, 30, 0)
            .single()
            .unwrap();
        let config = Config {
            reminders: vec![reminder("winddown", "22:00")],
            confirmation: Default::default(),
        };

        let activity = RuntimeActivity::collect(&config, &State::default(), now).unwrap();

        assert_eq!(activity.tray_state, TrayState::Active);
        assert_eq!(activity.tray_reminders.len(), 1);
        assert_eq!(activity.tray_reminders[0].id, "winddown");
        assert_eq!(
            activity.tray_reminders[0].state,
            ReminderTrayState::ActiveWindow
        );
    }

    #[test]
    fn done_reminders_still_surface_future_schedule() {
        let now = Local
            .with_ymd_and_hms(2026, 5, 1, 22, 30, 0)
            .single()
            .unwrap();
        let config = Config {
            reminders: vec![reminder("winddown", "22:00")],
            confirmation: Default::default(),
        };
        let mut state = State::default();
        state.days.insert(
            "2026-05-01".to_string(),
            [(
                "winddown".to_string(),
                ReminderDayState {
                    done: true,
                    last_notified_at: None,
                    notification_count: 0,
                },
            )]
            .into_iter()
            .collect(),
        );

        let activity = RuntimeActivity::collect(&config, &state, now).unwrap();

        assert_eq!(activity.tray_state, TrayState::Active);
        assert_eq!(activity.tray_reminders.len(), 1);
        assert_eq!(
            activity.tray_reminders[0].state,
            ReminderTrayState::Scheduled
        );
    }

    #[test]
    fn expired_timers_raise_alert_state() {
        let now = Local
            .with_ymd_and_hms(2026, 5, 1, 22, 30, 0)
            .single()
            .unwrap();
        let mut state = State::default();
        state.timers.insert(
            "tea".to_string(),
            Timer {
                id: "tea".to_string(),
                title: "Tea".to_string(),
                message: "Timer finished.".to_string(),
                duration: "10m".to_string(),
                started_at: (now - Duration::minutes(10)).to_rfc3339(),
                ends_at: now.to_rfc3339(),
                expired_at: Some(now.to_rfc3339()),
            },
        );

        let activity = RuntimeActivity::collect(&Config::default(), &state, now).unwrap();

        assert_eq!(activity.tray_state, TrayState::Alert);
        assert_eq!(activity.timers.len(), 1);
        assert!(activity.timers[0].expired);
    }
}
