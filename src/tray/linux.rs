use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use zbus::blocking::{Connection, Proxy};
use zbus::interface;
use zbus::names::WellKnownName;
use zbus::object_server::SignalContext;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};

use crate::activity::{RuntimeActivity, TrayState};
use crate::models::{Config, State};

use super::{
    activity_tooltip_lines, remaining_string, reminder_menu_label, reminder_section_title,
    runtime_activity, Tray,
};

const WATCHERS: &[(&str, &str)] = &[
    ("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher"),
    (
        "org.freedesktop.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
    ),
];
const ITEM_OBJECT_PATH: &str = "/StatusNotifierItem";
const MENU_OBJECT_PATH: &str = "/StatusNotifierMenu";
type Layout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);
type ToolTip = (String, Vec<(i32, i32, Vec<u8>)>, String, String);

pub fn create() -> Box<dyn Tray> {
    match LinuxTray::new() {
        Ok(tray) => Box::new(tray),
        Err(error) => {
            tracing::warn!("failed to initialize Linux status item: {error:#}");
            Box::new(super::NoopTray)
        }
    }
}

pub fn run_daemon(store: crate::store::Store) -> Result<()> {
    crate::daemon::run(store)
}

pub struct LinuxTray {
    connection: Connection,
    item_name: String,
    state: Arc<Mutex<TrayModel>>,
    item_iface: zbus::blocking::object_server::InterfaceRef<StatusNotifierItem>,
    menu_iface: zbus::blocking::object_server::InterfaceRef<DbusMenu>,
    watcher_registered: bool,
}

impl LinuxTray {
    pub fn new() -> Result<Self> {
        let connection =
            Connection::session().context("could not connect to the user D-Bus session")?;
        let icon_path = super::ensure_embedded_tray_icon()?;
        let icon_dir = icon_path
            .parent()
            .context("tray icon path did not have a parent directory")?
            .to_string_lossy()
            .into_owned();
        let item_name = format!(
            "org.freedesktop.StatusNotifierItem-{}-1",
            std::process::id()
        );
        connection
            .request_name(WellKnownName::try_from(item_name.as_str())?)
            .with_context(|| format!("could not request D-Bus name {item_name}"))?;

        let state = Arc::new(Mutex::new(TrayModel::default()));
        let connection_for_server = connection.clone();
        let object_server = connection_for_server.object_server();
        object_server.at(
            ITEM_OBJECT_PATH,
            StatusNotifierItem {
                state: state.clone(),
                icon_theme_path: icon_dir,
            },
        )?;
        object_server.at(
            MENU_OBJECT_PATH,
            DbusMenu {
                state: state.clone(),
            },
        )?;

        let item_iface = object_server.interface::<_, StatusNotifierItem>(ITEM_OBJECT_PATH)?;
        let menu_iface = object_server.interface::<_, DbusMenu>(MENU_OBJECT_PATH)?;

        Ok(Self {
            connection,
            item_name,
            state,
            item_iface,
            menu_iface,
            watcher_registered: false,
        })
    }

    fn ensure_registered(&mut self) {
        if self.watcher_registered {
            return;
        }

        for (name, path) in WATCHERS {
            let proxy = Proxy::new(&self.connection, *name, *path, *name);
            let Ok(proxy) = proxy else {
                continue;
            };
            let result =
                proxy.call_method("RegisterStatusNotifierItem", &(self.item_name.as_str()));
            if result.is_ok() {
                self.watcher_registered = true;
                return;
            }
        }
    }

    fn emit_item_updates(
        &self,
        previous: TrayState,
        current: TrayState,
        revision: u32,
    ) -> Result<()> {
        if previous != current {
            zbus::block_on(StatusNotifierItem::new_status(
                self.item_iface.signal_context(),
                tray_status_name(current),
            ))?;
        }

        zbus::block_on(StatusNotifierItem::new_tool_tip(
            self.item_iface.signal_context(),
        ))?;
        zbus::block_on(DbusMenu::layout_updated(
            self.menu_iface.signal_context(),
            revision,
            0,
        ))?;
        Ok(())
    }
}

