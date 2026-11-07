//! Desktop diagnostics and a headless JSON export using the same collector.
use adw::prelude::*;
mod amd_controls;
mod bench_process;
mod benchmark;
mod benchmark_dashboard;
mod benchmark_run;
mod community;
mod context_help;
mod cpu_lab;
mod diagnostic_context;
mod diagnostic_view;
mod duration_ui;
mod gpu_page;
mod gpu_processes;
mod live_chart;
mod live_cpu;
mod live_gpu;
mod live_settings;
mod presentation;
mod profiles_page;
mod system_lab;
mod ui_smoke;

use gameshift_core::Snapshot;
use gameshift_hardware::Collector;
use gtk::glib;
use std::{sync::mpsc, time::Duration};

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();
    if std::env::args().any(|arg| arg == "--community-diagnostic-json") {
        println!("{}", community::build());
        return glib::ExitCode::SUCCESS;
    }
    if std::env::args().any(|arg| arg == "--controls-json") {
        println!("{}", gameshift_control::snapshot_all());
        return glib::ExitCode::SUCCESS;
    }
    if std::env::args().any(|arg| arg == "--driver-lab-json") {
        return match driver_inventory_json() {
            Ok(value) => {
                println!("{value}");
                glib::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                glib::ExitCode::FAILURE
            }
        };
    }
    if std::env::args().any(|arg| arg == "--bench-worker" || arg == "--bench-worker-headless") {
        let headless = std::env::args().any(|arg| arg == "--bench-worker-headless");
        return match bench_process::worker(headless) {
            Ok(result) => match serde_json::to_string(&result) {
                Ok(json) => {
                    println!("{json}");
                    glib::ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("Export du benchmark : {e}");
                    glib::ExitCode::FAILURE
                }
            },
            Err(e) => {
                eprintln!("{e}");
                glib::ExitCode::FAILURE
            }
        };
    }
    if std::env::args().any(|arg| arg == "--render-smoke") {
        let mut request = String::new();
        use std::io::Read;
        let result = (|| -> Result<(), String> {
            std::io::stdin()
                .take(16_384)
                .read_to_string(&mut request)
                .map_err(|e| e.to_string())?;
            let config: gameshift_bench::RunConfig =
                serde_json::from_str(&request).map_err(|e| e.to_string())?;
            let width = config.width;
            let height = config.height;
            let (name, pixels, errors) = gameshift_bench::smoke_offscreen(config)?;
            if let Some(path) = std::env::args().nth(2) {
                let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
                for rgba in pixels.as_chunks::<4>().0.iter() {
                    ppm.extend_from_slice(&rgba[..3]);
                }
                std::fs::write(path, ppm).map_err(|e| e.to_string())?;
            }
            let distinct: std::collections::BTreeSet<_> = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| [p[0], p[1], p[2], p[3]])
                .collect();
            if distinct.len() < 10 {
                return Err("Le readback ne contient pas une scène visible".into());
            }
            println!(
                "{}",
                serde_json::json!({"gpu":name,"validation_errors":errors,"distinct_colors":distinct.len(),"pixels":pixels.len()/4,"performance_score":null})
            );
            Ok(())
        })();
        return match result {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Smoke test : {e}");
                glib::ExitCode::FAILURE
            }
        };
    }
    if std::env::args().any(|arg| arg == "--export-text-reports") {
        let target = std::env::args_os().nth(2).map(std::path::PathBuf::from);
        return match bench_process::export_existing_text(target.as_deref()) {
            Ok(paths) => {
                for path in paths {
                    println!("{}", path.display());
                }
                glib::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Export texte : {e}");
                glib::ExitCode::FAILURE
            }
        };
    }
    if std::env::args().any(|arg| arg == "--diagnose-json") {
        return match serde_json::to_string_pretty(&Collector::default().collect()) {
            Ok(json) => {
                println!("{json}");
                glib::ExitCode::SUCCESS
            }
            Err(error) => {
                tracing::error!(%error, "JSON export failed");
                glib::ExitCode::FAILURE
            }
        };
    }
    if std::env::args().any(|arg| arg == "--vulkan-json") {
        let validation = if std::env::args().any(|arg| arg == "--require-validation") {
            gameshift_bench::ValidationMode::Required
        } else {
            gameshift_bench::ValidationMode::IfAvailable
        };
        return match gameshift_bench::probe(validation) {
            Ok(report) => match serde_json::to_string_pretty(&report) {
                Ok(json) => {
                    println!("{json}");
                    glib::ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("Export Vulkan : {error}");
                    glib::ExitCode::FAILURE
                }
            },
            Err(error) => {
                eprintln!("{error}");
                glib::ExitCode::FAILURE
            }
        };
    }
    if std::env::args().any(|arg| arg == "--ui-smoke") {
        let directory = std::env::args_os()
            .nth(2)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp/gameshift-ui-smoke"));
        return match ui_smoke::run(directory) {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Contrôle GTK : {error}");
                glib::ExitCode::FAILURE
            }
        };
    }
    let app = adw::Application::builder()
        .application_id("io.github.gameshift.GameShift")
        .build();
    let quit = gtk::gio::SimpleAction::new("quit-interface", None);
    let weak = app.downgrade();
    quit.connect_activate(move |_, _| {
        std::thread::spawn(|| {
            let _ = gameshift_profiles::call(serde_json::json!({"command":"tray","visible":false}));
        });
        if let Some(app) = weak.upgrade() {
            app.quit();
        }
    });
    app.add_action(&quit);
    app.connect_activate(build_ui);
    app.run()
}

