//! Global destinations above the workspace; contextual destinations on the left.
use adw::prelude::*;

pub fn top(stack: &gtk::Stack) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    bar.add_css_class("gs-workspace-topbar");
    let brand = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    brand.add_css_class("gs-workspace-brand");
    brand.set_width_request(230);
    let mark = crate::overview::icon("gameshift", 48);
    mark.add_css_class("gs-brand-mark");
    brand.append(&mark);
    let words = gtk::Box::new(gtk::Orientation::Vertical, 3);
    let title = gtk::Label::new(Some("GameShift"));
    title.add_css_class("gs-brand-name");
    title.set_xalign(0.);
    words.append(&title);
    let subtitle = gtk::Label::new(Some("Contrôle • Performances • Benchmarks"));
    subtitle.add_css_class("gs-brand-caption");
    subtitle.set_xalign(0.);
    words.append(&subtitle);
    brand.append(&words);
    bar.append(&brand);
    let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    navigation.set_homogeneous(true);
    navigation.set_halign(gtk::Align::Center);
    navigation.set_hexpand(true);
    let mut first = None;
    for (name, title, icon) in [
        ("overview", "Accueil", "view-grid-symbolic"),
        ("Carte graphique", "GPU", "video-display-symbolic"),
        ("Processeur", "CPU", "preferences-system-symbolic"),
        ("laboratory", "Benchmarks", "applications-science-symbolic"),
        ("profiles", "Profils", "applications-games-symbolic"),
        ("settings", "Réglages", "preferences-system-symbolic"),
    ] {
        let button = gtk::ToggleButton::new();
        button.add_css_class("gs-workspace-tab");
        button.set_group(first.as_ref());
        if first.is_none() {
            first = Some(button.clone());
            button.set_active(true);
        }
        let content = gtk::Box::new(gtk::Orientation::Vertical, 5);
        content.set_halign(gtk::Align::Center);
        content.append(&crate::overview::icon(icon, 27));
        content.append(&gtk::Label::new(Some(title)));
        button.set_child(Some(&content));
        let weak = stack.downgrade();
        button.connect_clicked(move |_| {
            if let Some(stack) = weak.upgrade() {
                stack.set_visible_child_name(name);
            }
        });
        let weak = button.downgrade();
        stack.connect_visible_child_name_notify(move |stack| {
            if let Some(button) = weak.upgrade() {
                let selected = stack.visible_child_name();
                let selected = match selected.as_deref() {
                    Some("Système" | "Mémoire" | "Stockage") => Some("overview"),
                    other => other,
                };
                button.set_active(selected == Some(name));
            }
        });
        navigation.append(&button);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .child(&navigation)
        .hexpand(true)
        .build();
    bar.append(&scroll);
    bar
}
pub fn sidebar(stack: &gtk::Stack) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("gs-sidebar");
    root.add_css_class("gs-context-sidebar");
    root.set_width_request(230);
    root.set_hexpand(false);
    let pages = gtk::Stack::new();
    pages.set_vexpand(true);
    pages.set_hhomogeneous(false);
    pages.set_vhomogeneous(false);
    for (name, items) in [
        (
            "overview",
            vec![
                ("Tableau de bord", "overview", "view-grid-symbolic"),
                ("Système", "Système", "computer-symbolic"),
                ("Mémoire", "Mémoire", "media-flash-symbolic"),
                ("Stockage", "Stockage", "drive-harddisk-symbolic"),
            ],
        ),
        (
            "Carte graphique",
            vec![(
                "Carte graphique",
                "Carte graphique",
                "video-display-symbolic",
            )],
        ),
        (
            "Processeur",
            vec![("Processeur", "Processeur", "preferences-system-symbolic")],
        ),
        (
            "laboratory",
            vec![
                ("Aurelia", "summary", "applications-science-symbolic"),
                (
                    "Résultats & Historique",
                    "history",
                    "document-open-recent-symbolic",
                ),
                ("Comparaison", "comparison", "view-grid-symbolic"),
                ("Protocoles", "protocol", "emblem-system-symbolic"),
                (
                    "Configuration",
                    "configuration",
                    "dialog-information-symbolic",
                ),
            ],
        ),
        (
            "profiles",
            vec![("Profils GPU", "profiles", "applications-games-symbolic")],
        ),
        (
            "settings",
            vec![("Réglages", "settings", "preferences-system-symbolic")],
        ),
    ] {
        let menu = gtk::Box::new(gtk::Orientation::Vertical, 6);
        menu.set_margin_top(16);
        let mut first = None;
        let context_buttons =
            std::rc::Rc::new(std::cell::RefCell::new(std::collections::BTreeMap::new()));
        for (title, target, icon) in items {
            let button = gtk::ToggleButton::new();
            button.set_group(first.as_ref());
            if first.is_none() {
                first = Some(button.clone());
                button.set_active(true);
            }
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            row.append(&crate::overview::icon(icon, 24));
            let text = gtk::Label::new(Some(title));
            text.set_xalign(0.);
            text.set_ellipsize(gtk::pango::EllipsizeMode::End);
            row.append(&text);
            button.set_child(Some(&row));
            button.set_tooltip_text(Some(title));
            let weak = stack.downgrade();
            button.connect_clicked(move |_| {
                if let Some(stack) = weak.upgrade() {
                    if name == "laboratory" {
                        stack.set_visible_child_name("laboratory");
                        if let Some(page) = stack.child_by_name("laboratory") {
                            crate::benchmark::navigate(&page, target);
                        }
                    } else {
                        stack.set_visible_child_name(target);
                    }
                }
            });
            context_buttons.borrow_mut().insert(target, button.clone());
            menu.append(&button);
        }
        if name == "laboratory" {
            let connected = std::cell::Cell::new(false);
            stack.connect_visible_child_name_notify(move |stack| {
                if connected.get() {
                    return;
                }
                if let Some(inner) = stack
                    .child_by_name("laboratory")
                    .and_then(|page| crate::benchmark::named_stack(&page, "aurelia-workspace"))
                {
                    connected.set(true);
                    let buttons = context_buttons.clone();
                    inner.connect_visible_child_name_notify(move |inner| {
                        let buttons = buttons.borrow();
                        let target = match inner.visible_child_name().as_deref() {
                            Some("results") => "summary",
                            Some("history") => "history",
                            Some("launch") => {
                                if buttons.get("configuration").is_some_and(|b| b.is_active()) {
                                    return;
                                }
                                "protocol"
                            }
                            _ => return,
                        };
                        if let Some(button) = buttons.get(target) {
                            button.set_active(true);
                        }
                    });
                }
            });
        }
        pages.add_named(&menu, Some(name));
    }
    let weak = pages.downgrade();
    stack.connect_visible_child_name_notify(move |stack| {
        if let Some(pages) = weak.upgrade() {
            let selected = stack.visible_child_name();
            let selected = match selected.as_deref() {
                Some("Système" | "Mémoire" | "Stockage") => "overview",
                Some(name) => name,
                None => "overview",
            };
            pages.set_visible_child_name(selected);
        }
    });
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&decoration()));
    overlay.add_overlay(&pages);
    overlay.set_measure_overlay(&pages, true);
    overlay.set_vexpand(true);
    root.append(&overlay);
    let version = gtk::Label::new(Some(&format!(
        "GameShift v{}\nAurelia",
        env!("CARGO_PKG_VERSION")
    )));
    version.add_css_class("gs-context-version");
    version.set_margin_bottom(22);
    root.append(&version);
    root
}
fn decoration() -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_vexpand(true);
    area.set_hexpand(true);
    area.set_draw_func(|_, c, width, height| {
        let w = width as f64;
        let h = height as f64;
        c.set_source_rgb(0.038, 0.048, 0.063);
        let _ = c.paint();
        let halo =
            gtk::cairo::RadialGradient::new(w * 0.39, h * 0.65, 0., w * 0.39, h * 0.65, w * 0.8);
        halo.add_color_stop_rgba(0., 1., 0.43, 0.01, 0.20);
        halo.add_color_stop_rgba(0.45, 0.75, 0.18, 0.01, 0.08);
        halo.add_color_stop_rgba(1., 0., 0., 0., 0.);
        let _ = c.set_source(&halo);
        let _ = c.paint();
        for family in 0..2 {
            for i in 0..37 {
                let offset = (i as f64 - 18.) * 2.25;
                let path = |c: &gtk::cairo::Context| {
                    c.new_path();
                    if family == 0 {
                        c.move_to(-0.2 * w, 0.83 * h + offset);
                        c.curve_to(
                            0.30 * w,
                            0.83 * h + offset,
                            0.53 * w,
                            0.43 * h + offset,
                            1.2 * w,
                            0.40 * h + offset,
                        );
                    } else {
                        c.move_to(-0.2 * w, 0.50 * h + offset);
                        c.curve_to(
                            0.30 * w,
                            0.50 * h + offset,
                            0.52 * w,
                            0.81 * h + offset,
                            1.2 * w,
                            0.88 * h + offset,
                        );
                    }
                };
                let strength = 1. - ((i as f64 - 18.) / 21.).abs();
                let gradient = gtk::cairo::LinearGradient::new(0., 0., w, 0.);
                gradient.add_color_stop_rgba(0., 1., 0.29, 0.0, 0.18 + strength * 0.15);
                gradient.add_color_stop_rgba(0.4, 1., 0.72, 0.04, 0.45 + strength * 0.45);
                gradient.add_color_stop_rgba(0.68, 1., 0.43, 0., 0.4 + strength * 0.35);
                gradient.add_color_stop_rgba(1., 0.95, 0.26, 0., 0.2 + strength * 0.25);
                let _ = c.set_source(&gradient);
                path(c);
                c.set_line_width(0.85);
                let _ = c.stroke();
                if i == 18 {
                    path(c);
                    c.set_source_rgba(1., 0.72, 0.08, 0.10);
                    c.set_line_width(12.);
                    let _ = c.stroke();
                    path(c);
                    c.set_source_rgb(1., 0.88, 0.32);
                    c.set_line_width(1.8);
                    let _ = c.stroke();
                }
            }
        }
    });
    area
}
