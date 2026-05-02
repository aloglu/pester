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
            format!("notifications: available ({name} {version}, {vendor}, spec {spec_version})"),
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
    let canberra = find_canberra_gtk_play().context(
        "notification server has no sound capability and canberra-gtk-play was not found in PATH",
    )?;
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