fn driver_inventory_json() -> Result<serde_json::Value, String> {
    use gameshift_telemetry::{capabilities::Inventory, device::DeviceSensors};
    let args = std::env::args().collect::<Vec<_>>();
    let include_debugfs = args.iter().any(|s| s == "--include-debugfs");
    let pci = args
        .iter()
        .position(|s| s == "--driver-lab-json")
        .and_then(|i| args.get(i + 1))
        .filter(|s| !s.starts_with("--"));
    let mut inventories = Vec::new();
    let mut errors = Vec::new();
    if let Some(pci) = pci {
        inventories.push(Inventory::discover(pci, include_debugfs)?);
    } else {
        for result in DeviceSensors::discover_all()? {
            match result
                .and_then(|device| Inventory::discover(&device.pci_address, include_debugfs))
            {
                Ok(inventory) => inventories.push(inventory),
                Err(error) => errors.push(error),
            }
        }
    }
    Ok(
        serde_json::json!({"schema_version":1,"mode":"read_only","inventories":inventories,"errors":errors,"note":"Les lectures à effet de bord et les interfaces non reconnues ne sont pas ouvertes. Aucun changement matériel n’est appliqué."}),
    )
}

fn build_ui(app: &adw::Application) {
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }
    profiles_page::session_open();
    live_settings::initialize();
    let provider = gtk::CssProvider::new();
    provider.load_from_data(include_str!("style.css"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("GameShift")
        .default_width(1200)
        .default_height(840)
        .width_request(640)
        .height_request(560)
        .build();
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 12);
    sidebar.add_css_class("gs-sidebar");
    sidebar.set_width_request(180);
    let brand = gtk::Label::new(Some("GameShift"));
    brand.add_css_class("gs-brand");
    brand.set_halign(gtk::Align::Start);
    brand.set_margin_start(26);
    brand.set_margin_top(34);
    sidebar.append(&brand);
    let subtitle = gtk::Label::new(Some("Votre machine, en clair."));
    subtitle.add_css_class("gs-sidebar-note");
    subtitle.set_halign(gtk::Align::Start);
    subtitle.set_margin_start(26);
    subtitle.set_margin_bottom(24);
    sidebar.append(&subtitle);
    let stack = gtk::Stack::new();
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(160);
    let navigation = gtk::Box::new(gtk::Orientation::Vertical, 6);
    navigation.set_vexpand(true);
    let mut first = None;
    for (name, title) in [
        ("overview", "Accueil"),
        ("Carte graphique", "GPU"),
        ("Processeur", "CPU"),
        ("laboratory", "Benchmarks"),
        ("profiles", "Profils"),
        ("settings", "Paramètres"),
    ] {
        let button = gtk::ToggleButton::with_label(title);
        button.set_group(first.as_ref());
        if first.is_none() {
            first = Some(button.clone());
            button.set_active(true);
        }
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
    sidebar.append(&navigation);
    let badge = gtk::Label::new(Some("Diagnostic & mesures"));
    badge.add_css_class("gs-sidebar-note");
    badge.set_margin_bottom(24);
    sidebar.append(&badge);
    root.append(&sidebar);

    let layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    layout.add_css_class("gs-content");
    layout.set_hexpand(true);
    let header = adw::HeaderBar::new();
    header.add_css_class("flat");
    let title = gtk::Label::new(Some("Vue matérielle"));
    title.add_css_class("dim-label");
    header.set_title_widget(Some(&title));
    let title_weak = title.downgrade();
    stack.connect_visible_child_name_notify(move |stack| {
        if let Some(title) = title_weak.upgrade() {
            title.set_text(match stack.visible_child_name().as_deref() {
                Some("laboratory") => "Benchmarks Aurelia",
                Some("settings") => "Paramètres",
                Some("profiles") => "Profils GPU",
                Some("Processeur") => "CPU",
                Some("Carte graphique") => "GPU",
                Some("Système") => "Système",
                Some("Mémoire") => "Mémoire",
                Some("Stockage") => "Stockage",
                _ => "Accueil",
            });
        }
    });
    let refresh = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Actualiser les informations")
        .build();
    refresh.add_css_class("flat");
    header.pack_start(&refresh);
    layout.append(&header);
    layout.append(&stack);
    let status = gtk::Label::new(Some("Prêt pour le diagnostic"));
    status.add_css_class("gs-status");
    status.set_wrap(true);
    status.set_margin_top(10);
    status.set_margin_bottom(14);
    layout.append(&status);
    root.append(&layout);
    window.set_content(Some(&root));
    let stack_weak = stack.downgrade();
    let status_weak = status.downgrade();
    refresh.connect_clicked(move |button| {
        if let (Some(stack), Some(status)) = (stack_weak.upgrade(), status_weak.upgrade()) {
            start_collection(&stack, &status, button);
        }
    });
    let edit = gtk::gio::SimpleAction::new("edit-gpu-profile", None);
    let weak = stack.downgrade();
    edit.connect_activate(move |_, _| {
        if let Some(stack) = weak.upgrade() {
            stack.set_visible_child_name("Carte graphique");
            gpu_page::show_controls();
        }
    });
    app.add_action(&edit);
    window.connect_close_request(|_| {
        if !profiles_page::keep_tray_on_close() {
            std::thread::spawn(|| {
                let _ =
                    gameshift_profiles::call(serde_json::json!({"command":"tray","visible":false}));
            });
        }
        glib::Propagation::Proceed
    });
    start_collection(&stack, &status, &refresh);
    window.present();
}

fn start_collection(stack: &gtk::Stack, status: &gtk::Label, refresh: &gtk::Button) {
    if live_settings::benchmark_running() || amd_controls::any_active() {
        status.set_text("Actualisation différée : attendez la fin du traitement du laboratoire.");
        return;
    }
    refresh.set_sensitive(false);
    status.set_text("Collecte en cours… Aucun réglage système modifié.");
    let (sender, receiver) = mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("gameshift-diagnostic".into())
        .spawn(move || {
            let mut snapshot = Collector::default().collect();
            diagnostic_context::enrich(&mut snapshot);
            let latest = diagnostic_context::latest_report();
            let _ = sender.send((snapshot, latest));
        });
    if let Err(error) = spawned {
        status.set_text(&format!("Impossible de démarrer le diagnostic : {error}"));
        refresh.set_sensitive(true);
        return;
    }
    let stack_weak = stack.downgrade();
    let status_weak = status.downgrade();
    let refresh_weak = refresh.downgrade();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let (Some(stack), Some(status), Some(refresh)) = (
            stack_weak.upgrade(),
            status_weak.upgrade(),
            refresh_weak.upgrade(),
        ) else {
            return glib::ControlFlow::Break;
        };
        match receiver.try_recv() {
            Ok((snapshot, latest)) => {
                if live_settings::benchmark_running() || amd_controls::any_active() {
                    status.set_text(
                        "Le laboratoire est actif ; relancez l’actualisation après le test.",
                    );
                } else {
                    render_snapshot(&stack, &snapshot, latest.as_ref());
                    status.set_text(
                        "Diagnostic actualisé · Lecture seule · Actualisation à la demande.",
                    );
                }
                refresh.set_sensitive(true);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                status.set_text("Le diagnostic a été interrompu. Vous pouvez le relancer.");
                refresh.set_sensitive(true);
                glib::ControlFlow::Break
            }
        }
    });
}

