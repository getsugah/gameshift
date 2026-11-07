//! Compact diagnostic pages backed by one shared presentation model.
use crate::{
    diagnostic_context::LastReport,
    diagnostic_view::{Detail, Row, View},
};
use adw::prelude::*;
use gameshift_core::Snapshot;
use gtk::glib;
use std::collections::BTreeMap;
fn label(text: &str, class: &str) -> gtk::Label {
    let w = gtk::Label::new(Some(text));
    w.add_css_class(class);
    w.set_xalign(0.);
    w.set_wrap(true);
    w.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    w.set_selectable(true);
    w.set_hexpand(true);
    w
}
fn tile_label(text: &str, class: &str) -> gtk::Label {
    let label = label(text, class);
    label.set_selectable(false);
    label
}
fn page(title: &str, subtitle: &str) -> gtk::Box {
    let w = gtk::Box::new(gtk::Orientation::Vertical, 18);
    w.set_margin_top(20);
    w.set_margin_bottom(24);
    w.set_margin_start(20);
    w.set_margin_end(20);
    w.append(&label(title, "gs-page-title"));
    w.append(&label(subtitle, "gs-page-caption"));
    w
}
fn add_page(stack: &gtk::Stack, content: &gtk::Box, name: &str, title: &str, icon: &str) {
    let clamp = adw::Clamp::builder()
        .maximum_size(if name == "Carte graphique" {
            1800
        } else {
            1100
        })
        .tightening_threshold(if name == "Carte graphique" { 900 } else { 700 })
        .child(content)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp)
        .build();
    stack
        .add_titled(&scroll, Some(name), title)
        .set_icon_name(icon);
}
fn info_row(row: &Row) -> adw::ActionRow {
    let widget = adw::ActionRow::new();
    widget.set_use_markup(false);
    widget.set_title(&row.title);
    // Values are plain text, never markup from hardware or session data.
    widget.set_subtitle(&row.value);
    widget.set_subtitle_selectable(true);
    widget.set_subtitle_lines(0);
    widget.set_title_lines(0);
    widget.set_tooltip_text(Some(&format!("{}\n{}", row.help, row.sources)));
    let help = gtk::MenuButton::new();
    help.set_icon_name("help-about-symbolic");
    help.add_css_class("flat");
    help.set_valign(gtk::Align::Center);
    help.set_tooltip_text(Some(&format!("Comprendre : {}", row.title)));
    let explanation = format!(
        "{}\n\nSource : {}",
        row.help,
        if row.sources.is_empty() {
            "Non disponible"
        } else {
            &row.sources
        }
    );
    help.set_popover(Some(&crate::context_help::popup(&explanation)));
    widget.add_suffix(&help);
    widget
}
fn technical(detail: &Detail) -> gtk::Expander {
    let expander = gtk::Expander::new(Some("Informations techniques détaillées"));
    expander.add_css_class("gs-technical");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let mut groups = BTreeMap::<&str, Vec<_>>::new();
    for reading in &detail.technical {
        let (group, _) = reading
            .label
            .split_once(" — ")
            .unwrap_or(("Général", &reading.label));
        groups.entry(group).or_default().push(reading);
    }
    for (name, readings) in groups {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("boxed-list");
        for reading in readings {
            let title = reading
                .label
                .split_once(" — ")
                .map_or(reading.label.as_str(), |(_, s)| s);
            list.append(&info_row(&Row {
                title: title.into(),
                value: reading
                    .value
                    .clone()
                    .unwrap_or_else(|| "Indisponible".into()),
                help: reading.error.clone().unwrap_or_else(|| {
                    "Observation du diagnostic ; aucune modification appliquée.".into()
                }),
                sources: reading.source.clone(),
            }));
        }
        let group = gtk::Expander::new(Some(name));
        group.set_child(Some(&list));
        content.append(&group);
    }
    if detail.technical.is_empty() {
        content.append(&label("Toutes les informations collectées figurent dans les sections ci-dessus. Le bouton d’aide de chaque ligne donne sa source.","gs-card-detail"));
    }
    expander.set_child(Some(&content));
    expander
}
pub fn render(stack: &gtk::Stack, snapshot: &Snapshot, last: Option<&LastReport>) {
    let selected = stack.visible_child_name();
    while let Some(child) = stack.first_child() {
        stack.remove(&child);
    }
    let view = View::build(snapshot);
    let overview = page(
        "Votre machine",
        "Configuration matérielle et accès aux mesures.",
    );
    let grid = gtk::FlowBox::new();
    grid.add_css_class("gs-summary-grid");
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_min_children_per_line(1);
    grid.set_max_children_per_line(3);
    grid.set_row_spacing(8);
    grid.set_column_spacing(8);
    for tile in &view.tiles {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
        card.add_css_class("gs-summary-card");
        card.set_size_request(210, -1);
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let icon = gtk::Image::from_icon_name(tile.icon);
        icon.add_css_class("accent");
        top.append(&icon);
        top.append(&tile_label(&tile.title, "gs-card-label"));
        card.append(&top);
        card.append(&tile_label(&tile.value, "gs-summary-value"));
        card.append(&tile_label(&tile.detail, "gs-card-detail"));
        let button = gtk::Button::new();
        button.add_css_class("flat");
        button.set_child(Some(&card));
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "{} : {}",
            tile.title, tile.value
        ))]);
        button.set_tooltip_text(Some(&format!("Ouvrir : {}", tile.target)));
        let target = tile.target.clone();
        let weak = stack.downgrade();
        button.connect_clicked(move |_| {
            if let Some(stack) = weak.upgrade() {
                stack.set_visible_child_name(&target);
            }
        });
        grid.insert(&button, -1);
    }
    overview.append(&grid);
    overview.append(&crate::live_cpu::panel(stack, false));
    overview.append(&crate::live_gpu::panel(stack, false));
    let performances = gtk::Box::new(gtk::Orientation::Vertical, 10);
    performances.add_css_class("gs-info-panel");
    performances.append(&label("Performances", "gs-section-title"));
    performances.append(&label("Mesurez votre configuration dans le laboratoire. Les résultats CPU et GPU restent distincts.","gs-card-detail"));
    let open = gtk::Button::with_label("Ouvrir les benchmarks");
    open.add_css_class("suggested-action");
    open.set_halign(gtk::Align::Start);
    let weak = stack.downgrade();
    open.connect_clicked(move |_| {
        if let Some(stack) = weak.upgrade() {
            stack.set_visible_child_name("laboratory");
        }
    });
    performances.append(&open);
    if let Some(last) = last {
        performances.append(&label("Dernier benchmark graphique enregistré", "heading"));
        performances.append(&label(&last.description, "gs-card-detail"));
        let button = gtk::Button::with_label("Ouvrir l’export JSON");
        button.set_halign(gtk::Align::Start);
        let file = gtk::gio::File::for_path(&last.path);
        button.connect_clicked(move |_| {
            gtk::FileLauncher::new(Some(&file)).launch(
                None::<&gtk::Window>,
                None::<&gtk::gio::Cancellable>,
                |result| {
                    if let Err(error) = result {
                        tracing::warn!(%error,"Opening benchmark export failed");
                    }
                },
            )
        });
        performances.append(&button);
    } else {
        performances.append(&label(
            "Aucun export graphique complet et lisible trouvé. Aucun score n’est estimé.",
            "gs-card-detail",
        ));
    }
    overview.append(&performances);
    let stamp = glib::DateTime::from_unix_local(view.captured.min(i64::MAX as u64) as i64)
        .ok()
        .and_then(|d| d.format("%d/%m/%Y à %H:%M:%S").ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "date indisponible".into());
    overview.append(&label(
        &format!("Diagnostic du {stamp} · Instantané actualisé à la demande."),
        "gs-source",
    ));
    add_page(
        stack,
        &overview,
        "overview",
        "Accueil",
        "view-grid-symbolic",
    );
    for detail in &view.details {
        let gpu = detail.name == "Carte graphique";
        let content = if gpu {
            crate::gpu_page::page(stack, snapshot)
        } else {
            page(
                &detail.name,
                "Caractéristiques, état observé et explications.",
            )
        };
        if detail.name == "Processeur" {
            content.append(&crate::live_cpu::panel(stack, true));
        }
        let legacy = gtk::Box::new(gtk::Orientation::Vertical, 12);
        for group in &detail.groups {
            let preferences = adw::PreferencesGroup::new();
            preferences.set_title(&group.title);
            if !group.explanation.is_empty() {
                preferences.set_description(Some(&group.explanation));
            }
            for row in &group.rows {
                preferences.add(&info_row(row));
            }
            legacy.append(&preferences);
        }
        legacy.append(&technical(detail));
        if gpu {
            let details = gtk::Expander::new(Some("Configuration et diagnostic existants"));
            details.set_child(Some(&legacy));
            content.append(&details);
        } else {
            content.append(&legacy);
        }
        let icon = match detail.name.as_str() {
            "Système" => "computer-symbolic",
            "Processeur" => "preferences-system-symbolic",
            "Carte graphique" => "video-display-symbolic",
            "Mémoire" => "media-flash-symbolic",
            _ => "drive-harddisk-symbolic",
        };
        add_page(stack, &content, &detail.name, &detail.name, icon);
    }
    if let Some(name) = selected {
        if stack.child_by_name(&name).is_some() {
            stack.set_visible_child_name(&name);
        }
    }
}
