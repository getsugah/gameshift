//! CPU architecture experiments, asynchronous and independent of Vulkan rendering.
use adw::prelude::*;
use gameshift_cpu::{
    runner::{run_cancellable, Config, Report},
    topology::Topology,
    workload::Pattern,
};
use gtk::glib;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
fn label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.);
    label.set_wrap(true);
    label.set_selectable(true);
    label
}
fn row(layout: &gtk::Box, title: &str, widget: &impl IsA<gtk::Widget>) {
    let explanation=match title {
        "Sous-test"=>"Choisissez un nombre de threads, comparez plusieurs nombres de threads ou faites varier la taille des données pour observer l’influence du cache mémoire. Les calculs restent reproductibles.",
        "Threads CPU utilisés"=>"Nombre de fils d’exécution CPU dédiés à la simulation ; plusieurs peuvent travailler en parallèle. Un thread ne limite pas tout le processus : l’interface et le pilote peuvent utiliser d’autres threads. Le travail total reste identique lors de la comparaison automatique ; ce mode choisit ses propres nombres de threads et ignore ce champ.",
        "Taille des données (Mio, hors mémoire supplémentaire)"=>"Taille cible des zones de travail, en mébioctets (Mio). Le mode Influence du cache mémoire choisit ses onze tailles et ignore ce champ. Les structures spatiales, la gestion mémoire et les mesures enregistrées ajoutent de la mémoire. Une taille de 96 Mio ne garantit pas que les données tiennent dans 96 Mio de cache L3.",
        "Type de test"=>"Simulation d’objets : mises à jour et animations. Traitement de données : accès séquentiels. Accès pseudo-aléatoires : ordre reproductible. Accès dépendants : chaque adresse dépend de la précédente. Simulation spatiale : visibilité et parcours d’un arbre. Simulation combinée : mises à jour d’objets et parcours spatial.",
        "Étapes de simulation mesurées"=>"Nombre d’étapes mesurées par répétition, après la préparation du test. Chaque étape utilise la même durée simulée. Augmenter ce nombre allonge la mesure et consomme plus de mémoire pour les résultats. En mode historique, ce nombre est fixe. En mode calibré, il est calculé automatiquement et indiqué dans les résultats.",
        "Compteurs matériels CPU"=>"Demandent au processeur des compteurs de cycles, instructions et cache, si l’accès est autorisé. Leur collecte ajoute du travail au test. Comparez le même réglage ; un compteur indisponible ne signifie pas que le processeur est défectueux.",
        _=>"Paramètre du test CPU.",
    };
    let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    row.append(&crate::context_help::heading(title, explanation));
    crate::context_help::describe(widget, explanation);
    row.append(widget);
    layout.append(&row);
}

