//! The launching integration's menu: a pipeline (AYON, for one) that starts PhotoCraft can add a
//! menu of its own right after Window, given with `--host-menu <file>` (`PHOTOCRAFT_HOST_MENU`)
//! and read by the desktop app at launch. Its items run nothing inside PhotoCraft: a click queues
//! `{id, label}`, which the integration collects over the authenticated control channel with
//! `host.events` and acts on in its own process (open its Workfiles, Load or Publish tool).
//!
//! The definition is JSON: `{"title": "AYON", "items": [{"id": "workfiles", "label":
//! "Workfiles…"}, {"separator": true}, …]}`. It is checked when it is read; a bad one is
//! reported and no menu is added.

use std::collections::VecDeque;

use serde_json::{Value, json};

use crate::PhotocraftApp;
use crate::menus::{MenuItem, TOP_MENUS};

/// Menu ids of the host menu's items: `host.menu.<item id>`.
pub const PREFIX: &str = "host.menu.";
/// Most items (separators included) a host menu may have.
pub const MAX_ITEMS: usize = 64;
/// Longest menu title, in characters.
pub const MAX_TITLE: usize = 32;
/// Longest item label, in characters.
pub const MAX_LABEL: usize = 80;
/// Longest item id, in bytes (`[A-Za-z0-9._-]`).
pub const MAX_ID: usize = 64;
/// Clicks kept until the integration collects them; older ones are dropped first.
pub const MAX_EVENTS: usize = 64;

/// One row of the host menu: an item, or a separator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostMenuItem {
    Item { id: String, label: String },
    Separator,
}

/// A host menu and the clicks on it not collected yet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostMenu {
    pub title: String,
    pub items: Vec<HostMenuItem>,
    events: VecDeque<(String, String)>,
}

