use anyhow::Result;

use crate::models::{Reminder, Timer};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use self::linux as platform;
#[cfg(target_os = "macos")]
use self::macos as platform;
#[cfg(target_os = "windows")]
use self::windows as platform;

pub struct Handle {
    platform: platform::Handle,
}

impl Handle {
    pub fn new() -> Result<Self> {
        Ok(Self {
            platform: platform::Handle::new()?,
        })
    }

    pub fn send_reminder(&mut self, reminder: &Reminder) -> Result<()> {
        self.platform.send(&reminder.title, &reminder.message)?;
        Ok(())
    }

    pub fn send_timer(&mut self, timer: &Timer) -> Result<()> {
        self.platform.send_timer(timer)?;
        Ok(())
    }

    pub fn drain_dismissed_timer_ids(&mut self) -> Vec<String> {
        self.platform.drain_dismissed_timer_ids()
    }
}

pub fn send(reminder: &Reminder) -> Result<()> {
    let mut handle = Handle::new()?;
    handle.send_reminder(reminder)
}

pub fn send_timer(timer: &Timer) -> Result<()> {
    let mut handle = Handle::new()?;
    handle.send_timer(timer)
}

pub fn diagnostics() -> Vec<String> {
    platform::diagnostics()
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn app_name() -> &'static str {
    crate::app::APP_NAME
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn app_id() -> &'static str {
    crate::app::APP_ID
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn escape_xml_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::{app_id, app_name, escape_xml_text};

    #[cfg(target_os = "linux")]
    use super::linux::supports_sound;

    #[test]
    fn notification_app_name_is_stable() {
        assert_eq!(app_name(), "pester");
        assert_eq!(app_id(), "com.aloglu.pester");
    }

    #[test]
    fn escapes_notification_text_for_xml() {
        assert_eq!(
            escape_xml_text("Wind <down> & \"sleep\" 'now'"),
            "Wind &lt;down&gt; &amp; &quot;sleep&quot; &apos;now&apos;"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detects_notification_sound_capability() {
        assert!(supports_sound(&["sound".to_string(), "body".to_string()]));
        assert!(!supports_sound(&[
            "body".to_string(),
            "actions".to_string()
        ]));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ignores_empty_capability_list_for_sound_support() {
        assert!(!supports_sound(&[]));
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use anyhow::Result;

    use crate::models::Timer;

    pub struct Handle;

    impl Handle {
        pub fn new() -> Result<Self> {
            Ok(Self)
        }

        pub fn send(&mut self, _title: &str, _message: &str) -> Result<()> {
            tracing::warn!("notifications are not supported on this platform");
            Ok(())
        }

        pub fn send_timer(&mut self, timer: &Timer) -> Result<()> {
            self.send(&timer.title, &timer.message)
        }

        pub fn drain_dismissed_timer_ids(&mut self) -> Vec<String> {
            Vec::new()
        }
    }

    pub fn diagnostics() -> Vec<String> {
        vec!["notifications: unsupported on this platform".to_string()]
    }
}
