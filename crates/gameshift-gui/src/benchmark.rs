//! The first functional benchmark milestone: asynchronous Vulkan capability inspection.
use adw::prelude::*;
use gameshift_bench::{probe, GpuCapabilities, ValidationMode, VulkanReport};
use gtk::glib;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    time::Duration,
};

fn text(value: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(value));
    label.add_css_class(class);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_selectable(true);
    label
}

fn detail(label: &str, value: &str) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
    card.add_css_class("gs-card");
    card.append(&text(label, "gs-card-label"));
    card.append(&text(value, "gs-card-value"));
    card
}

fn show_gpu(content: &gtk::Box, gpu: &GpuCapabilities) {
    while let Some(child) = content.first_child() {
        content.remove(&child);
    }
    content.append(&detail("Périphérique sélectionné", &gpu.name));
    content.append(&text(
        &format!("{} · Vulkan {}", gpu.kind, gpu.api_version),
        "gs-page-caption",
    ));
    if let Some(driver) = &gpu.driver_name {
        content.append(&detail("Pilote Vulkan", driver));
    }
    content.append(&detail(
        "Mémoire locale déclarée",
        &format!(
            "{:.1} Gio",
            gpu.device_local_heap_bytes as f64 / 1_073_741_824.0
        ),
    ));
    content.append(&text("La mémoire locale annoncée peut inclure de la mémoire partagée sur un GPU intégré. Cette valeur n’est pas une mesure de mémoire libre.", "gs-card-detail"));
    let readiness = if gpu.raster_candidate() {
        "Fonctions graphiques et extension d’affichage disponibles"
    } else {
        "Prérequis graphiques incomplets pour le rendu prévu"
    };
    content.append(&detail("Prérequis de rendu", readiness));
    content.append(&detail(
        "Mesure du temps de calcul GPU",
        if gpu.graphics_timestamps() {
            "Horodatages GPU disponibles"
        } else {
            "Indisponible pour le rendu graphique"
        },
    ));
    content.append(&text(
        "La présentation et les mesures de temps GPU doivent être vérifiées lors d’un rendu réel.",
        "gs-card-detail",
    ));
    let extensions = gtk::Expander::new(Some(&format!(
        "Extensions Vulkan · {}",
        gpu.extensions.len()
    )));
    extensions.add_css_class("gs-technical");
    let raw = text(&gpu.extensions.join("\n"), "gs-mono");
    extensions.set_child(Some(&raw));
    content.append(&extensions);
}