fn printable(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

fn item_id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

impl HostMenu {
    /// Parse and check a host menu definition (see the module docs). Errors name what is wrong.
    pub fn from_json(v: &Value) -> Result<Self, String> {
        let obj = v.as_object().ok_or("the host menu must be a JSON object")?;
        let title = obj.get("title").and_then(Value::as_str).map(str::trim).ok_or("the host menu needs a `title` string")?;
        if title.is_empty() || title.chars().count() > MAX_TITLE || !printable(title) {
            return Err(format!("the host menu `title` must be 1 to {MAX_TITLE} printable characters"));
        }
        if TOP_MENUS.iter().any(|t| t.eq_ignore_ascii_case(title)) {
            return Err(format!("the host menu `title` {title:?} is already a PhotoCraft menu"));
        }
        let list = obj.get("items").and_then(Value::as_array).ok_or("the host menu needs an `items` list")?;
        if list.is_empty() || list.len() > MAX_ITEMS {
            return Err(format!("the host menu needs 1 to {MAX_ITEMS} `items`"));
        }
        let mut items = Vec::with_capacity(list.len());
        for (k, entry) in list.iter().enumerate() {
            let e = entry.as_object().ok_or_else(|| format!("host menu item {k} must be an object"))?;
            if e.get("separator").and_then(Value::as_bool) == Some(true) {
                items.push(HostMenuItem::Separator);
                continue;
            }
            let id = e.get("id").and_then(Value::as_str).ok_or_else(|| format!("host menu item {k} needs an `id` string"))?;
            if !item_id_ok(id) {
                return Err(format!("host menu item {k}: `id` must be 1 to {MAX_ID} of A-Z a-z 0-9 . _ -"));
            }
            let label = e.get("label").and_then(Value::as_str).map(str::trim).ok_or_else(|| format!("host menu item {k} needs a `label` string"))?;
            if label.is_empty() || label.chars().count() > MAX_LABEL || !printable(label) || label == "---" {
                return Err(format!("host menu item {k}: `label` must be 1 to {MAX_LABEL} printable characters"));
            }
            if items.iter().any(|i| matches!(i, HostMenuItem::Item { id: other, .. } if other == id)) {
                return Err(format!("host menu item {k}: the id {id:?} is used twice"));
            }
            items.push(HostMenuItem::Item { id: id.to_string(), label: label.to_string() });
        }
        if !items.iter().any(|i| matches!(i, HostMenuItem::Item { .. })) {
            return Err("the host menu has only separators".into());
        }
        Ok(Self { title: title.to_string(), items, events: VecDeque::new() })
    }

    fn label_of(&self, id: &str) -> Option<&str> {
        self.items.iter().find_map(|i| match i {
            HostMenuItem::Item { id: own, label } if own == id => Some(label.as_str()),
            _ => None,
        })
    }

    /// Queue a click on item `id`, dropping the oldest click when the queue is full.
    fn push(&mut self, id: &str) -> Result<(), String> {
        let label = self.label_of(id).ok_or_else(|| format!("the {} menu has no item {id:?}", self.title))?.to_string();
        while self.events.len() >= MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back((id.to_string(), label));
        Ok(())
    }
}

/// The host menu's title, when the launch gave one.
pub fn title(app: &PhotocraftApp) -> Option<&str> {
    app.services.host_menu.as_ref().map(|m| m.title.as_str())
}

/// Whether `id` is one of the host menu's rows (shown as given, never translated).
pub fn owns(id: &str) -> bool {
    id.starts_with(PREFIX)
}

/// Appends the host menu's rows; the menu bar shows them under the host menu's title.
pub fn insert_menu_items(app: &PhotocraftApp, items: &mut Vec<MenuItem>) {
    let Some(menu) = app.services.host_menu.as_ref() else { return };
    let path = vec![menu.title.clone()];
    for item in &menu.items {
        let (id, label) = match item {
            HostMenuItem::Item { id, label } => (format!("{PREFIX}{id}"), label.clone()),
            HostMenuItem::Separator => ("---".to_string(), "---".to_string()),
        };
        let enabled = id != "---";
        items.push(MenuItem { id, label, path: path.clone(), shortcut: None, enabled, checked: None, color: None });
    }
}

/// Enabled state of a host menu item (`None` for other ids): always on, documents or not.
pub fn is_enabled(app: &PhotocraftApp, id: &str) -> Option<bool> {
    let item = id.strip_prefix(PREFIX)?;
    Some(app.services.host_menu.as_ref().is_some_and(|m| m.label_of(item).is_some()))
}

/// A click on a host menu item (`None` for other ids): queued for `host.events`.
pub fn menu(app: &mut PhotocraftApp, id: &str) -> Option<Result<Value, String>> {
    let item = id.strip_prefix(PREFIX)?;
    let Some(menu) = app.services.host_menu.as_mut() else { return Some(Err("PhotoCraft was started without a host menu".into())) };
    Some(menu.push(item).map(|()| json!({"queued": item})))
}

/// The `host.events` control method: the clicks since the last call, oldest first, as
/// `{events: [{id, label}]}`. The queue is emptied.
pub fn drain(app: &mut PhotocraftApp) -> Value {
    let events: Vec<Value> = app
        .services
        .host_menu
        .as_mut()
        .map(|m| m.events.drain(..).map(|(id, label)| json!({"id": id, "label": label})).collect())
        .unwrap_or_default();
    json!({"events": events})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ayon() -> HostMenu {
        HostMenu::from_json(&json!({
            "title": "AYON",
            "items": [
                {"id": "workfiles", "label": "Workfiles…"},
                {"id": "loader", "label": "Load…"},
                {"separator": true},
                {"id": "publisher", "label": "Publish…"},
            ]
        }))
        .unwrap()
    }

    fn app_with(menu: Option<HostMenu>) -> PhotocraftApp {
        let services = crate::Services { host_menu: menu, ..Default::default() };
        PhotocraftApp::new(photocraft_engine::Session::new(), services)
    }

    #[test]
    fn the_host_menu_sits_after_window_and_queues_clicks() {
        let mut app = app_with(Some(ayon()));
        let ctx = egui::Context::default();
        assert_eq!(crate::menus::top_menus(&app), ["File", "Edit", "Image", "Layer", "Type", "Select", "Filter", "View", "Window", "AYON", "Help"]);
        let items = crate::menus::menu_items(&app);
        let rows: Vec<&MenuItem> = items.iter().filter(|i| i.path == ["AYON"]).collect();
        assert_eq!(rows.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Workfiles…", "Load…", "---", "Publish…"]);
        // Usable with no document open, as the Workfiles tool must be.
        assert!(crate::menus::is_enabled(&app, "host.menu.workfiles"));
        crate::menus::invoke(&mut app, &ctx, "host.menu.loader", json!({})).unwrap();
        crate::menus::invoke(&mut app, &ctx, "host.menu.publisher", json!({})).unwrap();
        assert!(crate::menus::invoke(&mut app, &ctx, "host.menu.missing", json!({})).is_err());
        assert_eq!(drain(&mut app), json!({"events": [{"id": "loader", "label": "Load…"}, {"id": "publisher", "label": "Publish…"}]}));
        assert_eq!(drain(&mut app), json!({"events": []}), "collected clicks are gone");
    }

    #[test]
    fn without_a_host_menu_nothing_changes() {
        let mut app = app_with(None);
        let tops = crate::menus::top_menus(&app);
        assert_eq!(tops, TOP_MENUS);
        assert!(crate::menus::menu_items(&app).iter().all(|i| !owns(&i.id)));
        assert_eq!(drain(&mut app), json!({"events": []}));
        assert!(menu(&mut app, "host.menu.workfiles").is_some_and(|r| r.is_err()));
    }

    #[test]
    fn the_click_queue_is_bounded() {
        let mut m = ayon();
        for _ in 0..MAX_EVENTS + 10 {
            m.push("loader").unwrap();
        }
        assert_eq!(m.events.len(), MAX_EVENTS);
    }

    #[test]
    fn bad_definitions_are_refused() {
        let long = "x".repeat(MAX_LABEL + 1);
        let many: Vec<Value> = (0..=MAX_ITEMS).map(|k| json!({"id": format!("i{k}"), "label": "Item"})).collect();
        for bad in [
            json!(null),
            json!([]),
            json!({"items": [{"id": "a", "label": "A"}]}),
            json!({"title": "", "items": [{"id": "a", "label": "A"}]}),
            json!({"title": "Window", "items": [{"id": "a", "label": "A"}]}),
            json!({"title": "help", "items": [{"id": "a", "label": "A"}]}),
            json!({"title": "A\nB", "items": [{"id": "a", "label": "A"}]}),
            json!({"title": "AYON"}),
            json!({"title": "AYON", "items": []}),
            json!({"title": "AYON", "items": many}),
            json!({"title": "AYON", "items": [{"separator": true}]}),
            json!({"title": "AYON", "items": ["workfiles"]}),
            json!({"title": "AYON", "items": [{"label": "A"}]}),
            json!({"title": "AYON", "items": [{"id": "a b", "label": "A"}]}),
            json!({"title": "AYON", "items": [{"id": "a", "label": long}]}),
            json!({"title": "AYON", "items": [{"id": "a", "label": "---"}]}),
            json!({"title": "AYON", "items": [{"id": "a", "label": "A"}, {"id": "a", "label": "B"}]}),
        ] {
            assert!(HostMenu::from_json(&bad).is_err(), "{bad} must be refused");
        }
    }
}
