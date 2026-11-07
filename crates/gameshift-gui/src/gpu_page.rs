//! First GPU design prototype: typed telemetry, a bounded chart and an opt-in safe driver lab.
use adw::prelude::*;
use gameshift_core::Snapshot;
use gameshift_telemetry::{
    capabilities::{Category, Inventory, ReadPolicy, ReadState, WriteState},
    device::{DeviceSensors, Measurement, Metric},
};
use gtk::glib;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

thread_local! {static CONTROL_SPACE:std::cell::RefCell<Option<gtk::glib::WeakRef<gtk::Stack>>>=const{std::cell::RefCell::new(None)};}
pub fn show_controls() {
    CONTROL_SPACE.with_borrow(|w| {
        if let Some(stack) = w.as_ref().and_then(|w| w.upgrade()) {
            stack.set_visible_child_name("controls");
        }
    });
}

fn text(value: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(value));
    label.set_xalign(0.);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::Word);
    label.add_css_class(class);
    label
}
fn metric(values: &[Measurement], kind: Metric) -> Option<&Measurement> {
    values.iter().find(|m| m.metric == kind)
}
fn temperature<'a>(values: &'a [Measurement], names: &[&str]) -> Option<&'a Measurement> {
    values.iter().find(|m| {
        m.metric == Metric::Temperature
            && names.iter().any(|name| m.label.eq_ignore_ascii_case(name))
    })
}
fn value(measurement: Option<&Measurement>, unit: &str, divisor: f64) -> String {
    measurement
        .and_then(|m| m.value)
        .map(|v| format!("{:.1} {unit}", v / divisor))
        .unwrap_or_else(|| "Indisponible".into())
}
fn provenance(measurement: Option<&Measurement>) -> String {
    measurement
        .map(|m| {
            format!(
                "{} · {} · Unix {} ms{}",
                m.label,
                m.source,
                m.captured_unix_millis,
                m.error
                    .as_ref()
                    .map(|e| format!(" · {e}"))
                    .unwrap_or_default()
            )
        })
        .unwrap_or_else(|| {
            "Ce capteur n’est pas identifié sur ce périphérique ; aucune valeur estimée.".into()
        })
}
struct Tile {
    value: gtk::Label,
    detail: gtk::Label,
    root: gtk::Box,
    gauge: gtk::LevelBar,
}
fn tile(title: &str, icon: &str, grid: &gtk::FlowBox) -> Tile {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
    root.add_css_class("gs-gpu-tile");
    root.set_size_request(155, -1);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let image = gtk::Image::from_icon_name(icon);
    image.add_css_class("gs-gpu-signal");
    header.append(&image);
    header.append(&text(title, "gs-gpu-kicker"));
    root.append(&header);
    let value = text("Indisponible", "gs-gpu-number");
    root.append(&value);
    let gauge = gtk::LevelBar::for_interval(0., 100.);
    gauge.set_visible(false);
    root.append(&gauge);
    let detail = text("En attente du capteur", "gs-gpu-caption");
    root.append(&detail);
    grid.insert(&root, -1);
    Tile {
        value,
        detail,
        root,
        gauge,
    }
}
impl Tile {
    fn update(&self, measurement: Option<&Measurement>, unit: &str, divisor: f64) {
        self.value.set_text(&value(measurement, unit, divisor));
        self.root.set_tooltip_text(Some(&provenance(measurement)));
        self.detail.set_text(
            measurement
                .map(|m| {
                    if m.error.is_some() {
                        "Lecture indisponible"
                    } else {
                        m.label.as_str()
                    }
                })
                .unwrap_or("Capteur non exposé"),
        );
    }
}
struct Frame {
    pci: String,
    values: Vec<Measurement>,
    errors: Vec<String>,
}
fn collect() -> Result<Vec<Frame>, String> {
    let mut frames = Vec::new();
    let mut errors = Vec::new();
    for device in DeviceSensors::discover_all()? {
        match device {
            Ok(device) => frames.push(Frame {
                pci: device.pci_address.clone(),
                values: device.read(),
                errors: device.discovery_errors,
            }),
            Err(error) => errors.push(error),
        }
    }
    if frames.is_empty() && !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    if let Some(frame) = frames.first_mut() {
        frame.errors.extend(errors);
    }
    Ok(frames)
}
fn functional_group(cap: &gameshift_telemetry::capabilities::Capability) -> &'static str {
    let name = cap.id.rsplit('/').next().unwrap_or("");
    if cap.id.contains("fan_ctrl")
        || name.starts_with("temp")
        || name.starts_with("fan")
        || name.starts_with("pwm")
    {
        "Refroidissement"
    } else if name.starts_with("power")
        || name == "pp_power_profile_mode"
        || name.starts_with("pm_policy")
    {
        "Énergie et performances"
    } else if name.starts_with("freq")
        || name.starts_with("pp_dpm_")
        || name == "pp_od_clk_voltage"
        || name.starts_with("in0_")
    {
        "Fréquences et tensions"
    } else if name.starts_with("mem_info") || name.contains("pcie") || name.contains("link_") {
        "Mémoire et PCIe"
    } else if name == "pp_features" {
        "Fonctions avancées"
    } else if cap.read_policy == ReadPolicy::PassiveText {
        "Diagnostic matériel"
    } else {
        "Interfaces inconnues"
    }
}
fn function_key(id: &str) -> String {
    if id.starts_with("hwmon/") {
        if let Some((parent, name)) = id.rsplit_once('/') {
            return format!("{parent}/{}", name.split('_').next().unwrap_or(name));
        }
    }
    id.into()
}
fn capability_name(id: &str) -> String {
    let name = id.rsplit('/').next().unwrap_or(id);
    if let Some(setting) = gameshift_control::FanSetting::ALL
        .into_iter()
        .find(|s| s.definition().0 == name)
    {
        return setting.definition().3.into();
    }
    match name {
        "fan_curve" => "Courbe de ventilation PMFW".into(),
        "power_dpm_force_performance_level" => "Mode de performance DPM".into(),
        "pp_power_profile_mode" => "Profil de charge énergétique".into(),
        "pp_od_clk_voltage" => "Fréquences et tension OverDrive".into(),
        "gpu_metrics" => "Mesures du firmware GPU".into(),
        "gpu_busy_percent" => "Occupation du GPU".into(),
        "mem_busy_percent" => "Occupation du contrôleur mémoire".into(),
        "pp_features" => "Fonctionnalités SMU".into(),
        _ => {
            if let Some(channel) = name.strip_prefix("temp") {
                format!("Température · canal {channel}")
            } else if let Some(channel) = name.strip_prefix("power") {
                format!("Puissance · canal {channel}")
            } else if let Some(channel) = name.strip_prefix("fan") {
                format!("Ventilateur · canal {channel}")
            } else {
                name.replace('_', " ")
            }
        }
    }
}
fn lab_contents(report: &Inventory, box_: &gtk::Box) {
    while let Some(child) = box_.first_child() {
        box_.remove(&child);
    }
    let stamp = report
        .captured_unix_millis
        .and_then(|ms| glib::DateTime::from_unix_local((ms / 1000) as i64).ok())
        .and_then(|d| d.format("%d/%m/%Y %H:%M:%S").ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "date indisponible".into());
    box_.append(&text(
        &format!("{} · amdgpu · inventaire du {stamp}", report.pci_address),
        "gs-gpu-caption",
    ));
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Rechercher une fonction du pilote"));
    box_.append(&search);
    let filter = gtk::DropDown::from_strings(&[
        "Toutes les capacités",
        "Modifiables",
        "Lecture seule",
        "Avancées",
        "Non prises en charge",
    ]);
    box_.append(&filter);
    let mut functions =
        BTreeMap::<String, Vec<&gameshift_telemetry::capabilities::Capability>>::new();
    for cap in &report.capabilities {
        functions
            .entry(function_key(&cap.id))
            .or_default()
            .push(cap);
    }
    let mut rows = Vec::new();
    for title in [
        "Refroidissement",
        "Énergie et performances",
        "Fréquences et tensions",
        "Mémoire et PCIe",
        "Diagnostic matériel",
        "Fonctions avancées",
        "Interfaces inconnues",
    ] {
        let group = gtk::Box::new(gtk::Orientation::Vertical, 6);
        group.add_css_class("gs-gpu-lab-group");
        group.append(&text(title, "gs-section-title"));
        let mut count = 0;
        for (key, attributes) in &functions {
            if functional_group(attributes[0]) != title {
                continue;
            }
            count += 1;
            let writable = attributes
                .iter()
                .any(|c| matches!(c.write_state, WriteState::Allowed));
            let advanced = attributes.iter().any(|c| c.category == Category::Advanced);
            let unsupported = attributes.iter().all(|c| {
                matches!(
                    c.write_state,
                    WriteState::Forbidden
                        | WriteState::Unsupported
                        | WriteState::NotImplemented
                        | WriteState::Unavailable
                )
            });
            let state = if writable {
                "Écriture autorisée via Contrôle"
            } else if attributes.iter().all(|c| c.present == Some(false)) {
                "Indisponible"
            } else if attributes
                .iter()
                .any(|c| c.read_policy == ReadPolicy::Forbidden)
            {
                "Critique / interdit"
            } else if attributes
                .iter()
                .any(|c| matches!(c.write_state, WriteState::NotImplemented))
            {
                "Écriture non implémentée"
            } else if attributes.iter().all(|c| c.read_access.is_none()) {
                "Non lu / lecteur non pris en charge"
            } else if attributes.iter().any(|c| c.read_state == ReadState::Denied) {
                "Lecture refusée"
            } else {
                "Observation"
            };
            let row = gtk::Expander::new(None);
            let header = text(&format!("{} · {state}", capability_name(key)), "heading");
            row.set_label_widget(Some(&header));
            let details = gtk::Box::new(gtk::Orientation::Vertical, 8);
            for cap in attributes {
                let current = cap.value.as_deref().unwrap_or("Non lu");
                let level = match cap.category {
                    Category::Observation => "Observation",
                    Category::Standard => "Réglage standard",
                    Category::Advanced => "Réglage avancé",
                    Category::Critical => "Critique / non autorisé",
                };
                details.append(&text(
                    &format!(
                        "{} · {level}\n{}\n{}\n{}\n{}",
                        cap.description,
                        cap.conditions,
                        cap.risk,
                        cap.restoration,
                        cap.read_error.as_deref().unwrap_or("")
                    ),
                    "gs-gpu-caption",
                ));
                let raw = text(&format!("{}\n{current}", cap.source), "gs-mono");
                raw.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                raw.set_selectable(true);
                details.append(&raw);
            }
            row.set_child(Some(&details));
            group.append(&row);
            rows.push((
                row,
                format!(
                    "{key} {}",
                    attributes
                        .iter()
                        .map(|c| c.description.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                )
                .to_lowercase(),
                writable,
                advanced,
                unsupported,
            ));
        }
        if count > 0 {
            box_.append(&group);
        }
    }
    for warning in &report.warnings {
        box_.append(&text(warning, "gs-gpu-caption"));
    }
    let rows = Rc::new(rows);
    let weak_search = search.downgrade();
    let weak_filter = filter.downgrade();
    let update = Rc::new(move || {
        if let (Some(search), Some(filter)) = (weak_search.upgrade(), weak_filter.upgrade()) {
            let query = search.text().to_lowercase();
            let selected = filter.selected();
            for (row, haystack, writable, advanced, unsupported) in rows.iter() {
                let allowed = match selected {
                    1 => *writable,
                    2 => !*writable,
                    3 => *advanced,
                    4 => *unsupported,
                    _ => true,
                };
                row.set_visible(allowed && (query.is_empty() || haystack.contains(&query)));
            }
        }
    });
    let u = update.clone();
    search.connect_search_changed(move |_| u());
    filter.connect_selected_notify(move |_| update());
}

fn laboratory(
    selected: Rc<RefCell<Option<String>>>,
    stack: &gtk::Stack,
    selector: &gtk::DropDown,
) -> (gtk::Box, gtk::Expander) {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.add_css_class("gs-gpu-surface");
    let expander = gtk::Expander::new(Some("Laboratoire du pilote"));
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.append(&text(
        "Explorer les fonctions du pilote. Les réglages autorisés sont appliqués depuis l’espace Contrôle. Les interfaces de reset, d’éviction ou inconnues restent protégées.",
        "gs-gpu-caption",
    ));
    let debug = gtk::CheckButton::with_label(
        "Inclure debugfs si accessible (lectures sélectionnées uniquement)",
    );
    body.append(&debug);
    let scan = gtk::Button::with_label("Explorer les capacités");
    scan.set_halign(gtk::Align::Start);
    body.append(&scan);
    let status = text(
        "Choisissez un GPU AMDGPU puis lancez une exploration.",
        "gs-gpu-caption",
    );
    body.append(&status);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    body.append(&content);
    expander.set_child(Some(&body));
    root.append(&expander);

    let clear_content = content.downgrade();
    let clear_status = status.downgrade();
    selector.connect_selected_notify(move |_| {
        if let Some(content) = clear_content.upgrade() {
            while let Some(child) = content.first_child() {
                content.remove(&child);
            }
        }
        if let Some(status) = clear_status.upgrade() {
            status.set_text("Sélection modifiée : lancez une exploration pour ce périphérique.");
        }
    });
    let weak_content = content.downgrade();
    let weak_status = status.downgrade();
    let weak_stack = stack.downgrade();
    scan.connect_clicked(move |button| {
        let (Some(status), Some(stack)) = (weak_status.upgrade(), weak_stack.upgrade()) else { return; };
        if crate::live_settings::benchmark_running() {
            status.set_text("Exploration suspendue pendant le traitement du laboratoire Aurelia.");
            return;
        }
        let Some(pci) = selected.borrow().clone() else {
            status.set_text("Aucun périphérique AMDGPU sélectionné ; la VM ne préjuge pas du support sur Fedora.");
            return;
        };
        let include_debugfs = debug.is_active();
        let (tx, rx) = mpsc::sync_channel(1);
        button.set_sensitive(false);
        status.set_text("Inventaire en cours, hors du thread graphique…");
        let request_pci = pci.clone();
        let spawned = std::thread::Builder::new()
            .name("gameshift-driver-inventory".into())
            .spawn(move || { let _ = tx.send(Inventory::discover(&request_pci, include_debugfs)); });
        if let Err(error) = spawned {
            button.set_sensitive(true);
            status.set_text(&error.to_string());
            return;
        }
        let weak_button = button.downgrade();
        let weak_content = weak_content.clone();
        let selected = selected.clone();
        let weak_stack = stack.downgrade();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let (Some(button), Some(content), Some(stack)) = (
                weak_button.upgrade(), weak_content.upgrade(), weak_stack.upgrade(),
            ) else { return glib::ControlFlow::Break; };
            match rx.try_recv() {
                Ok(result) => {
                    button.set_sensitive(true);
                    if selected.borrow().as_deref() != Some(pci.as_str())
                        || stack.visible_child_name().as_deref() != Some("Carte graphique")
                        || crate::live_settings::benchmark_running()
                    {
                        status.set_text("Exploration différée ou sélection modifiée : relancez-la pour le GPU courant.");
                        return glib::ControlFlow::Break;
                    }
                    match result {
                        Ok(report) => {
                            lab_contents(&report, &content);
                            status.set_text("Inventaire terminé · aucune écriture effectuée · droits réels de modification non testés.");
                        }
                        Err(error) => status.set_text(&error),
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    button.set_sensitive(true);
                    status.set_text("Exploration interrompue");
                    glib::ControlFlow::Break
                }
            }
        });
    });
    (root, expander)
}