pub fn panel(active: Rc<Cell<bool>>) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 12);
    panel.add_css_class("gs-card");
    let title = label("Architecture du processeur · expérimental");
    title.add_css_class("gs-section-title");
    panel.append(&title);
    panel.append(&label("Simulation CPU sans rendu 3D. Le même nombre d’étapes est exécuté pour chaque nombre de threads testé ; les threads de GTK et du système restent actifs."));
    let topology = Topology::detect();
    panel.append(&label(&format!(
        "{} cœurs physiques disponibles · {} threads logiques · cache mémoire L3 : {}",
        topology
            .physical_cores
            .map(|n| n.to_string())
            .unwrap_or_else(|| "Indisponible".into()),
        topology.allowed_logical_cpus.len(),
        topology
            .l3_bytes
            .map(|n| format!("{} Mio", n / 1048576))
            .unwrap_or_else(|| "Indisponible".into())
    )));
    let controls = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let mode = gtk::DropDown::from_strings(&[
        "Nombre de threads choisi",
        "Comparaison automatique des threads",
        "Influence du cache mémoire",
    ]);
    row(&controls, "Sous-test", &mode);
    let workers =
        gtk::SpinButton::with_range(1., topology.allowed_logical_cpus.len().max(1) as f64, 1.);
    row(&controls, "Threads CPU utilisés", &workers);
    let size = gtk::SpinButton::with_range(1., 256., 1.);
    size.set_value(16.);
    row(
        &controls,
        "Taille des données (Mio, hors mémoire supplémentaire)",
        &size,
    );
    let pattern = gtk::DropDown::from_strings(&[
        gameshift_cpu::reporting::pattern_name(Pattern::Entities),
        gameshift_cpu::reporting::pattern_name(Pattern::Sequential),
        gameshift_cpu::reporting::pattern_name(Pattern::Random),
        gameshift_cpu::reporting::pattern_name(Pattern::Dependent),
        gameshift_cpu::reporting::pattern_name(Pattern::Spatial),
        gameshift_cpu::reporting::pattern_name(Pattern::Mixed),
    ]);
    row(&controls, "Type de test", &pattern);
    let steps = gtk::SpinButton::with_range(1., 1000., 1.);
    steps.set_value(300.);
    let durations = crate::duration_ui::Controls::new(true);
    panel.append(&durations.root);
    let legacy = gtk::Switch::new();
    let historical = gtk::Expander::new(Some("Comparaison avec le protocole historique"));
    let fields = gtk::Box::new(gtk::Orientation::Vertical, 8);
    fields.append(&crate::context_help::heading("Nombre d’étapes fixe, sans calibration","Mode historique pour reproduire les anciens paramètres. Peut produire des répétitions très courtes ; les mêmes critères de fiabilité restent appliqués."));
    fields.append(&legacy);
    row(&fields, "Étapes de simulation mesurées", &steps);
    steps.set_sensitive(false);
    let fixed_steps = steps.clone();
    legacy.connect_active_notify(move |switch| fixed_steps.set_sensitive(switch.is_active()));
    historical.set_child(Some(&fields));
    controls.append(&historical);
    let counters = gtk::Switch::new();
    row(&controls, "Compteurs matériels CPU", &counters);
    let advanced = gtk::Expander::new(Some(
        "Configuration CPU · protocole, threads et options avancées",
    ));
    advanced.set_child(Some(&controls));
    panel.append(&advanced);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let start = gtk::Button::with_label("Mesurer le CPU");
    start.add_css_class("suggested-action");
    let stop = gtk::Button::with_label("Annuler");
    stop.set_sensitive(false);
    actions.append(&start);
    actions.append(&stop);
    panel.append(&actions);
    let status = label(
        "Profil Standard par défaut, calibration automatique et données reproductibles. Aucun réglage système modifié.",
    );
    panel.append(&status);
    panel.append(&crate::benchmark_dashboard::telemetry(active.clone()));
    let progress_bar = gtk::ProgressBar::new();
    panel.append(&progress_bar);
    let output = label("");
    let details = gtk::Expander::new(Some("Rapport CPU · mesures et variation"));
    details.set_child(Some(&output));
    details.set_expanded(true);
    panel.append(&details);
    panel.append(&crate::context_help::guide(&[
        ("Temps moyen par étape de simulation", "Durée totale observée d’une étape, y compris la coordination et les attentes. Plus elle est faible, plus ce travail précis est traité rapidement. Ne pas additionner les durées des threads exécutés en parallèle."),
        ("Débit de calcul", "Nombre d’objets traités par seconde. Une valeur plus élevée indique davantage de travail accompli pour ce type de test ; ce n’est pas un nombre d’images par seconde."),
        ("Accélération et efficacité du parallélisme", "L’accélération divise le temps à un thread par le temps au nombre de threads choisi. ×2 signifie deux fois plus rapide pour le même travail. L’efficacité divise cette accélération par le nombre de threads ; elle peut diminuer lorsque leur nombre augmente."),
        ("Variation entre les mesures", crate::context_help::VARIATION),
        ("Résultat à confirmer", "Le détail indique la cause : répétition trop courte, nombre de répétitions insuffisant, variation excessive ou référence à un thread incertaine. Cela ne signale pas une panne matérielle. Même stable, une mesure ne prouve pas à elle seule un gain lié à une optimisation."),
        ("Préparation du test", "Les premières étapes servent à préparer les données et l’exécution. Elles sont exclues des résultats. La même préparation est utilisée pour chaque répétition ; elle ne garantit pas une température identique."),
        ("Cache mémoire L3 et 3D V-Cache", "Le graphique de cache affiche le débit par objet, car une étape traite davantage d’objets quand les données grandissent. Le cache L3 est une mémoire rapide du processeur. Faire varier la taille des données aide à observer son influence, sans désactiver le 3D V-Cache. Les tailles et la mémoire supplémentaire réellement comptabilisées figurent dans le rapport."),
        ("Mémoire physique du processus (RSS)", "Mémoire résidente du processus à la fin du test, incluant les mesures et, dans l’application, l’interface. Ce n’est pas la taille des seules données testées."),
    ]));
    let chart = gtk::DrawingArea::new();
    chart.set_content_height(220);
    chart.set_hexpand(true);
    panel.append(&chart);
    let axes = label("X : threads CPU utilisés · Y : temps moyen par étape de simulation (ms)");
    panel.append(&axes);
    let data = Rc::new(RefCell::new(Vec::<(f64, f64)>::new()));
    let cache_curve = Rc::new(Cell::new(false));
    let drawn_cache = cache_curve.clone();
    let points = data.clone();
    chart.set_draw_func(move |_, cr, width, height| {
        let points = points.borrow();
        if points.is_empty() {
            return;
        }
        let max_x = points.iter().map(|p| p.0).fold(1., f64::max);
        let max_y = points.iter().map(|p| p.1).fold(0.001, f64::max) * 1.15;
        cr.set_source_rgb(0.45, 0.48, 0.52);
        cr.set_line_width(1.);
        cr.move_to(55., 12.);
        cr.line_to(55., height as f64 - 32.);
        cr.line_to(width as f64 - 12., height as f64 - 32.);
        let _ = cr.stroke();
        cr.set_source_rgb(0.2, 0.55, 0.95);
        cr.set_line_width(2.);
        for (index, &(x, y)) in points.iter().enumerate() {
            let px = 55. + x / max_x * (width as f64 - 80.);
            let py = height as f64 - 32. - y / max_y * (height as f64 - 52.);
            if index == 0 {
                cr.move_to(px, py);
            } else {
                cr.line_to(px, py);
            }
        }
        let _ = cr.stroke();
        for &(x, y) in points.iter() {
            let px = 55. + x / max_x * (width as f64 - 80.);
            let py = height as f64 - 32. - y / max_y * (height as f64 - 52.);
            cr.set_source_rgb(0.2, 0.55, 0.95);
            cr.arc(px, py, 4., 0., std::f64::consts::TAU);
            let _ = cr.fill();
            cr.set_source_rgb(0.45, 0.48, 0.52);
            cr.move_to(px, height as f64 - 13.);
            let _ = cr.show_text(&format!("{x:.0}"));
            cr.move_to(px + 5., py);
            let _ = cr.show_text(&if drawn_cache.get() {
                format!("{y:.2}")
            } else {
                format!("{y:.2} ms")
            });
        }
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let stop_cancel = cancel.clone();
    stop.connect_clicked(move |_| stop_cancel.store(true, Ordering::Relaxed));
    let close_cancel = cancel.clone();
    panel.connect_unrealize(move |_| close_cancel.store(true, Ordering::Relaxed));
    start.connect_clicked(move |start| {
        if crate::amd_controls::awaiting_confirmation(){status.set_text("Attendre la fin de l’application du profil GPU avant le benchmark.");return;}
        if active.get(){status.set_text("Un autre test est en cours dans le laboratoire. Attendez sa fin ou annulez-le avant de lancer celui-ci.");return;}
        active.set(true);
        progress_bar.set_fraction(0.);progress_bar.set_text(Some("Initialisation"));
        cancel.store(false, Ordering::Relaxed);
        let scaling = mode.selected() == 1;
        let cache = mode.selected() == 2;cache_curve.set(cache);data.borrow_mut().clear();chart.queue_draw();
        axes.set_text(if cache { "X : taille des données (Mio, hors mémoire supplémentaire) · Y : débit de calcul (millions d’objets traités/s)" }
            else { "X : threads CPU utilisés · Y : temps moyen par étape de simulation (ms)" });
        let duration=(!legacy.is_active()).then(||gameshift_cpu::calibrated::CpuDuration{profile:durations.effective_profile(),target_ms:(durations.measurement.value()*1000.).round() as u64,preparation_ms:(durations.preparation.value()*1000.).round() as u64});
        let config = Config { workers: if scaling { Topology::detect().scaling_steps() } else { vec![workers.value_as_int() as usize] },
            sizes_mib: if cache { vec![4,8,16,32,48,64,80,96,128,192,256] } else { vec![size.value_as_int() as u64] },
            pattern: match pattern.selected() { 1 => Pattern::Sequential, 2 => Pattern::Random, 3 => Pattern::Dependent, 4 => Pattern::Spatial, 5 => Pattern::Mixed, _ => Pattern::Entities },
            duration, repetitions:durations.repetitions.value_as_int() as usize, measured_steps: steps.value_as_int() as usize, hardware_counters: counters.is_active(), ..Config::default() };
        start.set_sensitive(false); controls.set_sensitive(false);durations.root.set_sensitive(false); stop.set_sensitive(true);
        status.set_text("Mesures en cours… Évitez les autres activités qui sollicitent le processeur ou la carte graphique. Annulation possible entre deux étapes.");
        let (tx, rx) = mpsc::channel(); let flag = cancel.clone();
        let progress:gameshift_core::duration::ProgressSink=Default::default();let observed=progress.clone();
        std::thread::spawn(move || {
            let guard=gameshift_control::benchmark::BenchmarkGuard::begin();
            let result = guard.and_then(|_guard|(if config.duration.is_some(){gameshift_cpu::calibrated::run(config,&flag,&observed)}else{run_cancellable(config,&flag)}).map(|report| {
                let exported = export(&report);
                let rendered = summary(&report);
                let points = report.cases.iter().map(|c| (if cache { c.size_mib as f64 } else { c.workers as f64 }, if cache {c.entities_per_second/1_000_000.}else{c.mean_step_ms})).collect::<Vec<_>>();
                (rendered, points, exported)
            }));
            let _ = tx.send(result);
        });
        let start = start.downgrade(); let controls = controls.downgrade(); let stop = stop.downgrade();
        let timing_controls=durations.root.downgrade();
        let progress_bar=progress_bar.downgrade();
        let status = status.downgrade(); let output = output.downgrade(); let chart = chart.downgrade(); let data = data.clone();
        let active=active.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let received = match rx.try_recv() {
                Ok(value) => value,
                Err(mpsc::TryRecvError::Empty) => {if let (Some(bar),Some(label),Some(p))=(progress_bar.upgrade(),status.upgrade(),progress.lock().ok().and_then(|slot|slot.clone())){crate::duration_ui::show_progress(&bar,&label,&p);}return glib::ControlFlow::Continue;},
                Err(_) => Err("Le calcul CPU s’est arrêté sans résultat".into()),
            };
            if let Some(bar)=progress_bar.upgrade(){bar.set_fraction(if received.is_ok(){1.}else{0.});bar.set_text(Some(if received.is_ok(){"Terminé"}else{"Interrompu"}));}
            if let Some(timing)=timing_controls.upgrade(){timing.set_sensitive(true);}
            active.set(false);
            if let Some(w) = start.upgrade() { w.set_sensitive(true); }
            if let Some(w) = controls.upgrade() { w.set_sensitive(true); }
            if let Some(w) = stop.upgrade() { w.set_sensitive(false); }
            match received {
                Ok((rendered, points, exported)) => {
                    if let Some(w) = status.upgrade() { w.set_text(&match exported { Ok(path) => format!("Terminé · exports : {}", path.display()), Err(e) => format!("Mesuré, mais export impossible : {e}") }); }
                    if let Some(w) = output.upgrade() { w.set_text(&rendered); }
                    *data.borrow_mut() = points;
                    if let Some(w) = chart.upgrade() { w.set_tooltip_text(Some(if cache { "X : taille des données (Mio) ; Y : millions d’objets traités/s" } else { "X : threads CPU utilisés ; Y : temps moyen par étape de simulation" })); w.queue_draw(); }
                }
                Err(e) => if let Some(w) = status.upgrade() { w.set_text(&gameshift_cpu::reporting::error_message(&e)); },
            }
            glib::ControlFlow::Break
        });
    });
    panel
}
fn summary(report: &Report) -> String {
    report.text()
}