pub fn page() -> gtk::ScrolledWindow {
    let layout = gtk::Box::new(gtk::Orientation::Vertical, 24);
    layout.set_margin_start(38);
    layout.set_margin_end(38);
    layout.set_margin_top(26);
    layout.set_margin_bottom(40);
    layout.append(&text("Aurelia", "gs-page-title"));
    layout.append(&text(
        "Mesures du processeur et de la carte graphique.",
        "gs-page-caption",
    ));
    let active = Rc::new(Cell::new(false));
    crate::live_settings::register_benchmark(&active);
    let spaces = gtk::Stack::new();
    spaces.set_vhomogeneous(false);
    spaces.set_hhomogeneous(false);
    spaces.set_transition_type(gtk::StackTransitionType::Crossfade);
    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&spaces));
    layout.append(&switcher);
    layout.append(&spaces);
    let home = gtk::Box::new(gtk::Orientation::Vertical, 18);
    home.append(&text(
        "Choisissez ce que vous voulez mesurer",
        "gs-section-title",
    ));
    home.append(&text(
        "Des mesures physiques et des protocoles identifiés. Aucun score global inventé.",
        "gs-card-detail",
    ));
    let choices = gtk::FlowBox::new();
    choices.set_selection_mode(gtk::SelectionMode::None);
    choices.set_min_children_per_line(1);
    choices.set_max_children_per_line(2);
    choices.set_homogeneous(true);
    choices.set_row_spacing(14);
    choices.set_column_spacing(14);
    for (name,title,description) in [("system","Système · CPU, cache et mémoire","Évaluation généraliste : sept domaines, rendu GPU et bibliothèques selon disponibilité."),("cpu","CPU · architecture et parallélisme","Rapide, Standard ou Approfondi : calcul, threads et sensibilité aux tailles de données."),("vulkan","GPU · scène Vulkan","Rendu 3D conservé, répétitions, stabilité et temps par image."),("storage","Stockage · à venir","Épreuve dédiée non implémentée. Aucun débit simulé ni écriture de test.")]{
        let card=gtk::Box::new(gtk::Orientation::Vertical,10);card.add_css_class("gs-card");card.append(&text(title,"gs-section-title"));card.append(&text(description,"gs-card-detail"));let button=gtk::Button::with_label(if name=="storage"{"Indisponible dans cette alpha"}else{"Préparer ce test"});button.set_sensitive(name!="storage");let weak=spaces.downgrade();button.connect_clicked(move |_|{if let Some(stack)=weak.upgrade(){stack.set_visible_child_name(name);}});card.append(&button);choices.insert(&card,-1);
    }
    home.append(&choices);
    home.append(&crate::benchmark_dashboard::hardware());
    home.append(&crate::benchmark_dashboard::latest());
    let launch = gtk::Button::with_label("Préparer l’évaluation système");
    launch.add_css_class("suggested-action");
    launch.add_css_class("pill");
    launch.set_halign(gtk::Align::Start);
    let weak = spaces.downgrade();
    launch.connect_clicked(move |_| {
        if let Some(stack) = weak.upgrade() {
            stack.set_visible_child_name("system");
        }
    });
    home.append(&launch);
    spaces.add_titled(&home, Some("home"), "Accueil");
    spaces.add_titled(
        &crate::system_lab::panel(active.clone()),
        Some("system"),
        "Système & historique",
    );
    spaces.add_titled(
        &crate::cpu_lab::panel(active.clone()),
        Some("cpu"),
        "CPU avancé",
    );
    let gpu = gtk::Box::new(gtk::Orientation::Vertical, 16);
    spaces.add_titled(&gpu, Some("vulkan"), "GPU Vulkan");
    let introduction = gtk::Box::new(gtk::Orientation::Vertical, 16);
    introduction.add_css_class("gs-card");
    introduction.append(&text("Préparer un test fiable", "gs-section-title"));
    introduction.append(&text("Identifiez le GPU et son pilote, puis lancez la scène 3D. Le test s’ouvre dans une fenêtre dédiée ; ses résultats reviennent ici.", "gs-card-detail"));
    let analyze = gtk::Button::with_label("Détecter les GPU Vulkan");
    analyze.add_css_class("suggested-action");
    analyze.set_halign(gtk::Align::Start);
    introduction.append(&analyze);
    gpu.append(&introduction);
    let status = text(
        "L’analyse ne modifie aucun paramètre système.",
        "gs-card-detail",
    );
    gpu.append(&status);
    let selection = gtk::DropDown::from_strings(&["Aucun GPU détecté pour le moment"]);
    selection.set_sensitive(false);
    selection.set_tooltip_text(Some("Carte graphique utilisée pour le test"));
    gpu.append(&crate::context_help::heading("Carte graphique du test", "Choisissez le GPU utilisé pour le rendu Vulkan. Sur une machine à plusieurs cartes, gardez la même sélection pour comparer. Le rendu logiciel utilise le CPU et ne mesure pas les performances d’une carte graphique."));
    crate::context_help::describe(
        &selection,
        "Carte graphique choisie pour le test Vulkan ; conserver la même carte pour comparer.",
    );
    gpu.append(&selection);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    let technical = gtk::Expander::new(Some("Détails du GPU et capacités Vulkan"));
    technical.set_child(Some(&content));
    gpu.append(&technical);
    let report: Rc<RefCell<Option<VulkanReport>>> = Rc::new(RefCell::new(None));
    let report_for_selection = report.clone();
    let content_weak = content.downgrade();
    selection.connect_selected_notify(move |dropdown| {
        if let (Some(content), Some(report)) = (
            content_weak.upgrade(),
            report_for_selection.borrow().as_ref(),
        ) {
            if let Some(gpu) = report.gpus.get(dropdown.selected() as usize) {
                show_gpu(&content, gpu);
            }
        }
    });
    let status_weak = status.downgrade();
    let selection_weak = selection.downgrade();
    let content_weak = content.downgrade();
    let report_for_probe = report.clone();
    analyze.connect_clicked(move |button| {
        let report = &report_for_probe;
        let (Some(status), Some(selection), Some(content)) = (
            status_weak.upgrade(),
            selection_weak.upgrade(),
            content_weak.upgrade(),
        ) else {
            return;
        };
        if crate::live_settings::benchmark_running() {
            status.set_text("Attendre la fin du benchmark avant une nouvelle analyse Vulkan.");
            return;
        }
        button.set_sensitive(false);
        selection.set_sensitive(false);
        while let Some(child) = content.first_child() {
            content.remove(&child);
        }
        *report.borrow_mut() = None;
        status.set_text("Interrogation du pilote Vulkan…");
        let (sender, receiver) = mpsc::sync_channel(1);
        if let Err(error) = std::thread::Builder::new()
            .name("gameshift-vulkan".into())
            .spawn(move || {
                let _ = sender.send(probe(ValidationMode::IfAvailable));
            })
        {
            status.set_text(&format!("Impossible de démarrer l’analyse : {error}"));
            button.set_sensitive(true);
            return;
        }
        let button_weak = button.downgrade();
        let status_weak = status.downgrade();
        let selection_weak = selection.downgrade();
        let content_weak = content.downgrade();
        let report = report.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let (Some(button), Some(status), Some(selection), Some(content)) = (
                button_weak.upgrade(),
                status_weak.upgrade(),
                selection_weak.upgrade(),
                content_weak.upgrade(),
            ) else {
                return glib::ControlFlow::Break;
            };
            match receiver.try_recv() {
                Ok(Ok(result)) => {
                    let names = gtk::StringList::new(
                        &result
                            .gpus
                            .iter()
                            .map(|g| format!("{} — {}", g.name, g.kind))
                            .collect::<Vec<_>>()
                            .iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>(),
                    );
                    let validation = if result.validation_enabled {
                        "vérifications Vulkan activées"
                    } else {
                        "vérifications Vulkan indisponibles ou désactivées"
                    };
                    status.set_text(&format!(
                        "{} GPU · chargeur Vulkan {} · {}. {}",
                        result.gpus.len(),
                        result.loader_api_version,
                        validation,
                        result
                            .warnings
                            .iter()
                            .map(|w| gameshift_bench::display_warning(w))
                            .collect::<Vec<_>>()
                            .join(" ")
                    ));
                    let selected = result
                        .gpus
                        .iter()
                        .position(|g| g.kind == "GPU dédié" && g.raster_candidate())
                        .unwrap_or(0);
                    *report.borrow_mut() = Some(result);
                    selection.set_model(Some(&names));
                    selection.set_selected(selected as u32);
                    selection.set_sensitive(true);
                    if let Some(gpu) = report.borrow().as_ref().and_then(|r| r.gpus.get(selected)) {
                        show_gpu(&content, gpu);
                    }
                    button.set_sensitive(true);
                    glib::ControlFlow::Break
                }
                Ok(Err(error)) => {
                    status.set_text(&format!("Analyse Vulkan indisponible : {error}"));
                    button.set_sensitive(true);
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    status.set_text("Analyse interrompue. Vous pouvez réessayer.");
                    button.set_sensitive(true);
                    glib::ControlFlow::Break
                }
            }
        });
    });
    let analyzed = Rc::new(Cell::new(false));
    let probe_button = analyze.clone();
    gpu.connect_map(move |_| {
        if !analyzed.get() && !crate::live_settings::benchmark_running() {
            analyzed.set(true);
            probe_button.emit_clicked();
        }
    });
    let run_panel = crate::benchmark_run::panel(&selection, &analyze, report, active);
    gpu.insert_child_after(&run_panel, Some(&selection));
    let clamp = adw::Clamp::builder()
        .maximum_size(1500)
        .child(&layout)
        .build();
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp)
        .build()
}
