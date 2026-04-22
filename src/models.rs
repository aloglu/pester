use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub reminders: Vec<Reminder>,
    #[serde(default)]
    pub confirmation: Confirmation,
}

impl Config {
    pub fn reminder(&self, id: &str) -> Option<&Reminder> {
        self.reminders.iter().find(|reminder| reminder.id == id)
    }

    pub fn reminder_mut(&mut self, id: &str) -> Option<&mut Reminder> {
        self.reminders.iter_mut().find(|reminder| reminder.id == id)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Reminder {
    pub id: String,
    pub title: String,
    pub message: String,
    pub time: String,
    pub repeat_every: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    #[serde(default, rename = "for", skip_serializing_if = "Option::is_none")]
    pub active_for: Option<String>,
    #[serde(default, rename = "max", skip_serializing_if = "Option::is_none")]
    pub max_notifications: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_phrase: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Confirmation {
    #[serde(default)]
    pub done_phrase: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct State {
    #[serde(default)]
    pub days: BTreeMap<String, BTreeMap<String, ReminderDayState>>,
}

impl State {
    pub fn get(&self, date: NaiveDate, reminder_id: &str) -> Option<&ReminderDayState> {
        self.days
            .get(&date.to_string())
            .and_then(|day| day.get(reminder_id))
    }

    pub fn entry_mut(&mut self, date: NaiveDate, reminder_id: &str) -> &mut ReminderDayState {
        self.days
            .entry(date.to_string())
            .or_default()
            .entry(reminder_id.to_string())
            .or_default()
    }

    pub fn mark_done(&mut self, date: NaiveDate, reminder_id: &str) {
        self.entry_mut(date, reminder_id).done = true;
    }

    pub fn mark_undone(&mut self, date: NaiveDate, reminder_id: &str) {
        self.entry_mut(date, reminder_id).done = false;
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ReminderDayState {
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub last_notified_at: Option<String>,
    #[serde(default)]
    pub notification_count: u32,
}

impl ReminderDayState {
    pub fn record_notification(&mut self, timestamp: String) {
        self.last_notified_at = Some(timestamp);
        self.notification_count = self.notification_count.saturating_add(1);
    }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::{Config, Reminder, State};

    #[test]
    fn marks_individual_reminder_done_for_date() {
        let mut state = State::default();
        let date = NaiveDate::from_ymd_opt(2026, 4, 21).unwrap();

        state.mark_done(date, "winddown");

        assert!(state.get(date, "winddown").unwrap().done);
        assert!(state.get(date, "meds").is_none());
    }

    #[test]
    fn marks_individual_reminder_undone_for_date() {
        let mut state = State::default();
        let date = NaiveDate::from_ymd_opt(2026, 4, 21).unwrap();

        state.mark_done(date, "winddown");
        state.mark_undone(date, "winddown");

        assert!(!state.get(date, "winddown").unwrap().done);
    }

    #[test]
    fn serializes_config_as_toml() {
        let config = Config {
            reminders: vec![Reminder {
                id: "winddown".to_string(),
                title: "Wind down".to_string(),
                message: "No exciting stuff now.".to_string(),
                time: "22:00".to_string(),
                repeat_every: "5m".to_string(),
                until: None,
                active_for: None,
                max_notifications: None,
                done_phrase: None,
                enabled: true,
            }],
            confirmation: Default::default(),
        };

        let encoded = toml::to_string(&config).unwrap();
        let decoded: Config = toml::from_str(&encoded).unwrap();

        assert_eq!(decoded.reminders.len(), 1);
        assert_eq!(decoded.reminders[0].id, "winddown");
        assert_eq!(decoded.reminders[0].time, "22:00");
        assert!(decoded.reminders[0].until.is_none());
        assert!(decoded.reminders[0].active_for.is_none());
        assert!(decoded.reminders[0].max_notifications.is_none());
        assert!(decoded.reminders[0].done_phrase.is_none());
    }

    #[test]
    fn deserializes_missing_enabled_as_true() {
        let decoded: Config = toml::from_str(
            r#"
[[reminders]]
id = "winddown"
title = "Wind down"
message = "No exciting stuff now."
time = "22:00"
repeat_every = "5m"
"#,
        )
        .unwrap();

        assert!(decoded.reminders[0].enabled);
        assert!(decoded.reminders[0].until.is_none());
        assert!(decoded.reminders[0].active_for.is_none());
        assert!(decoded.reminders[0].max_notifications.is_none());
        assert!(decoded.reminders[0].done_phrase.is_none());
    }

    #[test]
    fn serializes_window_fields_with_cli_names() {
        let config = Config {
            reminders: vec![Reminder {
                id: "stretch".to_string(),
                title: "Stretch".to_string(),
                message: "Stand up.".to_string(),
                time: "14:00".to_string(),
                repeat_every: "10m".to_string(),
                until: None,
                active_for: Some("1h".to_string()),
                max_notifications: Some(3),
                done_phrase: None,
                enabled: true,
            }],
            confirmation: Default::default(),
        };

        let encoded = toml::to_string(&config).unwrap();

        assert!(encoded.contains("for = \"1h\""));
        assert!(encoded.contains("max = 3"));
    }

    #[test]
    fn serializes_reminder_done_phrase_when_present() {
        let config = Config {
            reminders: vec![Reminder {
                id: "meds".to_string(),
                title: "Medication".to_string(),
                message: "Take medication.".to_string(),
                time: "09:00".to_string(),
                repeat_every: "5m".to_string(),
                until: None,
                active_for: None,
                max_notifications: None,
                done_phrase: Some("I took my medication".to_string()),
                enabled: true,
            }],
            confirmation: Default::default(),
        };

        let encoded = toml::to_string(&config).unwrap();
        let decoded: Config = toml::from_str(&encoded).unwrap();

        assert!(encoded.contains("done_phrase = \"I took my medication\""));
        assert_eq!(
            decoded.reminders[0].done_phrase.as_deref(),
            Some("I took my medication")
        );
    }
}