fn render_snapshot(
    stack: &gtk::Stack,
    snapshot: &Snapshot,
    latest: Option<&diagnostic_context::LastReport>,
) {
    let selected = stack.visible_child_name();
    let profiles = stack.child_by_name("profiles");
    if let Some(page) = &profiles {
        stack.remove(page);
    }
    let settings = stack.child_by_name("settings");
    if let Some(page) = &settings {
        stack.remove(page);
    }
    let existing = stack.child_by_name("laboratory");
    if let Some(page) = &existing {
        stack.remove(page);
    }
    presentation::render(stack, snapshot, latest);
    let page = existing.unwrap_or_else(|| benchmark::page().upcast());
    stack
        .add_titled(&page, Some("laboratory"), "Benchmarks")
        .set_icon_name("applications-science-symbolic");
    let settings = settings.unwrap_or_else(|| settings_page().upcast());
    stack.add_titled(&settings, Some("settings"), "Paramètres");
    let profiles = profiles.unwrap_or_else(|| profiles_page::page().upcast());
    stack.add_titled(&profiles, Some("profiles"), "Profils");
    if let Some(selected) = selected {
        if stack.child_by_name(&selected).is_some() {
            stack.set_visible_child_name(&selected);
        }
    }
}

fn settings_page() -> gtk::ScrolledWindow {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(20);
    content.set_margin_bottom(24);
    content.set_margin_start(20);
    content.set_margin_end(20);
    content.append(&live_settings::appearance());
    content.append(&live_settings::preferences());
    content.append(&community::panel());
    let compatibility = adw::PreferencesGroup::new();
    compatibility.set_title("Compatibilité et diagnostic");
    for (title,description) in [
        ("Version", concat!("GameShift ", env!("CARGO_PKG_VERSION"))),
        ("Sources matérielles", "CPU : procfs, cpufreq et hwmon. GPU AMD : périphériques PCI liés à amdgpu, sans ROCm obligatoire. Les relevés indisponibles conservent leur état et leur source."),
        ("Mesures et validation", "Les détails des pages CPU/GPU présentent les capteurs observés. Une machine virtuelle ne valide pas les fonctions d’une Radeon physique."),
        ("Contrôle matériel", "Les réglages AMDGPU reconnus sont accessibles dans GPU → Contrôle via un helper installé et Polkit. Validation des limites et restauration indépendante de GTK. Agent utilisateur pour les profils et contrôleur système à la demande ; démarrage avec la session facultatif. La validation Radeon physique reste nécessaire."),
        ("Historique", "Les résultats terminés sont conservés automatiquement. Exporter crée une copie .gsbench ; retirer un résultat le conserve dans le dossier removed de l’historique."),
    ] {
        let row=adw::ActionRow::new();row.set_title(title);row.set_subtitle(description);row.set_subtitle_lines(0);compatibility.add(&row);
    }
    content.append(&compatibility);
    let clamp = adw::Clamp::builder()
        .maximum_size(1100)
        .child(&content)
        .build();
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp)
        .build()
}
