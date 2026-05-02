use anyhow::{Context, Result};
use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_foundation::{
    ns_string, MainThreadMarker, NSDate, NSDefaultRunLoopMode, NSRunLoop, NSString,
};

use crate::activity::{RuntimeActivity, TrayState};
use crate::store::Store;

use super::{
    activity_tooltip_lines, remaining_string, reminder_menu_label, reminder_section_title,
    runtime_activity, NoopTray, Tray,
};

const NS_VARIABLE_STATUS_ITEM_LENGTH: f64 = -1.0;

mod bindings {
    use objc2::rc::Retained;
    use objc2::{extern_class, extern_conformance, extern_methods, MainThreadOnly};
    use objc2_foundation::{MainThreadMarker, NSInteger, NSObject, NSObjectProtocol, NSString};

    extern_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[derive(Debug, PartialEq, Eq, Hash)]
        pub(super) struct NSApplication;
    );

    extern_conformance!(
        unsafe impl NSObjectProtocol for NSApplication {}
    );

    impl NSApplication {
        extern_methods!(
            #[unsafe(method(sharedApplication))]
            #[unsafe(method_family = none)]
            pub(super) fn sharedApplication(mtm: MainThreadMarker) -> Retained<Self>;

            #[unsafe(method(setActivationPolicy:))]
            #[unsafe(method_family = none)]
            pub(super) fn setActivationPolicy(&self, activation_policy: NSInteger) -> bool;
        );
    }

    extern_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[derive(Debug, PartialEq, Eq, Hash)]
        pub(super) struct NSStatusBar;
    );

    extern_conformance!(
        unsafe impl NSObjectProtocol for NSStatusBar {}
    );

    impl NSStatusBar {
        extern_methods!(
            #[unsafe(method(systemStatusBar))]
            #[unsafe(method_family = none)]
            pub(super) fn systemStatusBar(mtm: MainThreadMarker) -> Retained<Self>;

            #[unsafe(method(statusItemWithLength:))]
            #[unsafe(method_family = none)]
            pub(super) fn statusItemWithLength(&self, length: f64) -> Retained<NSStatusItem>;
        );
    }

    extern_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[derive(Debug, PartialEq, Eq, Hash)]
        pub(super) struct NSStatusItem;
    );

    extern_conformance!(
        unsafe impl NSObjectProtocol for NSStatusItem {}
    );

    impl NSStatusItem {
        extern_methods!(
            #[unsafe(method(button))]
            #[unsafe(method_family = none)]
            pub(super) fn button(&self) -> Option<Retained<NSStatusBarButton>>;

            #[unsafe(method(setMenu:))]
            #[unsafe(method_family = none)]
            pub(super) fn setMenu(&self, menu: Option<&NSMenu>);

            #[unsafe(method(setVisible:))]
            #[unsafe(method_family = none)]
            pub(super) fn setVisible(&self, visible: bool);
        );
    }

    extern_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[derive(Debug, PartialEq, Eq, Hash)]
        pub(super) struct NSStatusBarButton;
    );

    extern_conformance!(
        unsafe impl NSObjectProtocol for NSStatusBarButton {}
    );

    impl NSStatusBarButton {
        extern_methods!(
            #[unsafe(method(setTitle:))]
            #[unsafe(method_family = none)]
            pub(super) fn setTitle(&self, title: &NSString);

            #[unsafe(method(setToolTip:))]
            #[unsafe(method_family = none)]
            pub(super) fn setToolTip(&self, tooltip: Option<&NSString>);

            #[unsafe(method(setImage:))]
            #[unsafe(method_family = none)]
            pub(super) fn setImage(&self, image: Option<&NSImage>);
        );
    }

    extern_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[derive(Debug, PartialEq, Eq, Hash)]
        pub(super) struct NSImage;
    );

    extern_conformance!(
        unsafe impl NSObjectProtocol for NSImage {}
    );

    impl NSImage {
        extern_methods!(
            #[unsafe(method(initWithContentsOfFile:))]
            #[unsafe(method_family = init)]
            pub(super) fn initWithContentsOfFile(
                this: objc2::rc::Allocated<Self>,
                path: &NSString,
            ) -> Option<Retained<Self>>;

            #[unsafe(method(setTemplate:))]
            #[unsafe(method_family = none)]
            pub(super) fn setTemplate(&self, template: bool);
        );
    }

    extern_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[derive(Debug, PartialEq, Eq, Hash)]
        pub(super) struct NSMenu;
    );

    extern_conformance!(
        unsafe impl NSObjectProtocol for NSMenu {}
    );

    impl NSMenu {
        extern_methods!(
            #[unsafe(method(new))]
            #[unsafe(method_family = new)]
            pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self>;

            #[unsafe(method(addItem:))]
            #[unsafe(method_family = none)]
            pub(super) fn addItem(&self, item: &NSMenuItem);
        );
    }

    extern_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[derive(Debug, PartialEq, Eq, Hash)]
        pub(super) struct NSMenuItem;
    );

    extern_conformance!(
        unsafe impl NSObjectProtocol for NSMenuItem {}
    );

    impl NSMenuItem {
        extern_methods!(
            #[unsafe(method(separatorItem))]
            #[unsafe(method_family = none)]
            pub(super) fn separatorItem() -> Retained<Self>;

            #[unsafe(method(initWithTitle:action:keyEquivalent:))]
            #[unsafe(method_family = init)]
            pub(super) fn initWithTitle_action_keyEquivalent(
                this: objc2::rc::Allocated<Self>,
                title: &NSString,
                action: Option<objc2::runtime::Sel>,
                key_equivalent: &NSString,
            ) -> Retained<Self>;

            #[unsafe(method(setEnabled:))]
            #[unsafe(method_family = none)]
            pub(super) fn setEnabled(&self, enabled: bool);
        );
    }
}

