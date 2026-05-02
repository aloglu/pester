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

fn send(title: &str, message: &str) -> Result<()> {
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
