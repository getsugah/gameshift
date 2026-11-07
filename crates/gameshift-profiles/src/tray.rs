//! Native StatusNotifierItem + DBusMenu. No GTK, AppIndicator library or extension installation.
use crate::{agent::Status, *};
use gio::{
    glib::{
        self,
        variant::{ObjectPath, ToVariant},
        Variant,
    },
    DBusCallFlags,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
const ITEM: &str = "org.kde.StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";
const XML: &str = r#"<node>
<interface name="org.kde.StatusNotifierItem">
<method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="SecondaryActivate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="Scroll"><arg type="i" direction="in"/><arg type="s" direction="in"/></method>
<property name="Category" type="s" access="read"/><property name="Id" type="s" access="read"/>
<property name="Title" type="s" access="read"/><property name="Status" type="s" access="read"/>
<property name="WindowId" type="i" access="read"/><property name="IconName" type="s" access="read"/>
<property name="IconPixmap" type="a(iiay)" access="read"/><property name="OverlayIconName" type="s" access="read"/>
<property name="OverlayIconPixmap" type="a(iiay)" access="read"/><property name="AttentionIconName" type="s" access="read"/>
<property name="AttentionIconPixmap" type="a(iiay)" access="read"/><property name="AttentionMovieName" type="s" access="read"/>
<property name="ToolTip" type="(sa(iiay)ss)" access="read"/><property name="ItemIsMenu" type="b" access="read"/>
<property name="Menu" type="o" access="read"/><property name="IconThemePath" type="s" access="read"/>
<signal name="NewTitle"/><signal name="NewIcon"/><signal name="NewStatus"><arg type="s"/></signal><signal name="NewToolTip"/>
</interface>
<interface name="com.canonical.dbusmenu">
<method name="GetLayout"><arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/><arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/></method>
<method name="GetGroupProperties"><arg type="ai" direction="in"/><arg type="as" direction="in"/><arg type="a(ia{sv})" direction="out"/></method>
<method name="GetProperty"><arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="out"/></method>
<method name="Event"><arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/><arg type="u" direction="in"/></method>
<method name="EventGroup"><arg type="a(isvu)" direction="in"/><arg type="ai" direction="out"/></method>
<method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
<method name="AboutToShowGroup"><arg type="ai" direction="in"/><arg type="ai" direction="out"/><arg type="ai" direction="out"/></method>
<property name="Version" type="u" access="read"/><property name="TextDirection" type="s" access="read"/><property name="Status" type="s" access="read"/><property name="IconThemePath" type="as" access="read"/>
<signal name="LayoutUpdated"><arg type="u"/><arg type="i"/></signal>
</interface></node>"#;
type Props = BTreeMap<String, Variant>;
type Layout = (i32, Props, Vec<Variant>);
fn props(label: &str, enabled: bool, checked: Option<bool>) -> Props {
    let mut p = Props::from([
        ("label".into(), label.to_variant()),
        ("enabled".into(), enabled.to_variant()),
        ("visible".into(), true.to_variant()),
    ]);
    if let Some(checked) = checked {
        p.insert("toggle-type".into(), "radio".to_variant());
        p.insert("toggle-state".into(), i32::from(checked).to_variant());
    }
    p
}
fn title(s: &Status) -> String {
    let active = s
        .active
        .as_ref()
        .and_then(|id| s.config.profiles.iter().find(|p| &p.id == id))
        .map(|p| p.name.as_str())
        .unwrap_or(if s.error.is_some() {
            "État à vérifier"
        } else {
            "Pilote automatique"
        });
    format!(
        "GameShift · {active}{}",
        if s.error.is_some() { " · erreur" } else { "" }
    )
}
fn entries(s: &Status) -> Vec<(i32, Props)> {
    let mut out = vec![
        (1, props(&title(s), false, None)),
        (
            2,
            props(
                "Automatique",
                true,
                Some(matches!(s.config.mode, Mode::Automatic {})),
            ),
        ),
    ];
    for (i, p) in s.config.profiles.iter().enumerate() {
        out.push((
            100 + i as i32,
            props(
                &p.name,
                true,
                Some(matches!(&s.config.mode,Mode::Manual{profile} if profile==&p.id)),
            ),
        ));
    }
    out.extend([
        (3, props("Ouvrir GameShift", true, None)),
        (4, props("Quitter l’interface", true, None)),
        (5, props("Quitter GameShift", true, None)),
    ]);
    out
}
fn layout(s: &Status, parent: i32) -> Layout {
    if parent != 0 {
        let property = entries(s)
            .into_iter()
            .find(|(id, _)| *id == parent)
            .map(|(_, p)| p)
            .unwrap_or_default();
        return (parent, property, vec![]);
    }
    (
        0,
        Props::from([("children-display".into(), "submenu".to_variant())]),
        entries(s)
            .into_iter()
            .map(|(id, p)| (id, p, Vec::<Variant>::new()).to_variant())
            .collect(),
    )
}
fn pixmap() -> Vec<(i32, i32, Vec<u8>)> {
    let mut bytes = Vec::new();
    for y in 0..32 {
        for x in 0..32 {
            let g = (6..26).contains(&y)
                && (6..26).contains(&x)
                && (!(10..22).contains(&x) || !(10..22).contains(&y) || (y >= 16 && x >= 17));
            let (r, g, b) = if g { (66, 223, 178) } else { (20, 23, 33) };
            bytes.extend_from_slice(&[255, r, g, b]);
        }
    }
    vec![(32, 32, bytes)]
}
fn gui(state: &Status, quit: bool) -> Result<()> {
    let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
        .map_err(|e| e.to_string())?;
    if quit {
        bus.call_sync(
            Some("io.github.gameshift.GameShift"),
            "/io/github/gameshift/GameShift",
            "org.gtk.Actions",
            "Activate",
            Some(&("quit-interface", Vec::<Variant>::new(), Props::new()).to_variant()),
            None,
            DBusCallFlags::NONE,
            2000,
            None::<&gio::Cancellable>,
        )
        .map_err(|e| e.to_string())?;
    } else {
        let result = bus.call_sync(
            Some("io.github.gameshift.GameShift"),
            "/io/github/gameshift/GameShift",
            "org.freedesktop.Application",
            "Activate",
            Some(&(Props::new(),).to_variant()),
            None,
            DBusCallFlags::NONE,
            1000,
            None::<&gio::Cancellable>,
        );
        if result.is_err() {
            let path =
                state.config.preferences.gui_launcher.as_deref().ok_or(
                    "Ouvrir une première fois l’AppImage pour enregistrer son emplacement",
                )?;
            std::process::Command::new(path)
                .env_remove("LD_LIBRARY_PATH")
                .env_remove("LD_PRELOAD")
                .env_remove("LD_AUDIT")
                .spawn()
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn event(state: &Arc<Mutex<Status>>, id: i32) {
    let snapshot = state.lock().unwrap().clone();
    let command = match id {
        2 => Some(json!({"command":"automatic"})),
        3 => {
            let _ = gui(&snapshot, false);
            None
        }
        4 => {
            let _ = gui(&snapshot, true);
            Some(json!({"command":"tray","visible":false}))
        }
        5 => {
            // The interface may already be closed; always stop the agent too.
            let _ = gui(&snapshot, true);
            Some(json!({"command":"shutdown"}))
        }
        id if id >= 100 => snapshot
            .config
            .profiles
            .get((id - 100) as usize)
            .map(|p| json!({"command":"manual","profile":p.id})),
        _ => None,
    };
    if let Some(command) = command {
        std::thread::spawn(move || {
            if let Err(e) = call(command) {
                eprintln!("Menu des profils : {e}");
            }
        });
    }
}
pub fn run(state: Arc<Mutex<Status>>) -> Result<()> {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| -> Result<()> {
            let connection = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
                .map_err(|e| e.to_string())?;
            let node = gio::DBusNodeInfo::for_xml(XML).map_err(|e| e.to_string())?;
            let item_state = state.clone();
            let prop_state = state.clone();
            let item = connection
                .register_object(
                    "/StatusNotifierItem",
                    &node.lookup_interface(ITEM).ok_or("Interface SNI absente")?,
                )
                .method_call(move |_, _, _, _, method, _, invocation| {
                    if matches!(method, "Activate" | "SecondaryActivate") {
                        let s = item_state.lock().unwrap().clone();
                        let _ = gui(&s, false);
                    }
                    invocation.return_value(Some(&().to_variant()));
                })
                .property(move |_, _, _, _, property| {
                    let s = prop_state.lock().unwrap();
                    match property {
                        "Category" => "Hardware".to_variant(),
                        "Id" => "gameshift".to_variant(),
                        "Title" => title(&s).to_variant(),
                        "Status" => "Active".to_variant(),
                        "WindowId" => 0i32.to_variant(),
                        "IconName" => "".to_variant(),
                        "IconPixmap" => pixmap().to_variant(),
                        "OverlayIconPixmap" | "AttentionIconPixmap" => {
                            Vec::<(i32, i32, Vec<u8>)>::new().to_variant()
                        }
                        "ItemIsMenu" => false.to_variant(),
                        "Menu" => ObjectPath::try_from("/MenuBar").unwrap().to_variant(),
                        "ToolTip" => (
                            "",
                            pixmap(),
                            title(&s),
                            s.error.as_deref().unwrap_or(&s.reason),
                        )
                            .to_variant(),
                        _ => "".to_variant(),
                    }
                })
                .build()
                .map_err(|e| e.to_string())?;
            let menu_state = state.clone();
            let menu_revision = std::rc::Rc::new(std::cell::Cell::new(1u32));
            let read_revision = menu_revision.clone();
            let menu = connection
                .register_object(
                    "/MenuBar",
                    &node
                        .lookup_interface(MENU)
                        .ok_or("Interface menu absente")?,
                )
                .method_call(move |_, _, _, _, method, parameters, invocation| {
                    let s = menu_state.lock().unwrap().clone();
                    let result = match method {
                        "GetLayout" => {
                            let parent = parameters.child_value(0).get::<i32>().unwrap_or(0);
                            (read_revision.get(), layout(&s, parent)).to_variant()
                        }
                        "GetGroupProperties" => {
                            let ids = parameters
                                .child_value(0)
                                .get::<Vec<i32>>()
                                .unwrap_or_default();
                            let data = entries(&s)
                                .into_iter()
                                .filter(|(id, _)| ids.is_empty() || ids.contains(id))
                                .collect::<Vec<_>>();
                            (data,).to_variant()
                        }
                        "GetProperty" => {
                            let id = parameters.child_value(0).get::<i32>().unwrap_or(0);
                            let name = parameters
                                .child_value(1)
                                .get::<String>()
                                .unwrap_or_default();
                            let p = entries(&s)
                                .into_iter()
                                .find(|(i, _)| *i == id)
                                .and_then(|(_, p)| p.get(&name).cloned())
                                .unwrap_or_else(|| "".to_variant());
                            (p,).to_variant()
                        }
                        "Event" => {
                            if let Some((id, kind, _, _)) =
                                parameters.get::<(i32, String, Variant, u32)>()
                            {
                                if kind == "clicked" {
                                    event(&menu_state, id);
                                }
                            }
                            ().to_variant()
                        }
                        "EventGroup" => {
                            if let Some((events,)) =
                                parameters.get::<(Vec<(i32, String, Variant, u32)>,)>()
                            {
                                for (id, kind, _, _) in events {
                                    if kind == "clicked" {
                                        event(&menu_state, id);
                                    }
                                }
                            }
                            (Vec::<i32>::new(),).to_variant()
                        }
                        "AboutToShow" => (false,).to_variant(),
                        "AboutToShowGroup" => (Vec::<i32>::new(), Vec::<i32>::new()).to_variant(),
                        _ => ().to_variant(),
                    };
                    invocation.return_value(Some(&result));
                })
                .property(|_, _, _, _, property| match property {
                    "Version" => 3u32.to_variant(),
                    "TextDirection" => "ltr".to_variant(),
                    "IconThemePath" => Vec::<String>::new().to_variant(),
                    _ => "normal".to_variant(),
                })
                .build()
                .map_err(|e| e.to_string())?;
            let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
            let mut owned = false;
            let mut watcher = None::<String>;
            let mut signature = String::new();
            let mut next = std::time::Instant::now();
            loop {
                while context.pending() {
                    context.iteration(false);
                }
                if next.elapsed() >= Duration::from_secs(1) {
                    next = std::time::Instant::now();
                    let snapshot = state.lock().unwrap().clone();
                    if snapshot.tray_visible && !owned {
                        connection
                            .call_sync(
                                Some("org.freedesktop.DBus"),
                                "/org/freedesktop/DBus",
                                "org.freedesktop.DBus",
                                "RequestName",
                                Some(&(name.as_str(), 0u32).to_variant()),
                                None,
                                DBusCallFlags::NONE,
                                1000,
                                None::<&gio::Cancellable>,
                            )
                            .map_err(|e| e.to_string())?;
                        owned = true;
                    } else if !snapshot.tray_visible && owned {
                        let _ = connection.call_sync(
                            Some("org.freedesktop.DBus"),
                            "/org/freedesktop/DBus",
                            "org.freedesktop.DBus",
                            "ReleaseName",
                            Some(&(name.as_str(),).to_variant()),
                            None,
                            DBusCallFlags::NONE,
                            1000,
                            None::<&gio::Cancellable>,
                        );
                        owned = false;
                        watcher = None;
                        state.lock().unwrap().tray_available = false;
                    }
                    if owned {
                        let actual = connection
                            .call_sync(
                                Some("org.freedesktop.DBus"),
                                "/org/freedesktop/DBus",
                                "org.freedesktop.DBus",
                                "GetNameOwner",
                                Some(&("org.kde.StatusNotifierWatcher",).to_variant()),
                                None,
                                DBusCallFlags::NONE,
                                1000,
                                None::<&gio::Cancellable>,
                            )
                            .ok()
                            .and_then(|v| v.get::<(String,)>().map(|v| v.0));
                        if actual != watcher {
                            watcher = None;
                            if actual.is_some()
                                && connection
                                    .call_sync(
                                        Some("org.kde.StatusNotifierWatcher"),
                                        "/StatusNotifierWatcher",
                                        "org.kde.StatusNotifierWatcher",
                                        "RegisterStatusNotifierItem",
                                        Some(&(name.as_str(),).to_variant()),
                                        None,
                                        DBusCallFlags::NONE,
                                        2000,
                                        None::<&gio::Cancellable>,
                                    )
                                    .is_ok()
                            {
                                watcher = actual;
                            }
                        }
                        state.lock().unwrap().tray_available = watcher.is_some();
                        let current = format!(
                            "{}:{}:{:?}",
                            snapshot.revision,
                            title(&snapshot),
                            snapshot.config.mode
                        );
                        if current != signature {
                            signature = current;
                            menu_revision.set(menu_revision.get().wrapping_add(1).max(1));
                            let _ = connection.emit_signal(
                                None,
                                "/MenuBar",
                                MENU,
                                "LayoutUpdated",
                                Some(&(menu_revision.get(), 0i32).to_variant()),
                            );
                            for signal in ["NewTitle", "NewToolTip"] {
                                let _ = connection.emit_signal(
                                    None,
                                    "/StatusNotifierItem",
                                    ITEM,
                                    signal,
                                    Some(&().to_variant()),
                                );
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
                if connection.is_closed() {
                    let _ = connection.unregister_object(item);
                    let _ = connection.unregister_object(menu);
                    return Err("Bus de session fermé".into());
                }
            }
        })
        .map_err(|e| e.to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_and_icon_have_exact_dbus_types() {
        let s = Status {
            config: Config::default(),
            revision: 0,
            active: None,
            reason: String::new(),
            error: None,
            tray_visible: true,
            tray_available: false,
            pending: false,
        };
        assert_eq!(layout(&s, 0).to_variant().type_().as_str(), "(ia{sv}av)");
        assert_eq!(
            (0u32, layout(&s, 0)).to_variant().type_().as_str(),
            "(u(ia{sv}av))"
        );
        assert_eq!(pixmap().to_variant().type_().as_str(), "a(iiay)");
        assert_eq!(pixmap()[0].2.len(), 32 * 32 * 4);
        gio::DBusNodeInfo::for_xml(XML).unwrap();
    }
}