impl Tray for LinuxTray {
    fn refresh(&mut self, config: &Config, state: &State) -> Result<()> {
        let activity = runtime_activity(config, state)?;
        let tray_state = activity.tray_state;
        let (previous, revision) = {
            let mut model = self.state.lock().expect("tray model lock poisoned");
            let previous = model.activity.tray_state;
            model.activity = activity;
            model.revision = model.revision.wrapping_add(1).max(1);
            model.status = if model.activity.tray_state == TrayState::Alert {
                "notice".to_string()
            } else {
                "normal".to_string()
            };
            (previous, model.revision)
        };

        self.ensure_registered();
        self.emit_item_updates(previous, tray_state, revision)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct TrayModel {
    activity: RuntimeActivity,
    revision: u32,
    status: String,
}

impl Default for TrayModel {
    fn default() -> Self {
        Self {
            activity: RuntimeActivity {
                tray_state: TrayState::Hidden,
                tray_reminders: Vec::new(),
                timers: Vec::new(),
            },
            revision: 1,
            status: "normal".to_string(),
        }
    }
}

struct StatusNotifierItem {
    state: Arc<Mutex<TrayModel>>,
    icon_theme_path: String,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }

    #[zbus(property)]
    fn id(&self) -> &str {
        crate::app::APP_NAME
    }

    #[zbus(property)]
    fn title(&self) -> &str {
        crate::app::APP_NAME
    }

    #[zbus(property)]
    fn status(&self) -> String {
        tray_status_name(
            self.state
                .lock()
                .expect("tray model lock poisoned")
                .activity
                .tray_state,
        )
        .to_string()
    }

    #[zbus(property)]
    fn icon_name(&self) -> String {
        stable_linux_icon_name().to_string()
    }

    #[zbus(property)]
    fn attention_icon_name(&self) -> &str {
        stable_linux_icon_name()
    }

    #[zbus(property)]
    fn tool_tip(&self) -> ToolTip {
        let model = self.state.lock().expect("tray model lock poisoned");
        (
            self.icon_name(),
            Vec::new(),
            "pester".to_string(),
            tooltip_text(&model.activity),
        )
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        ObjectPath::try_from(MENU_OBJECT_PATH)
            .expect("valid menu object path")
            .into()
    }

    #[zbus(property)]
    fn window_id(&self) -> u32 {
        0
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> &str {
        &self.icon_theme_path
    }

    fn context_menu(&self, _x: i32, _y: i32) {}

    fn activate(&self, _x: i32, _y: i32) {}

    fn secondary_activate(&self, _x: i32, _y: i32) {}

    fn scroll(&self, _delta: i32, _orientation: &str) {}

    #[zbus(signal)]
    async fn new_status(ctxt: &SignalContext<'_>, status: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_tool_tip(ctxt: &SignalContext<'_>) -> zbus::Result<()>;
}

struct DbusMenu {
    state: Arc<Mutex<TrayModel>>,
}

#[interface(name = "com.canonical.dbusmenu")]
impl DbusMenu {
    #[zbus(property)]
    fn version(&self) -> u32 {
        4
    }

    #[zbus(property)]
    fn status(&self) -> String {
        self.state
            .lock()
            .expect("tray model lock poisoned")
            .status
            .clone()
    }

    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> (u32, Layout) {
        let model = self.state.lock().expect("tray model lock poisoned");
        (
            model.revision,
            layout_for_parent(&model.activity, parent_id, recursion_depth, &property_names),
        )
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        let model = self.state.lock().expect("tray model lock poisoned");
        let items = menu_items(&model.activity);
        let wanted_all = ids.is_empty();
        items
            .into_iter()
            .filter(|item| wanted_all || ids.contains(&item.id))
            .map(|item| {
                (
                    item.id,
                    select_properties(&item.properties, &property_names),
                )
            })
            .collect()
    }

    fn get_property(&self, id: i32, name: String) -> OwnedValue {
        let model = self.state.lock().expect("tray model lock poisoned");
        let items = menu_items(&model.activity);
        items
            .into_iter()
            .find(|item| item.id == id)
            .and_then(|item| item.properties.get(&name).map(clone_value))
            .unwrap_or_else(|| string_value(""))
    }

    fn event(&self, _id: i32, _event_id: &str, _data: OwnedValue, _timestamp: u32) {}

    fn event_group(&self, _events: Vec<(i32, String, OwnedValue, u32)>) -> Vec<i32> {
        Vec::new()
    }

    fn about_to_show(&self, _id: i32) -> bool {
        false
    }

    fn about_to_show_group(&self, _ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (Vec::new(), Vec::new())
    }

    #[zbus(signal)]
    async fn layout_updated(
        ctxt: &SignalContext<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}

struct MenuItem {
    id: i32,
    properties: HashMap<String, OwnedValue>,
    children: Vec<MenuItem>,
}

fn tooltip_text(activity: &RuntimeActivity) -> String {
    let lines = activity_tooltip_lines(activity);
    if lines.is_empty() {
        "No active timers or reminders".to_string()
    } else {
        lines.join("\n")
    }
}

fn layout_for_parent(
    activity: &RuntimeActivity,
    parent_id: i32,
    recursion_depth: i32,
    property_names: &[String],
) -> Layout {
    let root = MenuItem {
        id: 0,
        properties: root_properties(),
        children: menu_items(activity),
    };
    build_layout(
        find_menu_item(&root, parent_id).unwrap_or(&root),
        recursion_depth,
        property_names,
    )
}

fn build_layout(item: &MenuItem, recursion_depth: i32, property_names: &[String]) -> Layout {
    let next_depth = if recursion_depth < 0 {
        -1
    } else {
        recursion_depth.saturating_sub(1)
    };
    let children = if recursion_depth == 0 {
        Vec::new()
    } else {
        item.children
            .iter()
            .map(|child| {
                let layout = build_layout(child, next_depth, property_names);
                OwnedValue::try_from(zbus::zvariant::Value::new(layout))
                    .expect("dbusmenu layout structure should serialize")
            })
            .collect()
    };

    (
        item.id,
        select_properties(&item.properties, property_names),
        children,
    )
}

fn find_menu_item(item: &MenuItem, id: i32) -> Option<&MenuItem> {
    if item.id == id {
        return Some(item);
    }
    item.children
        .iter()
        .find_map(|child| find_menu_item(child, id))
}

fn menu_items(activity: &RuntimeActivity) -> Vec<MenuItem> {
    let mut items = Vec::new();
    let mut next_id = 1;

    if !activity.timers.is_empty() {
        let mut timer_children = Vec::new();
        for timer in &activity.timers {
            let label = if timer.expired {
                format!("{}: expired", timer.title)
            } else {
                format!("{}: {} left", timer.title, remaining_string(timer.ends_at))
            };
            timer_children.push(leaf_item(next_id, &label));
            next_id += 1;
        }
        items.push(section_item(next_id, "Timers", timer_children));
        next_id += 1;
    }

    if !activity.tray_reminders.is_empty() {
        let mut reminder_children = Vec::new();
        for reminder in &activity.tray_reminders {
            reminder_children.push(leaf_item(next_id, &reminder_menu_label(reminder)));
            next_id += 1;
        }
        items.push(section_item(
            next_id,
            reminder_section_title(),
            reminder_children,
        ));
    }

    if items.is_empty() {
        items.push(leaf_item(next_id, "No active timers or reminders"));
    }

    items
}

fn section_item(id: i32, label: &str, children: Vec<MenuItem>) -> MenuItem {
    let mut properties = base_item_properties(label);
    properties.insert("children-display".to_string(), string_value("submenu"));
    MenuItem {
        id,
        properties,
        children,
    }
}

fn leaf_item(id: i32, label: &str) -> MenuItem {
    MenuItem {
        id,
        properties: base_item_properties(label),
        children: Vec::new(),
    }
}

fn root_properties() -> HashMap<String, OwnedValue> {
    HashMap::new()
}

fn base_item_properties(label: &str) -> HashMap<String, OwnedValue> {
    let mut properties = HashMap::new();
    properties.insert("label".to_string(), string_value(label));
    properties.insert("enabled".to_string(), OwnedValue::from(false));
    properties.insert("visible".to_string(), OwnedValue::from(true));
    properties
}

fn select_properties(
    properties: &HashMap<String, OwnedValue>,
    property_names: &[String],
) -> HashMap<String, OwnedValue> {
    if property_names.is_empty() {
        return clone_properties(properties);
    }
    properties
        .iter()
        .filter(|(name, _)| property_names.iter().any(|wanted| wanted == *name))
        .map(|(name, value)| (name.clone(), clone_value(value)))
        .collect()
}

fn clone_properties(properties: &HashMap<String, OwnedValue>) -> HashMap<String, OwnedValue> {
    properties
        .iter()
        .map(|(name, value)| (name.clone(), clone_value(value)))
        .collect()
}

fn clone_value(value: &OwnedValue) -> OwnedValue {
    value.try_clone().expect("owned D-Bus values should clone")
}

fn string_value(value: &str) -> OwnedValue {
    OwnedValue::try_from(zbus::zvariant::Value::new(value.to_string()))
        .expect("string value should serialize")
}

fn tray_status_name(state: TrayState) -> &'static str {
    match state {
        TrayState::Hidden => "Passive",
        TrayState::Active => "Active",
        TrayState::Alert => "NeedsAttention",
    }
}

fn stable_linux_icon_name() -> &'static str {
    super::TRAY_ICON_NAME
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Local, TimeZone};