pub fn page(stack: &gtk::Stack, snapshot: &Snapshot) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 18);
    root.add_css_class("gs-gpu");
    root.set_margin_top(20);
    root.set_margin_bottom(24);
    root.set_margin_start(20);
    root.set_margin_end(20);
    let names = snapshot
        .sections
        .iter()
        .flat_map(|s| &s.readings)
        .filter_map(|r| {
            Some((
                r.label.strip_suffix(" — Modèle graphique")?.to_owned(),
                r.value.clone()?,
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let hero = gtk::Box::new(gtk::Orientation::Vertical, 10);
    hero.add_css_class("gs-gpu-hero");
    hero.append(&text("GAMESHIFT / GPU", "gs-gpu-kicker"));
    let identity = text("Votre espace graphique", "gs-gpu-title");
    hero.append(&identity);
    hero.append(&text("Surveiller · Contrôler · Mesurer", "gs-gpu-caption"));
    let selector = gtk::DropDown::from_strings(&["Détection des périphériques AMDGPU…"]);
    selector.set_sensitive(false);
    selector.set_tooltip_text(Some("Carte graphique surveillée et explorée"));
    hero.append(&selector);
    root.append(&hero);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let explore = gtk::Button::with_label("Explorer le pilote");
    explore.add_css_class("gs-gpu-action");
    let measure = gtk::Button::with_label("Mesurer avec Aurelia");
    measure.add_css_class("flat");
    actions.append(&explore);
    actions.append(&measure);
    root.append(&actions);
    let weak = stack.downgrade();
    measure.connect_clicked(move |_| {
        if let Some(stack) = weak.upgrade() {
            if stack.child_by_name("laboratory").is_some() {
                stack.set_visible_child_name("laboratory");
            }
        }
    });
    let status = text("Détection en cours…", "gs-gpu-caption");
    root.append(&status);
    let spaces = gtk::Stack::new();
    CONTROL_SPACE.with_borrow_mut(|w| *w = Some(spaces.downgrade()));
    spaces.set_vhomogeneous(false);
    spaces.set_hhomogeneous(false);
    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&spaces));
    switcher.set_halign(gtk::Align::Start);
    root.append(&switcher);
    root.append(&spaces);
    let monitoring = gtk::Box::new(gtk::Orientation::Vertical, 16);
    let surveillance = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let views = gtk::Stack::new();
    views.set_vhomogeneous(false);
    views.set_hhomogeneous(false);
    let tabs = gtk::StackSwitcher::new();
    tabs.set_stack(Some(&views));
    surveillance.append(&tabs);
    surveillance.append(&views);
    views.add_titled(&monitoring, Some("sensors"), "Capteurs Radeon");
    views.add_titled(
        &crate::gpu_processes::panel(names.clone()),
        Some("processes"),
        "Processus GPU",
    );
    let hero_ = hero.clone();
    let actions_ = actions.clone();
    let status_ = status.clone();
    views.connect_visible_child_name_notify(move |views| {
        let sensors = views.visible_child_name().as_deref() == Some("sensors");
        hero_.set_visible(sensors);
        actions_.set_visible(sensors);
        status_.set_visible(sensors);
    });
    spaces.add_titled(&surveillance, Some("monitoring"), "Surveillance");
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_min_children_per_line(1);
    grid.set_max_children_per_line(4);
    grid.set_row_spacing(10);
    grid.set_column_spacing(10);
    grid.add_css_class("gs-gpu-grid");
    monitoring.append(&grid);
    let utilization = tile("Occupation", "utilities-system-monitor-symbolic", &grid);
    let gpu_temperature = tile("Température GPU", "weather-clear-symbolic", &grid);
    let hotspot = tile("Point chaud", "weather-clear-symbolic", &grid);
    let clock = tile("Fréquence GPU", "preferences-system-symbolic", &grid);
    let vram = tile("Mémoire vidéo", "media-flash-symbolic", &grid);
    let memory_clock = tile("Fréquence VRAM", "media-flash-symbolic", &grid);
    let power = tile("Puissance", "battery-good-symbolic", &grid);
    let fan = tile("Ventilation", "preferences-system-symbolic", &grid);
    let graph = gtk::Box::new(gtk::Orientation::Vertical, 10);
    graph.add_css_class("gs-gpu-surface");
    graph.append(&text("Activité du GPU", "gs-section-title"));
    graph.append(&text(
        "Occupation globale · 60 secondes · échelle fixe 0–100 %",
        "gs-gpu-caption",
    ));
    let chart = crate::live_chart::PercentChart::new("Occupation GPU · %");
    chart.widget.set_content_height(230);
    chart.widget.add_css_class("gs-gpu-chart");
    graph.append(&chart.widget);
    monitoring.append(&graph);
    let selected = Rc::new(RefCell::new(None::<String>));
    let selected_pci = selected.clone();
    let device_ids = Rc::new(RefCell::new(Vec::<String>::new()));
    let ids = device_ids.clone();
    let reset_chart = chart.clone();
    let cleared_values = [
        &utilization,
        &gpu_temperature,
        &hotspot,
        &clock,
        &vram,
        &memory_clock,
        &power,
        &fan,
    ]
    .into_iter()
    .map(|t| {
        (
            t.value.downgrade(),
            t.detail.downgrade(),
            t.gauge.downgrade(),
        )
    })
    .collect::<Vec<_>>();
    let changed_identity = identity.downgrade();
    let names_on_selection = names.clone();
    selector.connect_selected_notify(move |dropdown| {
        *selected_pci.borrow_mut() = ids.borrow().get(dropdown.selected() as usize).cloned();
        reset_chart.clear();
        for (value, detail, gauge) in &cleared_values {
            if let Some(value) = value.upgrade() {
                value.set_text("En attente…");
            }
            if let Some(detail) = detail.upgrade() {
                detail.set_text("Nouveau périphérique sélectionné");
            }
            if let Some(gauge) = gauge.upgrade() {
                gauge.set_visible(false);
            }
        }
        if let Some(identity) = changed_identity.upgrade() {
            identity.set_text(
                selected_pci
                    .borrow()
                    .as_ref()
                    .and_then(|pci| names_on_selection.get(pci))
                    .map(String::as_str)
                    .unwrap_or("GPU AMD"),
            );
        }
    });
    let (lab, expander) = laboratory(selected.clone(), stack, &selector);
    expander.set_expanded(true);
    let exploration = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(520)
        .child(&lab)
        .build();
    spaces.add_titled(&exploration, Some("exploration"), "Exploration");
    let controls = crate::amd_controls::panel(selected.clone());
    spaces.add_titled(&controls, Some("controls"), "Contrôle");
    let weak_spaces = spaces.downgrade();
    explore.connect_clicked(move |_| {
        if let Some(spaces) = weak_spaces.upgrade() {
            spaces.set_visible_child_name("exploration");
        }
    });
    let sources = text("", "gs-mono");
    sources.set_selectable(true);
    let advanced = gtk::Expander::new(Some("Sources et relevés détaillés"));
    advanced.set_child(Some(&sources));
    monitoring.append(&advanced);
    let weak_root = root.downgrade();
    let weak_stack = stack.downgrade();
    let mut pending: Option<mpsc::Receiver<Result<Vec<Frame>, String>>> = None;
    let mut last = Instant::now() - Duration::from_secs(5);
    let mut discard = false;
    let mut visible = false;
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let (Some(_root), Some(stack)) = (weak_root.upgrade(), weak_stack.upgrade()) else {
            return glib::ControlFlow::Break;
        };
        if stack.visible_child_name().as_deref() != Some("Carte graphique")
            || crate::live_settings::benchmark_running()
        {
            selector.set_sensitive(false);
            if visible {
                chart.push(None);
            }
            visible = false;
            if let Some(rx) = &pending {
                if !matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                    pending = None;
                }
            }
            discard = pending.is_some();
            return glib::ControlFlow::Continue;
        }
        visible = true;
        if let Some(rx) = &pending {
            match rx.try_recv() {
                Ok(result) => {
                    pending = None;
                    if discard {
                        discard = false;
                        return glib::ControlFlow::Continue;
                    }
                    match result {
                        Ok(frames) => {
                            let new_ids = frames.iter().map(|f| f.pci.clone()).collect::<Vec<_>>();
                            if *device_ids.borrow() != new_ids {
                                let old = selected.borrow().clone();
                                let labels = frames
                                    .iter()
                                    .map(|f| {
                                        format!(
                                            "{} · {}",
                                            names
                                                .get(&f.pci)
                                                .map(String::as_str)
                                                .unwrap_or("GPU AMD"),
                                            f.pci
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                let refs = labels.iter().map(String::as_str).collect::<Vec<_>>();
                                *device_ids.borrow_mut() = new_ids;
                                selector.set_model(Some(&gtk::StringList::new(&refs)));
                                let index = old
                                    .and_then(|pci| {
                                        device_ids.borrow().iter().position(|id| *id == pci)
                                    })
                                    .unwrap_or(0);
                                selector.set_selected(index as u32);
                                *selected.borrow_mut() = device_ids.borrow().get(index).cloned();
                                chart.push(None);
                            }
                            selector.set_sensitive(
                                !frames.is_empty() && !crate::amd_controls::any_active(),
                            );
                            if let Some(frame) = frames
                                .iter()
                                .find(|f| Some(f.pci.as_str()) == selected.borrow().as_deref())
                            {
                                identity.set_text(
                                    names
                                        .get(&frame.pci)
                                        .map(String::as_str)
                                        .unwrap_or("GPU AMD"),
                                );
                                let values = &frame.values;
                                let busy = metric(values, Metric::Utilization);
                                utilization.update(busy, "%", 1.);
                                utilization
                                    .gauge
                                    .set_visible(busy.and_then(|m| m.value).is_some());
                                if let Some(value) = busy.and_then(|m| m.value) {
                                    utilization.gauge.set_value(value);
                                }
                                gpu_temperature.update(temperature_value(values), "°C", 1.);
                                hotspot.update(
                                    temperature(values, &["junction", "hotspot"]),
                                    "°C",
                                    1.,
                                );
                                clock.update(metric(values, Metric::CoreClock), "MHz", 1.);
                                memory_clock.update(metric(values, Metric::MemoryClock), "MHz", 1.);
                                vram.update(
                                    metric(values, Metric::VramUsed),
                                    "Gio",
                                    1_073_741_824.,
                                );
                                if let Some(total) =
                                    metric(values, Metric::VramTotal).and_then(|m| m.value)
                                {
                                    vram.detail.set_text(&format!(
                                        "sur {:.1} Gio",
                                        total / 1_073_741_824.
                                    ));
                                }
                                power.update(metric(values, Metric::Power), "W", 1.);
                                fan.update(metric(values, Metric::FanSpeed), "tr/min", 1.);
                                chart.push(busy.and_then(|m| m.value));
                                let raw = values
                                    .iter()
                                    .map(|m| {
                                        format!(
                                            "{} : {}\n{}",
                                            m.label,
                                            crate::live_gpu::format_measurement(m),
                                            provenance(Some(m))
                                        )
                                    })
                                    .chain(frame.errors.iter().cloned())
                                    .collect::<Vec<_>>()
                                    .join("\n\n");
                                sources.set_text(&raw);
                                status.set_text(&format!(
                                    "{} · amdgpu · relevés en direct · lecture seule",
                                    frame.pci
                                ));
                                grid.set_visible(true);
                                graph.set_visible(true);
                            } else {
                                identity.set_text("Votre espace graphique");
                                status.set_text("Aucun périphérique AMDGPU exposé ici. L’absence de Radeon dans la VM ne préjuge pas de sa prise en charge sur Fedora.");
                                grid.set_visible(false);
                                graph.set_visible(false);
                                chart.push(None);
                                sources.set_text("");
                            }
                        }
                        Err(error) => {
                            status.set_text(&error);
                            chart.push(None);
                            grid.set_visible(false);
                            graph.set_visible(false);
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    pending = None;
                    status.set_text("Collecte interrompue");
                    chart.push(None);
                    grid.set_visible(false);
                    graph.set_visible(false);
                }
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        if pending.is_none() && last.elapsed() >= crate::live_settings::interval() {
            let (tx, rx) = mpsc::sync_channel(1);
            match std::thread::Builder::new()
                .name("gameshift-gpu-page".into())
                .spawn(move || {
                    let _ = tx.send(collect());
                }) {
                Ok(_) => pending = Some(rx),
                Err(error) => {
                    status.set_text(&format!("Collecte indisponible : {error}"));
                    chart.push(None);
                    grid.set_visible(false);
                    graph.set_visible(false);
                }
            }
            last = Instant::now();
        }
        glib::ControlFlow::Continue
    });
    root
}
fn temperature_value(values: &[Measurement]) -> Option<&Measurement> {
    temperature(values, &["edge", "gpu"])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hotspot_is_not_substituted_for_the_gpu_edge_temperature() {
        let samples = vec![Measurement {
            metric: Metric::Temperature,
            label: "junction".into(),
            unit: gameshift_telemetry::device::Unit::Celsius,
            source: "fixture".into(),
            captured_unix_millis: 1,
            value: Some(80.),
            error: None,
        }];
        assert!(temperature_value(&samples).is_none());
        assert_eq!(
            temperature(&samples, &["junction", "hotspot"])
                .unwrap()
                .value,
            Some(80.)
        );
        assert_eq!(value(None, "°C", 1.), "Indisponible");
    }
}
