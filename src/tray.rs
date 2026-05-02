use anyhow::Result;
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

use crate::activity::{ReminderTrayState, RuntimeActivity, TrayReminder};
use crate::models::{Config, State};
use crate::store::Store;

#[cfg(target_os = "linux")]
const TRAY_ICON_NAME: &str = "pester-tray-v4";
const TRAY_ICON_FILENAME: &str = "pester-tray-v4.svg";
const TRAY_ICON_SVG: &str = include_str!("../assets/icons/pester-tray.svg");

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
#[allow(non_snake_case)]
mod macos;

#[cfg(target_os = "linux")]
use self::linux as platform;
#[cfg(target_os = "macos")]
use self::macos as platform;

pub trait Tray {
    fn refresh(&mut self, config: &Config, state: &State) -> Result<()>;
}

pub struct NoopTray;

impl Tray for NoopTray {
    fn refresh(&mut self, _config: &Config, _state: &State) -> Result<()> {
        Ok(())
    }
}

pub fn create() -> Box<dyn Tray> {
    platform::create()
}

pub fn runtime_activity(config: &Config, state: &State) -> Result<RuntimeActivity> {
    RuntimeActivity::collect(config, state, chrono::Local::now())
}

pub fn run_daemon(store: Store) -> Result<()> {
    platform::run_daemon(store)
}

fn ensure_embedded_tray_icon() -> Result<PathBuf> {
    let project = ProjectDirs::from("", "aloglu", "pester")
        .ok_or_else(|| anyhow::anyhow!("could not determine platform directories for tray icon"))?;
    let icon_dir = project.data_local_dir().join("icons");
    fs::create_dir_all(&icon_dir)?;
    let icon_path = icon_dir.join(TRAY_ICON_FILENAME);
    if fs::read_to_string(&icon_path).ok().as_deref() != Some(TRAY_ICON_SVG) {
        fs::write(&icon_path, TRAY_ICON_SVG)?;
    }
    Ok(icon_path)
}

fn reminder_section_title() -> &'static str {
    "Reminders"
}

fn timer_tooltip_line(timer: &crate::activity::ActiveTimer) -> String {
    let detail = if timer.expired {
        "expired".to_string()
    } else {
        format!("{} left", remaining_string(timer.ends_at))
    };
    format!("Timer: {} ({detail})", timer.title)
}

fn reminder_tooltip_line(reminder: &TrayReminder) -> String {
    let detail = match reminder.state {
        ReminderTrayState::ActiveWindow => {
            format!("active until {}", reminder.relevant_at.format("%H:%M"))
        }
        ReminderTrayState::Scheduled => {
            format!("next in {}", remaining_string(reminder.relevant_at))
        }
    };
    format!("Reminder: {} ({detail})", reminder.title)
}

fn reminder_menu_label(reminder: &TrayReminder) -> String {
    match reminder.state {
        ReminderTrayState::ActiveWindow => format!(
            "{}: active until {}",
            reminder.title,
            reminder.relevant_at.format("%H:%M")
        ),
        ReminderTrayState::Scheduled => format!(
            "{}: next in {}",
            reminder.title,
            remaining_string(reminder.relevant_at)
        ),
    }
}

fn activity_tooltip_lines(activity: &RuntimeActivity) -> Vec<String> {
    let mut lines = Vec::new();
    for timer in &activity.timers {
        lines.push(timer_tooltip_line(timer));
    }
    for reminder in &activity.tray_reminders {
        lines.push(reminder_tooltip_line(reminder));
    }
    lines
}

fn remaining_string(ends_at: chrono::DateTime<chrono::Local>) -> String {
    let remaining = ends_at.signed_duration_since(chrono::Local::now());
    if remaining.num_seconds() <= 0 {
        return "expired".to_string();
    }
    let total_seconds = remaining.num_seconds();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    if minutes >= 60 {
        let hours = minutes / 60;
        let remainder_minutes = minutes % 60;
        format!("{hours}h {remainder_minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform {
    use anyhow::Result;

    use crate::store::Store;

    use super::{NoopTray, Tray};

    pub fn create() -> Box<dyn Tray> {
        Box::new(NoopTray)
    }

    pub fn run_daemon(store: Store) -> Result<()> {
        crate::daemon::run(store)
    }
}