    use super::{
        layout_for_parent, menu_items, stable_linux_icon_name, tooltip_text, tray_status_name,
    };
    use crate::activity::{
        ActiveTimer, ReminderTrayState, RuntimeActivity, TrayReminder, TrayState,
    };

    fn sample_activity() -> RuntimeActivity {
        let now = Local
            .with_ymd_and_hms(2026, 5, 2, 21, 15, 0)
            .single()
            .unwrap();
        RuntimeActivity {
            tray_state: TrayState::Active,
            timers: vec![
                ActiveTimer {
                    id: "tea".to_string(),
                    title: "Tea".to_string(),
                    ends_at: now + Duration::minutes(10),
                    expired: false,
                },
                ActiveTimer {
                    id: "bread".to_string(),
                    title: "Bread".to_string(),
                    ends_at: now - Duration::minutes(1),
                    expired: true,
                },
            ],
            tray_reminders: vec![TrayReminder {
                id: "stretch".to_string(),
                title: "Stretch".to_string(),
                state: ReminderTrayState::Scheduled,
                relevant_at: now + Duration::minutes(45),
                last_notified_at: None,
            }],
        }
    }

    #[test]
    fn tooltip_lists_active_timers_and_reminders() {
        let tooltip = tooltip_text(&sample_activity());

        assert!(tooltip.contains("Timer: Tea"));
        assert!(tooltip.contains("Timer: Bread (expired)"));
        assert!(tooltip.contains("Reminder: Stretch (next in"));
    }

