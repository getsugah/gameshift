//! Compact diagnostic pages backed by one shared presentation model.
use crate::{
    diagnostic_context::LastReport,
    diagnostic_view::{Detail, Row, View},
};
use adw::prelude::*;
use gameshift_core::Snapshot;
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
fn add_page(stack: &gtk::Stack, content: &gtk::Box, name: &str, title: &str, icon: &str, groups: Option<&gtk::FlowBox>) {
    content.set_hexpand(true);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(content)
        .build();
    scroll.add_css_class("gs-unified-page");
    let adaptive=adw::BreakpointBin::new(); adaptive.set_size_request(320,280); adaptive.set_child(Some(&scroll));
    if let Some(groups)=groups {
        let wide=adw::Breakpoint::new(adw::BreakpointCondition::new_length(adw::BreakpointConditionLengthType::MinWidth,1500.,adw::LengthUnit::Px));
        wide.add_setter(groups,"max-children-per-line",Some(&2u32.to_value()));
        wide.add_setter(groups,"min-children-per-line",Some(&2u32.to_value()));
        adaptive.add_breakpoint(wide);
    }
    stack
        .add_titled(&adaptive, Some(name), title)
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
    help.set_child(Some(&gtk::Label::new(Some("?"))));
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
    stack.add_titled(&crate::overview::page(stack, &view, last), Some("overview"), "Accueil")
        .set_icon_name("view-grid-symbolic");
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
        let groups=gtk::FlowBox::new(); groups.set_selection_mode(gtk::SelectionMode::None); groups.set_homogeneous(true);
        groups.set_min_children_per_line(1); groups.set_max_children_per_line(1);
        groups.set_row_spacing(18); groups.set_column_spacing(18);
        legacy.append(&groups);
        for group in &detail.groups {
            let preferences = adw::PreferencesGroup::new();
            preferences.set_title(&group.title);
            if !group.explanation.is_empty() {
                preferences.set_description(Some(&group.explanation));
            }
            for row in &group.rows {
                preferences.add(&info_row(row));
            }
            preferences.set_valign(gtk::Align::Start);
            groups.insert(&preferences,-1);
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
        add_page(stack, &content, &detail.name, &detail.name, icon, if gpu {None} else {Some(&groups)});
    }
    if let Some(name) = selected {
        if stack.child_by_name(&name).is_some() {
            stack.set_visible_child_name(&name);
        }
    }
}
