use anyhow::Result;

use crate::models::{Reminder, Timer};

pub fn send(reminder: &Reminder) -> Result<()> {
    send_notification(&reminder.title, &reminder.message)
}

pub fn send_timer(timer: &Timer) -> Result<()> {
    send_notification(&timer.title, &timer.message)
}

pub fn diagnostics() -> Vec<String> {
    platform::diagnostics()
}

fn send_notification(title: &str, message: &str) -> Result<()> {
    platform::send(title, message)
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

#[cfg(target_os = "linux")]
mod platform {
    use std::collections::HashMap;
    use std::env;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use anyhow::{Context, Result};
    use zbus::blocking::Connection;
    use zbus::zvariant::{OwnedValue, Str};

    const CRITICAL_URGENCY: u8 = 2;
    const REMINDER_SOUND_NAME: &str = "alarm-clock-elapsed";

    pub fn send(title: &str, message: &str) -> Result<()> {
        let connection = Connection::session()
            .context("could not connect to the user D-Bus session; desktop notifications may be unavailable in this environment")?;
        let proxy = zbus::blocking::Proxy::new(
            &connection,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .context("could not connect to the Freedesktop notification service")?;
        let sound_support = notification_sound_support(&proxy).unwrap_or_else(|error| {
            tracing::warn!("could not inspect notification capabilities: {error:#}");
            SoundSupport::Unknown
        });

        let actions: Vec<&str> = Vec::new();
        let mut hints: HashMap<&str, OwnedValue> = HashMap::new();
        hints.insert("urgency", CRITICAL_URGENCY.into());
        hints.insert("sound-name", Str::from_static(REMINDER_SOUND_NAME).into());
        let timeout_ms = -1i32;
        let replaces_id = 0u32;

        let result: std::result::Result<u32, zbus::Error> = proxy.call(
            "Notify",
            &(
                super::app_name(),
                replaces_id,
                "",
                title,
                message,
                actions,
                hints,
                timeout_ms,
            ),
        );

        match result {
            Ok(_) => {
                if matches!(sound_support, SoundSupport::FallbackNeeded) {
                    if let Err(error) = play_sound_fallback(title) {
                        tracing::warn!(
                            "could not play Linux notification sound fallback: {error:#}"
                        );
                    }
                }
                Ok(())
            }
            Err(error) if is_service_unknown(&error) => Err(error).context(
                "no Freedesktop notification service is registered on the user D-Bus session; WSL and headless Linux sessions usually need desktop notification forwarding or a notification daemon",
            ),
            Err(error) => {
                Err(error).context("the desktop notification service rejected the notification")
            }
        }
    }

    pub fn diagnostics() -> Vec<String> {
        let connection = Connection::session();
        let Ok(connection) = connection else {
            return vec![
                "D-Bus session: unavailable".to_string(),
                format!("notifications: unavailable ({:#})", connection.unwrap_err()),
                "hint: desktop Linux needs a user D-Bus session and a notification daemon; WSL/headless sessions usually need forwarding".to_string(),
            ];
        };

        let proxy = zbus::blocking::Proxy::new(
            &connection,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        );
        let Ok(proxy) = proxy else {
            return vec![
                "D-Bus session: available".to_string(),
                format!("notifications: unavailable ({:#})", proxy.unwrap_err()),
                "hint: install or start a Freedesktop notification daemon such as your desktop shell, dunst, or mako".to_string(),
            ];
        };

        let info: Result<(String, String, String, String), zbus::Error> =
            proxy.call("GetServerInformation", &());
        match info {
            Ok((name, vendor, version, spec_version)) => vec![
                "D-Bus session: available".to_string(),
                format!(
                    "notifications: available ({name} {version}, {vendor}, spec {spec_version})"
                ),
                capabilities_diagnostics(&proxy),
                sound_diagnostics(&proxy),
            ],
            Err(error) => vec![
                "D-Bus session: available".to_string(),
                format!("notifications: unavailable ({error:#})"),
            ],
        }
    }

    fn notification_sound_support(
        proxy: &zbus::blocking::Proxy<'_>,
    ) -> Result<SoundSupport, zbus::Error> {
        let capabilities = notification_capabilities(proxy)?;
        Ok(if supports_sound(&capabilities) {
            SoundSupport::Native
        } else {
            SoundSupport::FallbackNeeded
        })
    }

    fn notification_capabilities(
        proxy: &zbus::blocking::Proxy<'_>,
    ) -> Result<Vec<String>, zbus::Error> {
        proxy.call("GetCapabilities", &())
    }

    pub(super) fn supports_sound(capabilities: &[String]) -> bool {
        capabilities.iter().any(|capability| capability == "sound")
    }

    fn capabilities_diagnostics(proxy: &zbus::blocking::Proxy<'_>) -> String {
        match notification_capabilities(proxy) {
            Ok(capabilities) if capabilities.is_empty() => {
                "notification capabilities: none".to_string()
            }
            Ok(capabilities) => format!("notification capabilities: {}", capabilities.join(", ")),
            Err(error) => format!("notification capabilities: unknown ({error:#})"),
        }
    }

    fn sound_diagnostics(proxy: &zbus::blocking::Proxy<'_>) -> String {
        match notification_sound_support(proxy) {
            Ok(SoundSupport::Native) => {
                "notification sound: handled by the notification server".to_string()
            }
            Ok(SoundSupport::FallbackNeeded) => {
                if has_canberra_gtk_play() {
                    format!(
                        "notification sound: using local fallback via canberra-gtk-play ({REMINDER_SOUND_NAME})"
                    )
                } else {
                    format!(
                        "notification sound: unavailable (server has no sound capability and canberra-gtk-play is not installed)"
                    )
                }
            }
            Ok(SoundSupport::Unknown) => "notification sound: unknown".to_string(),
            Err(error) => format!("notification sound: unknown ({error:#})"),
        }
    }

    fn play_sound_fallback(title: &str) -> Result<()> {
        let canberra = find_canberra_gtk_play()
            .context("notification server has no sound capability and canberra-gtk-play was not found in PATH")?;
        let status = Command::new(canberra)
            .args(["--id", REMINDER_SOUND_NAME, "--description", title])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("failed to start canberra-gtk-play")?;
        if !status.success() {
            anyhow::bail!("canberra-gtk-play exited with status {status}");
        }
        Ok(())
    }

    fn has_canberra_gtk_play() -> bool {
        find_canberra_gtk_play().is_some()
    }

    fn find_canberra_gtk_play() -> Option<std::path::PathBuf> {
        let path = env::var_os("PATH")?;
        env::split_paths(&path)
            .map(|dir| dir.join("canberra-gtk-play"))
            .find(|candidate| candidate.is_file() && is_executable(candidate))
    }

    fn is_executable(path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;

        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    fn is_service_unknown(error: &zbus::Error) -> bool {
        let message = error.to_string();
        message.contains("org.freedesktop.DBus.Error.ServiceUnknown")
            || message.contains("was not provided by any .service files")
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum SoundSupport {
        Native,
        FallbackNeeded,
        Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::{app_id, app_name, escape_xml_text};

    #[cfg(target_os = "linux")]
    use super::platform::supports_sound;

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

#[cfg(target_os = "macos")]
mod platform {
    use std::ptr::NonNull;
    use std::sync::mpsc;
    use std::time::Duration;

    use anyhow::{Context, Result};
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationInterruptionLevel,
        UNNotificationRequest, UNNotificationSound, UNUserNotificationCenter,
    };

    pub fn send(title: &str, message: &str) -> Result<()> {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        request_authorization(&center)?;

        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(message));
        let sound = UNNotificationSound::defaultSound();
        content.setSound(Some(&sound));
        content.setInterruptionLevel(UNNotificationInterruptionLevel::TimeSensitive);

        let identifier = NSString::from_str("pester-notification");
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &identifier,
            &content,
            None,
        );
        add_request(&center, &request)?;
        Ok(())
    }

    pub fn diagnose() -> String {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        match request_authorization(&center) {
            Ok(()) => "available".to_string(),
            Err(error) => format!("unavailable ({error:#})"),
        }
    }

    pub fn diagnostics() -> Vec<String> {
        vec![format!("notifications: {}", diagnose())]
    }

    fn request_authorization(center: &UNUserNotificationCenter) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let completion = RcBlock::new(move |granted: Bool, error: *mut NSError| {
            let result = if let Some(error) = nonnull_error(error) {
                Err(format!(
                    "macOS notification authorization failed: {}",
                    unsafe { error.as_ref() }
                ))
            } else if granted.as_bool() {
                Ok(())
            } else {
                Err("macOS notification permission was not granted".to_string())
            };
            let _ = tx.send(result);
        });

        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &completion,
        );

        rx.recv_timeout(Duration::from_secs(10))
            .context("timed out waiting for macOS notification authorization")?
            .map_err(anyhow::Error::msg)
    }

    fn add_request(
        center: &UNUserNotificationCenter,
        request: &UNNotificationRequest,
    ) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let completion = RcBlock::new(move |error: *mut NSError| {
            let result = if let Some(error) = nonnull_error(error) {
                Err(format!(
                    "macOS rejected the notification request: {}",
                    unsafe { error.as_ref() }
                ))
            } else {
                Ok(())
            };
            let _ = tx.send(result);
        });

        center.addNotificationRequest_withCompletionHandler(request, Some(&completion));

        rx.recv_timeout(Duration::from_secs(10))
            .context("timed out waiting for macOS to schedule notification")?
            .map_err(anyhow::Error::msg)
    }

    fn nonnull_error(error: *mut NSError) -> Option<NonNull<NSError>> {
        NonNull::new(error)
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use anyhow::{Context, Result};
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    pub fn send(title: &str, message: &str) -> Result<()> {
        let document = toast_document(title, message)?;
        let notification = ToastNotification::CreateToastNotification(&document)
            .context("could not create Windows Toast notification")?;
        let notifier =
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(super::app_id()))
                .context("could not create Windows Toast notifier")?;
        notifier
            .Show(&notification)
            .context("Windows rejected the Toast notification")?;
        Ok(())
    }

    pub fn diagnose() -> String {
        match ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(super::app_id())) {
            Ok(_) => "available".to_string(),
            Err(error) => format!("unavailable ({error:#})"),
        }
    }

    pub fn diagnostics() -> Vec<String> {
        vec![
            format!("notifications: {}", diagnose()),
            format!("AppUserModelID: {}", super::app_id()),
        ]
    }

    fn toast_document(title: &str, message: &str) -> Result<XmlDocument> {
        let xml = format!(
            r#"<toast>
  <visual>
    <binding template="ToastGeneric">
      <text>{}</text>
      <text>{}</text>
    </binding>
  </visual>
</toast>"#,
            super::escape_xml_text(title),
            super::escape_xml_text(message)
        );
        let document = XmlDocument::new().context("could not create Windows Toast XML document")?;
        document
            .LoadXml(&HSTRING::from(xml))
            .context("could not load Windows Toast XML")?;
        Ok(document)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use anyhow::bail;
    use anyhow::Result;

    pub fn send(_title: &str, _message: &str) -> Result<()> {
        bail!("notifications are only supported on Linux, macOS, and Windows")
    }

    pub fn diagnose() -> String {
        "unsupported platform".to_string()
    }

    pub fn diagnostics() -> Vec<String> {
        vec![format!("notifications: {}", diagnose())]
    }
}
