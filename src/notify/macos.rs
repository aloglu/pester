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
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationCategory,
    UNNotificationCategoryOptions, UNNotificationDefaultActionIdentifier,
    UNNotificationDismissActionIdentifier, UNNotificationInterruptionLevel,
    UNNotificationPresentationOptionNone, UNNotificationPresentationOptions, UNNotificationRequest,
    UNNotificationResponse, UNNotificationSound, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};

use crate::models::Timer;

const TIMER_NOTIFICATION_CATEGORY: &str = "pester.timer";
const TIMER_NOTIFICATION_PREFIX: &str = "pester-timer:";

static TIMER_RESPONSE_TX: OnceLock<Mutex<Option<mpsc::Sender<String>>>> = OnceLock::new();

pub struct Handle {
    // The center keeps only a weak reference to its delegate.
    _delegate: Retained<NotificationDelegate>,
    response_rx: mpsc::Receiver<String>,
}

impl Handle {
    pub fn new() -> Result<Self> {
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            anyhow::anyhow!("macOS notifications must initialize on the main thread")
        })?;
        let center = notification_center();
        install_timer_category(&center);

        let (response_tx, response_rx) = mpsc::channel();
        replace_timer_response_sender(response_tx);
        let delegate = install_delegate(&center, mtm);

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

fn notification_center() -> Retained<UNUserNotificationCenter> {
    UNUserNotificationCenter::currentNotificationCenter()
}

fn install_delegate(
    center: &UNUserNotificationCenter,
    mtm: MainThreadMarker,
) -> Retained<NotificationDelegate> {
    let delegate = NotificationDelegate::new(mtm);
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    delegate
}

fn send(title: &str, message: &str) -> Result<()> {
    let center = notification_center();
    request_authorization(&center)?;

    let content = base_content(title, message);
    let request = notification_request("pester-notification", &content);
    add_request(&center, &request)?;
    Ok(())
}

fn send_timer_notification(timer: &Timer) -> Result<()> {
    let center = notification_center();
    request_authorization(&center)?;

    let content = base_content(&timer.title, &timer.message);
    content.setCategoryIdentifier(&NSString::from_str(TIMER_NOTIFICATION_CATEGORY));
    let request = notification_request(&timer_request_identifier(&timer.id), &content);
    add_request(&center, &request)?;
    Ok(())
}

pub fn diagnose() -> String {
    let center = notification_center();
    match request_authorization(&center) {
        Ok(()) => "available".to_string(),
        Err(error) => format!("unavailable ({error:#})"),
    }
}

pub fn diagnostics() -> Vec<String> {
    vec![format!("notifications: {}", diagnose())]
}

fn base_content(title: &str, message: &str) -> Retained<UNMutableNotificationContent> {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(message));
    let sound = UNNotificationSound::defaultSound();
    content.setSound(Some(&sound));
    content.setInterruptionLevel(UNNotificationInterruptionLevel::TimeSensitive);
    content
}

fn notification_request(
    identifier: &str,
    content: &UNMutableNotificationContent,
) -> Retained<UNNotificationRequest> {
    let identifier = NSString::from_str(identifier);
    UNNotificationRequest::requestWithIdentifier_content_trigger(&identifier, content, None)
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

fn add_request(center: &UNUserNotificationCenter, request: &UNNotificationRequest) -> Result<()> {
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

fn replace_timer_response_sender(sender: mpsc::Sender<String>) {
    let shared_tx = TIMER_RESPONSE_TX.get_or_init(|| Mutex::new(None));
    // Only the current daemon instance should receive dismissal callbacks.
    *shared_tx
        .lock()
        .expect("macOS notification sender lock poisoned") = Some(sender);
}

fn forward_timer_response(timer_id: &str) {
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

fn nonnull_error(error: *mut NSError) -> Option<NonNull<NSError>> {
    NonNull::new(error)
}

fn install_timer_category(center: &UNUserNotificationCenter) {
    let identifier = NSString::from_str(TIMER_NOTIFICATION_CATEGORY);
    let actions = NSArray::from_slice(&[]);
    let intents: Retained<NSArray<NSString>> = NSArray::from_slice(&[]);
    let category = UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
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
    let dismiss = unsafe { UNNotificationDismissActionIdentifier };
    let default = unsafe { UNNotificationDefaultActionIdentifier };
    // Treat both explicit dismiss and default open as timer acknowledgement.
    let should_clear = action == dismiss || action == default;
    if !should_clear {
        return;
    }

    let request = response.notification().request();
    let request_id = request.identifier().to_string();
    let Some(timer_id) = timer_id_from_request_identifier(&request_id) else {
        return;
    };

    forward_timer_response(timer_id);
}

fn timer_request_identifier(timer_id: &str) -> String {
    format!("{TIMER_NOTIFICATION_PREFIX}{timer_id}")
}

fn timer_id_from_request_identifier(request_id: &str) -> Option<&str> {
    request_id.strip_prefix(TIMER_NOTIFICATION_PREFIX)
}
