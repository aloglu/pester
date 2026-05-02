use anyhow::Result;

use crate::models::{Reminder, Timer};

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
    use std::sync::mpsc;
    use std::thread;

    use anyhow::{Context, Result};
    use zbus::blocking::{Connection, Proxy, SignalIterator};
    use zbus::zvariant::{OwnedValue, Str};

    use crate::models::Timer;

    const CRITICAL_URGENCY: u8 = 2;
    const REMINDER_SOUND_NAME: &str = "alarm-clock-elapsed";

    pub struct Handle {
        proxy: Proxy<'static>,
        sound_support: SoundSupport,
        closed_rx: mpsc::Receiver<u32>,
        timer_notifications: HashMap<u32, String>,
    }

    impl Handle {
        pub fn new() -> Result<Self> {
            let connection = zbus::blocking::Connection::session().context(
                "could not connect to the user D-Bus session; desktop notifications may be unavailable in this environment",
            )?;
            let proxy = notifications_proxy(&connection)?;
            let sound_support = notification_sound_support(&proxy).unwrap_or_else(|error| {
                tracing::warn!("could not inspect notification capabilities: {error:#}");
                SoundSupport::Unknown
            });

            let (closed_tx, closed_rx) = mpsc::channel();
            spawn_close_listener(closed_tx);

            Ok(Self {
                proxy,
                sound_support,
                closed_rx,
                timer_notifications: HashMap::new(),
            })
        }

        pub fn send(&mut self, title: &str, message: &str) -> Result<()> {
            let id = send_via_proxy(&self.proxy, title, message)?;
            if matches!(self.sound_support, SoundSupport::FallbackNeeded) {
                if let Err(error) = play_sound_fallback(title) {
                    tracing::warn!("could not play Linux notification sound fallback: {error:#}");
                }
            }
            tracing::debug!("sent Linux notification {id} for {title}");
            Ok(())
        }

        pub fn send_timer(&mut self, timer: &Timer) -> Result<()> {
            let id = send_via_proxy(&self.proxy, &timer.title, &timer.message)?;
            self.timer_notifications.insert(id, timer.id.clone());
            if matches!(self.sound_support, SoundSupport::FallbackNeeded) {
                if let Err(error) = play_sound_fallback(&timer.title) {
                    tracing::warn!("could not play Linux notification sound fallback: {error:#}");
                }
            }
            Ok(())
        }

        pub fn drain_dismissed_timer_ids(&mut self) -> Vec<String> {
            let mut dismissed = Vec::new();
            while let Ok(notification_id) = self.closed_rx.try_recv() {
                if let Some(timer_id) = self.timer_notifications.remove(&notification_id) {
                    dismissed.push(timer_id);
                }
            }
            dismissed
        }
    }

    pub fn send(title: &str, message: &str) -> Result<()> {
        let connection = zbus::blocking::Connection::session().context(
            "could not connect to the user D-Bus session; desktop notifications may be unavailable in this environment",
        )?;
        let proxy = notifications_proxy(&connection)?;
        let sound_support = notification_sound_support(&proxy).unwrap_or_else(|error| {
            tracing::warn!("could not inspect notification capabilities: {error:#}");
            SoundSupport::Unknown
        });
        send_via_proxy(&proxy, title, message)?;
        if matches!(sound_support, SoundSupport::FallbackNeeded) {
            if let Err(error) = play_sound_fallback(title) {
                tracing::warn!("could not play Linux notification sound fallback: {error:#}");
            }
        }
        Ok(())
    }

    fn send_via_proxy(proxy: &Proxy<'_>, title: &str, message: &str) -> Result<u32> {
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
            Ok(id) => Ok(id),
            Err(error) if is_service_unknown(&error) => Err(error).context(
                "no Freedesktop notification service is registered on the user D-Bus session; WSL and headless Linux sessions usually need desktop notification forwarding or a notification daemon",
            ),
            Err(error) => {
                Err(error).context("the desktop notification service rejected the notification")
            }
        }
    }

    fn notifications_proxy(connection: &zbus::blocking::Connection) -> Result<Proxy<'static>> {
        let proxy = Proxy::new(
            connection,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .context("could not connect to the Freedesktop notification service")?;
        Ok(proxy)
    }

    fn spawn_close_listener(closed_tx: mpsc::Sender<u32>) {
        thread::spawn(move || {
            if let Err(error) = run_close_listener(closed_tx) {
                tracing::warn!("Linux notification close listener stopped: {error:#}");
            }
        });
    }

    fn run_close_listener(closed_tx: mpsc::Sender<u32>) -> Result<()> {
        let connection = zbus::blocking::Connection::session()
            .context("could not connect close-listener to the user D-Bus session")?;
        let proxy = notifications_proxy(&connection)?;
        let mut signals = proxy
            .receive_signal("NotificationClosed")
            .context("could not subscribe to NotificationClosed signals")?;
        forward_closed_notifications(&mut signals, closed_tx)
    }

    fn forward_closed_notifications(
        signals: &mut SignalIterator<'_>,
        closed_tx: mpsc::Sender<u32>,
    ) -> Result<()> {
        for signal in signals {
            let (notification_id, _reason): (u32, u32) = signal
                .body()
                .deserialize()
                .context("could not decode NotificationClosed signal")?;
            if closed_tx.send(notification_id).is_err() {
                break;
            }
        }
        Ok(())
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
                    "notification sound: unavailable (server has no sound capability and canberra-gtk-play is not installed)".to_string()
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
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use anyhow::{Context, Result};
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2::runtime::ProtocolObject;
    use objc2::{define_class, msg_send, MainThreadOnly};
    use objc2_foundation::{
        MainThreadMarker, NSArray, NSError, NSObject, NSObjectProtocol, NSSet, NSString,
    };
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
        UNNotificationCategory, UNNotificationCategoryOptions,
        UNNotificationDefaultActionIdentifier, UNNotificationDismissActionIdentifier,
        UNNotificationInterruptionLevel, UNNotificationPresentationOptionNone,
        UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
        UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    use crate::models::Timer;

    const TIMER_NOTIFICATION_CATEGORY: &str = "pester.timer";
    const TIMER_NOTIFICATION_PREFIX: &str = "pester-timer:";

    static TIMER_RESPONSE_TX: OnceLock<Mutex<Option<mpsc::Sender<String>>>> = OnceLock::new();

    pub struct Handle {
        _delegate: Retained<NotificationDelegate>,
        response_rx: mpsc::Receiver<String>,
    }

    impl Handle {
        pub fn new() -> Result<Self> {
            let mtm = MainThreadMarker::new().ok_or_else(|| {
                anyhow::anyhow!("macOS notifications must initialize on the main thread")
            })?;
            let center = UNUserNotificationCenter::currentNotificationCenter();
            install_timer_category(&center);

            let (response_tx, response_rx) = mpsc::channel();
            let shared_tx = TIMER_RESPONSE_TX.get_or_init(|| Mutex::new(None));
            *shared_tx
                .lock()
                .expect("macOS notification sender lock poisoned") = Some(response_tx);

            let delegate = NotificationDelegate::new(mtm);
            center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

            Ok(Self {
                _delegate: delegate,
                response_rx,
            })
        }

        pub fn send(&mut self, title: &str, message: &str) -> Result<()> {
            send(title, message)
        }

        pub fn send_timer(&mut self, timer: &Timer) -> Result<()> {
            send_timer_notification(timer)
        }

        pub fn drain_dismissed_timer_ids(&mut self) -> Vec<String> {
            let mut dismissed = Vec::new();
            while let Ok(timer_id) = self.response_rx.try_recv() {
                dismissed.push(timer_id);
            }
            dismissed
        }
    }

    define_class!(
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        struct NotificationDelegate;

        unsafe impl NSObjectProtocol for NotificationDelegate {}

        unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn user_notification_center_did_receive_notification_response_with_completion_handler(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                completion_handler: &block2::DynBlock<dyn Fn()>,
            ) {
                handle_timer_response(response);
                completion_handler.call(());
            }

            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn user_notification_center_will_present_notification_with_completion_handler(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                completion_handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                completion_handler.call((UNNotificationPresentationOptionNone,));
            }
        }
    );

    impl NotificationDelegate {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm);
            unsafe { msg_send![this, init] }
        }
    }

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

    fn send_timer_notification(timer: &Timer) -> Result<()> {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        request_authorization(&center)?;

        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&timer.title));
        content.setBody(&NSString::from_str(&timer.message));
        content.setCategoryIdentifier(&NSString::from_str(TIMER_NOTIFICATION_CATEGORY));
        let sound = UNNotificationSound::defaultSound();
        content.setSound(Some(&sound));
        content.setInterruptionLevel(UNNotificationInterruptionLevel::TimeSensitive);

        let identifier = NSString::from_str(&timer_request_identifier(&timer.id));
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

    fn install_timer_category(center: &UNUserNotificationCenter) {
        let identifier = NSString::from_str(TIMER_NOTIFICATION_CATEGORY);
        let actions = NSArray::from_slice(&[]);
        let intents: Retained<NSArray<NSString>> = NSArray::from_slice(&[]);
        let category =
            UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
                &identifier,
                &actions,
                &intents,
                UNNotificationCategoryOptions::CustomDismissAction,
            );
        let categories = NSSet::from_retained_slice(&[category]);
        center.setNotificationCategories(&categories);
    }

    fn handle_timer_response(response: &UNNotificationResponse) {
        let action = response.actionIdentifier();
        let action: &NSString = &action;
        let should_clear = action == UNNotificationDismissActionIdentifier
            || action == UNNotificationDefaultActionIdentifier;
        if !should_clear {
            return;
        }

        let request = response.notification().request();
        let request_id = request.identifier().to_string();
        let Some(timer_id) = timer_id_from_request_identifier(&request_id) else {
            return;
        };

        if let Some(shared_tx) = TIMER_RESPONSE_TX.get() {
            if let Some(tx) = shared_tx
                .lock()
                .expect("macOS notification sender lock poisoned")
                .as_ref()
            {
                let _ = tx.send(timer_id.to_string());
            }
        }
    }

    fn timer_request_identifier(timer_id: &str) -> String {
        format!("{TIMER_NOTIFICATION_PREFIX}{timer_id}")
    }

    fn timer_id_from_request_identifier(request_id: &str) -> Option<&str> {
        request_id.strip_prefix(TIMER_NOTIFICATION_PREFIX)
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use anyhow::{Context, Result};
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    use crate::models::Timer;

    pub struct Handle;

    impl Handle {
        pub fn new() -> Result<Self> {
            Ok(Self)
        }

        pub fn send(&mut self, title: &str, message: &str) -> Result<()> {
            send(title, message)
        }

        pub fn send_timer(&mut self, timer: &Timer) -> Result<()> {
            send(&timer.title, &timer.message)
        }

        pub fn drain_dismissed_timer_ids(&mut self) -> Vec<String> {
            Vec::new()
        }
    }

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

    use crate::models::Timer;

    pub struct Handle;

    impl Handle {
        pub fn new() -> Result<Self> {
            Ok(Self)
        }

        pub fn send(&mut self, title: &str, message: &str) -> Result<()> {
            send(title, message)
        }

        pub fn send_timer(&mut self, timer: &Timer) -> Result<()> {
            send(&timer.title, &timer.message)
        }

        pub fn drain_dismissed_timer_ids(&mut self) -> Vec<String> {
            Vec::new()
        }
    }

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