fn export(report: &Report) -> Result<std::path::PathBuf, String> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
        })
        .ok_or("Dossier d’état utilisateur introuvable")?;
    let root = root.join("gameshift/cpu-runs");
    export_in(report, &root)
}
fn export_in(report: &Report, root: &std::path::Path) -> Result<std::path::PathBuf, String> {
    use std::{
        io::Write,
        time::{SystemTime, UNIX_EPOCH},
    };
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let target = root.join(format!("{id}-{}", std::process::id()));
    let staging = root.join(format!("{id}-{}.partial", std::process::id()));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        let mut json =
            std::fs::File::create(staging.join("result.json")).map_err(|e| e.to_string())?;
        serde_json::to_writer_pretty(&mut json, report).map_err(|e| e.to_string())?;
        json.flush().map_err(|e| e.to_string())?;
        std::fs::write(staging.join("result.txt"), summary(report)).map_err(|e| e.to_string())?;
        let mut csv = String::from(
            "protocol,size_mib,workers,repetition,step,critical_wall_ns,worker_cpu_ns,checksum",
        );
        if report.schema_version >= 2 {
            csv.push_str(",schema_version,duration_profile,measured_steps,preparation_steps,engine_version,protocol_version,pattern,seed,fixed_step_ns,target_ms,repetitions,hardware_counters,preparation_target_ms,preparation_actual_ms,measurement_wall_actual_ms");
        }
        csv.push('\n');
        for case in &report.cases {
            for (r, samples) in case.repetitions.iter().enumerate() {
                for (s, sample) in samples.iter().enumerate() {
                    csv.push_str(&format!(
                        "{},{},{},{},{},{},{},{}{}\n",
                        report.protocol,
                        case.size_mib,
                        case.workers,
                        r,
                        s,
                        sample.critical_wall_ns,
                        sample
                            .cumulative_worker_cpu_ns
                            .map(|n| n.to_string())
                            .unwrap_or_default(),
                        sample.checksum,
                        if let (Some(duration), Some(cal)) =
                            (&report.config.duration, &case.calibration)
                        {
                            format!(
                                ",{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                                report.schema_version,
                                serde_json::to_string(&duration.profile)
                                    .unwrap_or_default()
                                    .trim_matches('"'),
                                cal.measured_steps,
                                cal.preparation_steps,
                                report.engine_version,
                                report.protocol_version,
                                serde_json::to_string(&report.config.pattern)
                                    .unwrap_or_default()
                                    .trim_matches('"'),
                                report.config.seed,
                                report.config.fixed_step_ns,
                                duration.target_ms,
                                report.config.repetitions,
                                report.config.hardware_counters,
                                duration.preparation_ms,
                                cal.preparation_actual_ms
                                    .get(r)
                                    .map(|n| n.to_string())
                                    .unwrap_or_default(),
                                cal.measurement_wall_ms
                                    .get(r)
                                    .map(|n| n.to_string())
                                    .unwrap_or_default()
                            )
                        } else {
                            String::new()
                        }
                    ));
                }
            }
        }
        std::fs::write(staging.join("samples.csv"), csv).map_err(|e| e.to_string())?;
        std::fs::rename(&staging, &target).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result.map(|_| target)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calibrated_csv_preserves_columns_and_adds_protocol_metadata() {
        let report = gameshift_cpu::runner::run(Config {
            sizes_mib: vec![1],
            repetitions: 2,
            duration: Some(gameshift_cpu::calibrated::CpuDuration {
                profile: gameshift_core::duration::DurationProfile::Custom,
                target_ms: 100,
                preparation_ms: 0,
            }),
            ..Config::default()
        })
        .unwrap();
        let root = std::env::temp_dir().join(format!(
            "gameshift-calibrated-export-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = export_in(&report, &root).unwrap();
        let csv = std::fs::read_to_string(path.join("samples.csv")).unwrap();
        let mut lines = csv.lines();
        let header = lines.next().unwrap();
        assert!(header.starts_with(
            "protocol,size_mib,workers,repetition,step,critical_wall_ns,worker_cpu_ns,checksum,"
        ));
        assert!(header.contains("engine_version"));
        let count = header.split(',').count();
        assert!(lines.all(|line| line.split(',').count() == count));
        let decoded: Report =
            serde_json::from_slice(&std::fs::read(path.join("result.json")).unwrap()).unwrap();
        assert_eq!(decoded.schema_version, 2);
        assert_eq!(decoded.protocol_version, 2);
        assert_eq!(
            decoded.cases[0]
                .calibration
                .as_ref()
                .unwrap()
                .measured_steps,
            decoded.cases[0].repetitions[0].len()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn exports_complete_cpu_samples_without_overwriting_a_previous_run() {
        let report = gameshift_cpu::runner::run(Config {
            sizes_mib: vec![1],
            measured_steps: 2,
            warmup_steps: 1,
            repetitions: 1,
            ..Config::default()
        })
        .unwrap();
        let root = std::env::temp_dir().join(format!(
            "gameshift-cpu-export-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = export_in(&report, &root).unwrap();
        let original = std::fs::read(first.join("result.json")).unwrap();
        let second = export_in(&report, &root).unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(first.join("result.json")).unwrap(), original);
        let decoded: Report = serde_json::from_slice(&original).unwrap();
        assert_eq!(decoded.cases[0].repetitions[0].len(), 2);
        assert_eq!(
            std::fs::read_to_string(first.join("samples.csv"))
                .unwrap()
                .lines()
                .count(),
            3
        );
        assert!(std::fs::read_to_string(first.join("result.txt"))
            .unwrap()
            .contains("Résultat à confirmer"));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }
}