    #[test]
    fn menu_groups_timers_and_reminders_into_sections() {
        let items = menu_items(&sample_activity());

        assert_eq!(items.len(), 2);
        let children_display: String = items[0]
            .properties
            .get("children-display")
            .expect("timer section submenu")
            .try_clone()
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(children_display, "submenu");
        assert_eq!(items[0].children.len(), 2);
        assert_eq!(items[1].children.len(), 1);
        let reminder_label: String = items[1]
            .properties
            .get("label")
            .expect("reminder section label")
            .try_clone()
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(reminder_label, "Reminders");
    }

    #[test]
    fn menu_falls_back_to_single_empty_row_when_idle() {
        let items = menu_items(&RuntimeActivity {
            tray_state: TrayState::Hidden,
            tray_reminders: Vec::new(),
            timers: Vec::new(),
        });

        assert_eq!(items.len(), 1);
        let label: String = items[0]
            .properties
            .get("label")
            .expect("idle label")
            .try_clone()
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(label, "No active timers or reminders");
    }

    #[test]
    fn root_layout_contains_section_children() {
        let (_, properties, children) = layout_for_parent(&sample_activity(), 0, 1, &[]);

        assert!(properties.is_empty());
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn root_layout_can_filter_properties() {
        let (_, _, children) = layout_for_parent(&sample_activity(), 0, 1, &["label".to_string()]);
        let first_child_layout: (
            i32,
            std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
            Vec<zbus::zvariant::OwnedValue>,
        ) = children[0].try_clone().unwrap().try_into().unwrap();

        assert_eq!(first_child_layout.1.len(), 1);
        assert!(first_child_layout.1.contains_key("label"));
    }

    #[test]
    fn status_and_icon_policy_are_stable() {
        assert_eq!(tray_status_name(TrayState::Hidden), "Passive");
        assert_eq!(tray_status_name(TrayState::Active), "Active");
        assert_eq!(tray_status_name(TrayState::Alert), "NeedsAttention");
        assert_eq!(stable_linux_icon_name(), crate::tray::TRAY_ICON_NAME);
    }
}