use bindings::{
    NSApplication, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusBarButton, NSStatusItem,
};

struct MacTray {
    mtm: MainThreadMarker,
    status_item: Retained<NSStatusItem>,
    button: Retained<NSStatusBarButton>,
    // Keep a strong reference so AppKit continues to display the status image.
    _image: Retained<NSImage>,
    menu: Option<Retained<NSMenu>>,
}

impl MacTray {
    fn new(mtm: MainThreadMarker) -> Result<Self> {
        configure_status_application(mtm);

        let status_item = create_status_item(mtm);
        let button = status_button(&status_item)?;
        let image = load_status_image(mtm)?;
        initialize_status_button(&button, &image);
        hide_status_item(&status_item);

        Ok(Self {
            mtm,
            status_item,
            button,
            _image: image,
            menu: None,
        })
    }
}

impl Tray for MacTray {
    fn refresh(
        &mut self,
        config: &crate::models::Config,
        state: &crate::models::State,
    ) -> Result<()> {
        let activity = runtime_activity(config, state)?;
        set_status_item_visibility(&self.status_item, activity.tray_state != TrayState::Hidden);
        if activity.tray_state == TrayState::Hidden {
            self.menu = None;
            clear_status_item_menu(&self.status_item);
            return Ok(());
        }

        set_status_button_tooltip(&self.button, &status_tooltip(&activity));

        let menu = build_menu(self.mtm, &activity);
        set_status_item_menu(&self.status_item, &menu);
        self.menu = Some(menu);
        Ok(())
    }
}

pub fn create() -> Box<dyn Tray> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Box::new(NoopTray);
    };

    match MacTray::new(mtm) {
        Ok(tray) => Box::new(tray),
        Err(error) => {
            tracing::warn!("failed to initialize macOS status item: {error:#}");
            Box::new(NoopTray)
        }
    }
}

pub fn run_daemon(store: Store) -> Result<()> {
    let mut tray = create();
    let mut notifier = crate::notify::Handle::new()?;
    let run_loop = NSRunLoop::currentRunLoop();

    loop {
        if let Err(error) = crate::daemon::tick_with_tray(&store, tray.as_mut(), &mut notifier) {
            tracing::error!("{error:#}");
        }

        run_loop_tick(&run_loop);
    }
}

fn configure_status_application(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    // Accessory mode keeps the daemon out of the Dock while still allowing a status item.
    let _ = app.setActivationPolicy(1);
}

fn create_status_item(mtm: MainThreadMarker) -> Retained<NSStatusItem> {
    let status_bar = NSStatusBar::systemStatusBar(mtm);
    status_bar.statusItemWithLength(NS_VARIABLE_STATUS_ITEM_LENGTH)
}

fn status_button(status_item: &NSStatusItem) -> Result<Retained<NSStatusBarButton>> {
    status_item
        .button()
        .context("macOS status item did not provide a button")
}

fn load_status_image(mtm: MainThreadMarker) -> Result<Retained<NSImage>> {
    let icon_path = super::ensure_embedded_tray_icon()?;
    let image_path = NSString::from_str(&icon_path.display().to_string());
    NSImage::initWithContentsOfFile(NSImage::alloc(mtm), &image_path)
        .context("could not load embedded tray icon for macOS")
}

fn initialize_status_button(button: &NSStatusBarButton, image: &NSImage) {
    image.setTemplate(false);
    button.setImage(Some(image));
    button.setTitle(ns_string!(""));
}

fn hide_status_item(status_item: &NSStatusItem) {
    set_status_item_visibility(status_item, false);
}

fn set_status_item_visibility(status_item: &NSStatusItem, visible: bool) {
    status_item.setVisible(visible);
}

fn clear_status_item_menu(status_item: &NSStatusItem) {
    status_item.setMenu(None);
}

fn set_status_item_menu(status_item: &NSStatusItem, menu: &NSMenu) {
    status_item.setMenu(Some(menu));
}

fn set_status_button_tooltip(button: &NSStatusBarButton, tooltip: &str) {
    let tooltip = NSString::from_str(tooltip);
    button.setToolTip(Some(&tooltip));
}

fn run_loop_tick(run_loop: &NSRunLoop) {
    let next = NSDate::dateWithTimeIntervalSinceNow(1.0);
    // The tray daemon owns the main-thread run loop on macOS, so we pump it manually.
    let _ = unsafe { run_loop.runMode_beforeDate(NSDefaultRunLoopMode, &next) };
}

fn build_menu(mtm: MainThreadMarker, activity: &RuntimeActivity) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    for title in summary_lines(activity) {
        let item = label_menu_item(mtm, &title, true);
        menu.addItem(&item);
    }
    if !activity.timers.is_empty() && !activity.tray_reminders.is_empty() {
        let separator = NSMenuItem::separatorItem();
        menu.addItem(&separator);
    }
    if !activity.timers.is_empty() {
        let header = label_menu_item(mtm, "Timers", true);
        menu.addItem(&header);
        for timer in &activity.timers {
            let status = if timer.expired {
                "expired".to_string()
            } else {
                format!("{} left", remaining_string(timer.ends_at))
            };
            let item = label_menu_item(mtm, &format!("{}: {}", timer.title, status), false);
            menu.addItem(&item);
        }
    }
    if !activity.tray_reminders.is_empty() {
        if !activity.timers.is_empty() {
            let separator = NSMenuItem::separatorItem();
            menu.addItem(&separator);
        }
        let header = label_menu_item(mtm, reminder_section_title(), true);
        menu.addItem(&header);
        for reminder in &activity.tray_reminders {
            let item = label_menu_item(mtm, &reminder_menu_label(reminder), false);
            menu.addItem(&item);
        }
    }
    menu
}

fn label_menu_item(mtm: MainThreadMarker, title: &str, dimmed: bool) -> Retained<NSMenuItem> {
    let title = NSString::from_str(title);
    let item = NSMenuItem::initWithTitle_action_keyEquivalent(
        NSMenuItem::alloc(mtm),
        &title,
        None,
        ns_string!(""),
    );
    item.setEnabled(!dimmed);
    item
}

fn summary_lines(activity: &RuntimeActivity) -> Vec<String> {
    let mut lines = Vec::new();
    let running_timers = activity
        .timers
        .iter()
        .filter(|timer| !timer.expired)
        .count();
    let expired_timers = activity.timers.iter().filter(|timer| timer.expired).count();
    lines.push(format!(
        "{} {} timer(s), {} expired",
        status_prefix(activity.tray_state),
        running_timers,
        expired_timers
    ));
    lines.push(format!("{} reminder(s)", activity.tray_reminders.len()));
    lines
}

fn status_prefix(state: TrayState) -> &'static str {
    match state {
        TrayState::Hidden => "",
        TrayState::Active => "Running:",
        TrayState::Alert => "Alert:",
    }
}

fn status_tooltip(activity: &RuntimeActivity) -> String {
    let lines = activity_tooltip_lines(activity);
    if lines.is_empty() {
        "No active timers or reminders".to_string()
    } else {
        lines.join("\n")
    }
}
