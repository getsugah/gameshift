//! Launch, cancel and inspect real raster runs from the existing laboratory page.
use adw::prelude::*;
use gameshift_bench::{RunConfig, RunResult, VulkanReport};
use gtk::glib;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

fn label(value: &str, class: &str) -> gtk::Label {
    let w = gtk::Label::new(Some(value));
    w.add_css_class(class);
    w.set_xalign(0.0);
    w.set_wrap(true);
    w.set_selectable(true);
    w
}

fn results(box_: &gtk::Box, result: &RunResult, path: Result<std::path::PathBuf, String>) {
    while let Some(child) = box_.first_child() {
        box_.remove(&child);
    }
    box_.append(&label("Résultats mesurés", "gs-section-title"));
    let frames = result
        .repetitions
        .iter()
        .map(|r| r.statistics.frames as f64)
        .sum::<f64>();
    let ms = result
        .repetitions
        .iter()
        .map(|r| r.statistics.measured_ms)
        .sum::<f64>();
    if ms > 0. {
        box_.append(&label(
            &format!(
                "{:.1} FPS · {:.1} s mesurées",
                frames * 1000. / ms,
                ms / 1000.
            ),
            "gs-gpu-value",
        ));
    }
    box_.append(&label(
        &format!(
            "Variation : {} · moteur {}",
            result
                .repetition_cv_percent
                .map(|v| format!("{v:.2} %"))
                .unwrap_or("indisponible".into()),
            result.scene_version
        ),
        "gs-section-title",
    ));

    box_.append(&label(
        &format!(
            "{} · rendu {} × {} · {} · Anticrénelage (MSAA) {}×",
            result.gpu_name,
            result.actual_width,
            result.actual_height,
            result.present_mode,
            result.msaa_samples
        ),
        "gs-card-detail",
    ));
    box_.append(&label(
        &format!(
            "Fenêtre de présentation : {} × {}",
            result.presentation_width, result.presentation_height
        ),
        "gs-card-detail",
    ));
    if let Some(name) = result
        .environment
        .gpu_profile_before
        .as_ref()
        .and_then(|v| v["active_profile"]["name"].as_str())
    {
        box_.append(&label(
            &format!("Profil GPU observé avant mesure : {name}"),
            "gs-card-detail",
        ));
    }
    if let Some(average) = result.gpu_utilization.average_percent {
        box_.append(&label(
            &format!(
                "Occupation globale du GPU : {average:.1} % en moyenne · pic {} % · {} relevés",
                result.gpu_utilization.maximum_percent.unwrap_or(0),
                result.gpu_utilization.samples.len()
            ),
            "gs-section-title",
        ));
        box_.append(&label("Ce capteur inclut les autres applications. Un jeu en parallèle empêche d’attribuer cette charge au seul benchmark.","gs-card-detail"));
    } else {
        box_.append(&label(
            "Occupation GPU : indisponible sur ce capteur. La saturation n’est pas confirmée.",
            "gs-card-detail",
        ));
    }
    for (index, run) in result.repetitions.iter().enumerate() {
        let s = &run.statistics;
        let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
        card.add_css_class("gs-card");
        card.append(&label(
            &format!("Répétition {} · {} images calculées", index + 1, s.frames),
            "gs-card-label",
        ));
        card.append(&label(
            &format!(
                "{:.1} images/s (FPS) · temps moyen par image : {:.2} ms",
                s.average_fps, s.average_frametime_ms
            ),
            "gs-card-value",
        ));
        card.append(&label(
            &format!(
                "P95 {:.2} ms · P99 {:.2} ms · P99,9 {:.2} ms",
                s.p95_ms, s.p99_ms, s.p999_ms
            ),
            "gs-card-detail",
        ));
        card.append(&label(
            &format!(
                "Images les plus lentes : 1 % low {:.1} FPS · 0,1 % low {:.1} FPS",
                s.one_percent_low_fps, s.point_one_percent_low_fps
            ),
            "gs-card-detail",
        ));
        let gpu = s
            .average_gpu_execution_ms
            .map(|n| format!("{n:.3} ms"))
            .unwrap_or_else(|| "Indisponible".into());
        let cpu = s
            .average_cpu_work_ms
            .map(|n| format!("{n:.3} ms"))
            .unwrap_or_else(|| "Indisponible".into());
        card.append(&label(
            &format!("Préparation et soumission CPU : {cpu} · temps de calcul GPU : {gpu}"),
            "gs-card-detail",
        ));
        box_.append(&card);
    }
    if let Some(cv) = result.repetition_cv_percent {
        box_.append(&label(
            &format!("Variation entre les mesures : {cv:.2} % (coefficient de variation)"),
            "gs-card-detail",
        ));
    }
    if let Some(timing) = &result.duration {
        box_.append(&label(
            &format!(
                "Profil {} · protocole {} · préparation identique avant chaque passage",
                timing.profile.label(),
                timing.protocol_id
            ),
            "gs-card-detail",
        ));
        if let (Some(min), Some(max)) = (timing.minimum_window_fps, timing.maximum_window_fps) {
            box_.append(&label(&format!("FPS par fenêtres d’au moins 1 s : minimum {min:.1}, maximum {max:.1}. La dernière fenêtre partielle est exclue des extrema."),"gs-card-detail"));
        }
        for pass in &timing.passes {
            box_.append(&label(
                &format!(
                    "Passage {} : préparation réelle {:.2} s · mesure réelle {:.2} s",
                    pass.repetition,
                    pass.preparation_ms / 1000.,
                    pass.measurement_wall_ms / 1000.
                ),
                "gs-card-detail",
            ));
        }
        box_.append(&label("Les séries par seconde et les capteurs GPU disponibles sont conservés dans le JSON. Attente de présentation séparée : indisponible ; aucune durée GPU n’est déduite du travail CPU.","gs-card-detail"));
    }
    if let Some(first) = result.repetitions.first() {
        box_.append(&label("Temps par image · répétition 1", "gs-section-title"));
        let points: Vec<_> = first.samples.iter().map(|s| s.frame_interval_ms).collect();
        let plot = gtk::DrawingArea::new();
        plot.set_content_height(180);
        plot.set_hexpand(true);
        plot.add_css_class("gs-technical");
        plot.set_draw_func(move |_, ctx, width, height| {
            if points.is_empty() {
                return;
            }
            let left = 52.0;
            let top = 18.0;
            let w = (f64::from(width) - left - 12.0).max(1.0);
            let h = (f64::from(height) - top - 26.0).max(1.0);
            let maximum = points.iter().copied().fold(0.0_f64, f64::max).max(1.0) * 1.05;
            ctx.set_font_size(10.0);
            for step in 0..=4 {
                let y = top + h * f64::from(step) / 4.0;
                ctx.set_source_rgba(0.5, 0.5, 0.5, 0.25);
                ctx.move_to(left, y);
                ctx.line_to(left + w, y);
                let _ = ctx.stroke();
                ctx.set_source_rgb(0.5, 0.5, 0.5);
                ctx.move_to(2.0, y + 3.0);
                let _ = ctx.show_text(&format!("{:.1}", maximum * (1.0 - f64::from(step) / 4.0)));
            }
            let bucket = points.len().div_ceil(w as usize).max(1);
            let maxima: Vec<f64> = points
                .chunks(bucket)
                .map(|c| c.iter().copied().fold(0.0, f64::max))
                .collect();
            ctx.set_source_rgb(0.2, 0.5, 0.95);
            ctx.set_line_width(1.4);
            for (i, p) in maxima.iter().enumerate() {
                let x = left + w * i as f64 / (maxima.len() - 1).max(1) as f64;
                let y = top + h * (1.0 - p / maximum);
                if i == 0 {
                    ctx.move_to(x, y);
                } else {
                    ctx.line_to(x, y);
                }
            }
            let _ = ctx.stroke();
            ctx.set_source_rgb(0.5, 0.5, 0.5);
            ctx.move_to(left, top + h + 19.0);
            let _ = ctx.show_text("Images, dans l’ordre de calcul · ordonnée en ms");
        });
        box_.append(&plot);
        box_.append(&label("Affichage réduit par maximum de tranche pour conserver les pics. Chaque image calculée figure dans le CSV.","gs-card-detail"));
    }
    for warning in &result.warnings {
        box_.append(&label(
            &gameshift_bench::display_warning(warning),
            "gs-card-detail",
        ));
    }
    box_.append(&crate::context_help::guide(&[
        ("Images par seconde (FPS)", "Nombre moyen d’images calculées par seconde. Plus il est élevé, plus le rendu est rapide pour ce protocole. Il ne correspond pas nécessairement au nombre d’images affichées par l’écran."),
        ("Temps par image", crate::context_help::FRAME_TIME),
        ("P95, P99 et P99,9", "Durées sous lesquelles se trouvent respectivement 95 %, 99 % et 99,9 % des intervalles mesurés, selon le rang supérieur. Elles renseignent sur les images lentes ; plus elles sont faibles, mieux c’est. Les extrêmes demandent beaucoup d’images pour être représentatifs."),
        ("Images les plus lentes : 1 % low et 0,1 % low", "Débit correspondant au temps moyen des 1 % ou 0,1 % d’images les plus lentes, en FPS. Le nombre d’images est arrondi au supérieur. Plus ce débit est élevé, mieux c’est. Ce ne sont pas les FPS minimums ni simplement l’inverse de P99."),
        ("Temps CPU et temps de calcul GPU", "La préparation et soumission CPU mesure une partie du travail du processeur, pas son activité totale. Le temps de calcul GPU provient des horodatages Vulkan quand ils sont disponibles. Ces durées sont distinctes du temps total par image."),
        ("Variation entre les mesures", crate::context_help::VARIATION),
        ("Occupation globale du GPU", "Activité de toute la carte, y compris les autres applications. Un pourcentage élevé ne prouve pas que le test seul sature le GPU. Fermez les charges concurrentes pour établir une référence."),
        ("Mode d’affichage Vulkan", "FIFO attend la synchronisation avec l’écran et peut plafonner les images par seconde. MAILBOX peut remplacer une image calculée avant affichage. IMMEDIATE n’attend pas cette synchronisation et peut produire des déchirures. Le mode est enregistré ; comparez le même mode."),
        ("Vérifications Vulkan", "Contrôles de développement détectant des usages incorrects de Vulkan. Ils ajoutent du travail au CPU : comparez des résultats obtenus avec le même état des vérifications. Un résultat à confirmer ne signifie pas que ces contrôles ont détecté une panne."),
        ("Préparation du test", "Chaque répétition utilise la durée de préparation du profil, exclue des statistiques, puis la durée de mesure choisie. Gardez la fenêtre visible. Cette préparation ne garantit pas une température identique entre les tests."),
    ]));
    match path {
        Ok(path) => {
            box_.append(&label("Exports enregistrés", "gs-section-title"));
            let text_path = path.with_extension("txt");
            box_.append(&label(&text_path.display().to_string(), "gs-source"));
            let open_report = gtk::Button::with_label("Ouvrir le rapport texte");
            open_report.set_halign(gtk::Align::Start);
            let uri = gtk::gio::File::for_path(&text_path).uri();
            let error_label = label("", "gs-card-detail");
            let weak = error_label.downgrade();
            open_report.connect_clicked(move |_| {
                if let Err(error) = gtk::gio::AppInfo::launch_default_for_uri(
                    &uri,
                    None::<&gtk::gio::AppLaunchContext>,
                ) {
                    if let Some(label) = weak.upgrade() {
                        label.set_text(&format!("Impossible d’ouvrir le rapport : {error}"));
                    }
                }
            });
            box_.append(&open_report);
            box_.append(&error_label);

            box_.append(&label(&path.display().to_string(), "gs-source"));
            box_.append(&label(
                &path.with_extension("csv").display().to_string(),
                "gs-source",
            ));
            if let Some(directory) = path.parent() {
                let uri = gtk::gio::File::for_path(directory).uri();
                let open = gtk::Button::with_label("Ouvrir le dossier des résultats");
                open.set_halign(gtk::Align::Start);
                let error_label = label("", "gs-card-detail");
                let weak = error_label.downgrade();
                open.connect_clicked(move |_| {
                    if let Err(error) = gtk::gio::AppInfo::launch_default_for_uri(
                        &uri,
                        None::<&gtk::gio::AppLaunchContext>,
                    ) {
                        if let Some(label) = weak.upgrade() {
                            label.set_text(&format!("Impossible d’ouvrir le dossier : {error}"));
                        }
                    }
                });
                box_.append(&open);
                box_.append(&error_label);
            }
        }
        Err(e) => box_.append(&label(
            &format!("Résultat affiché, mais export incomplet : {e}"),
            "gs-card-detail",
        )),
    }
}

pub fn panel(
    selection: &gtk::DropDown,
    analyze: &gtk::Button,
    report: Rc<RefCell<Option<VulkanReport>>>,
    active: Rc<Cell<bool>>,
) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 18);
    let controls = gtk::Box::new(gtk::Orientation::Vertical, 16);
    controls.add_css_class("gs-card");
    controls.append(&label("Scène 3D · Aurelia", "gs-section-title"));
    controls.append(&label("Place architecturale, sculpture métallique, façades vitrées, ombres et ciel atmosphérique. Les durées et répétitions sont choisies ci-dessous. Gardez la fenêtre de rendu visible.","gs-card-detail"));
    let tuning = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let advanced = gtk::Expander::new(Some("Résolution, complexité et options avancées"));
    advanced.set_child(Some(&tuning));
    controls.append(&advanced);
    let resolution =
        gtk::DropDown::from_strings(&["1280 × 720", "1920 × 1080", "3840 × 2160 · 4K"]);
    let load = gtk::DropDown::from_strings(&[
        "Légère · 16 × 16 bâtiments",
        "Standard · 24 × 24 bâtiments",
        "Élevée · 40 × 40 bâtiments",
        "Très élevée · 48 × 48 bâtiments",
    ]);
    load.set_selected(1);
    resolution.set_selected(1);
    let quality = gtk::DropDown::from_strings(&[
        "Anticrénelage (MSAA) 4×",
        "Anticrénelage (MSAA) 8×",
        "Anticrénelage (MSAA) 16×",
    ]);
    quality.set_selected(1);
    tuning.append(&crate::context_help::heading("Résolution interne de rendu", "Nombre de pixels calculés pour chaque image, indépendant de la taille de la fenêtre. Une résolution plus élevée augmente généralement le travail GPU. Comparez la même résolution."));
    crate::context_help::describe(&resolution,"Nombre de pixels calculés par image ; une valeur élevée augmente généralement le travail GPU.");
    tuning.append(&resolution);
    tuning.append(&crate::context_help::heading("Complexité de la scène", "Nombre de bâtiments dans la scène. Augmenter cette valeur ajoute de la géométrie et du travail au rendu. Comparez exactement le même réglage ; ce n’est pas un type de test CPU."));
    crate::context_help::describe(
        &load,
        "Nombre de bâtiments ; augmente le travail du rendu. Garder le même réglage pour comparer.",
    );
    tuning.append(&load);
    tuning.append(&crate::context_help::heading(
        "Anticrénelage (MSAA) demandé",
        crate::context_help::MSAA,
    ));
    crate::context_help::describe(&quality, crate::context_help::MSAA);
    tuning.append(&quality);
    tuning.append(&label("Le niveau pris en charge est vérifié au lancement. Le rendu interne reste indépendant de la taille de la fenêtre, jusqu’à la 4K. Les résolutions élevées et la complexité très élevée permettent d’augmenter le travail GPU.","gs-card-detail"));
    let require_validation =
        gtk::CheckButton::with_label("Exiger les couches de validation Vulkan (développement)");
    tuning.append(&require_validation);
    let durations = crate::duration_ui::Controls::new(false);
    controls.append(&durations.root);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let start = gtk::Button::with_label("Lancer le test 3D");
    start.add_css_class("suggested-action");
    start.set_sensitive(false);
    let cancel = gtk::Button::with_label("Annuler");
    cancel.set_sensitive(false);
    actions.append(&start);
    actions.append(&cancel);
    controls.append(&actions);
    let status=label("Détectez puis sélectionnez un GPU matériel. Les vérifications Vulkan sont facultatives ; leur état réel est conservé dans le résultat.","gs-card-detail");
    controls.append(&status);
    panel.append(&crate::benchmark_dashboard::telemetry(active.clone()));
    let progress_bar = gtk::ProgressBar::new();
    controls.append(&progress_bar);
    panel.append(&controls);
    let output = gtk::Box::new(gtk::Orientation::Vertical, 18);
    panel.append(&output);
    let running = Rc::new(Cell::new(false));
    let cancellation: Rc<RefCell<Option<Arc<AtomicBool>>>> = Rc::new(RefCell::new(None));
    let cancellation_for_button = cancellation.clone();
    cancel.connect_clicked(move |button| {
        if let Some(flag) = cancellation_for_button.borrow().as_ref() {
            flag.store(true, Ordering::Relaxed);
            button.set_sensitive(false);
        }
    });
    let start_weak = start.downgrade();
    let selection_weak = selection.downgrade();
    let check_report = report.clone();
    let check_running = running.clone();
    let check_validation = require_validation.clone();
    let update: Rc<dyn Fn()> = Rc::new(move || {
        if let (Some(start), Some(selection)) = (start_weak.upgrade(), selection_weak.upgrade()) {
            let ready = check_report.borrow().as_ref().is_some_and(|r| {
                (!check_validation.is_active() || r.validation_enabled)
                    && r.gpus.get(selection.selected() as usize).is_some_and(|g| {
                        g.raster_candidate()
                            && g.device_uuid.is_some()
                            && g.kind != "Rendu logiciel (CPU)"
                    })
            });
            start.set_sensitive(ready && selection.is_sensitive() && !check_running.get());
        }
    });
    let validation_update = update.clone();
    require_validation.connect_toggled(move |_| validation_update());
    let update_selection = update.clone();
    selection.connect_selected_notify(move |_| update_selection());
    let update_sensitive = update.clone();
    selection.connect_sensitive_notify(move |_| update_sensitive());
    let selection = selection.downgrade();
    let analyze = analyze.downgrade();
    let status = status.downgrade();
    let output = output.downgrade();
    let resolution_weak = resolution.downgrade();
    let load_weak = load.downgrade();
    let quality_weak = quality.downgrade();
    let cancel_weak = cancel.downgrade();
    start.connect_clicked(move|start|{
        let(Some(selection),Some(analyze),Some(status),Some(output),Some(resolution),Some(load),Some(quality),Some(cancel))=(selection.upgrade(),analyze.upgrade(),status.upgrade(),output.upgrade(),resolution_weak.upgrade(),load_weak.upgrade(),quality_weak.upgrade(),cancel_weak.upgrade())else{return;};
        if running.get(){return;}
        let gpu=report.borrow().as_ref().and_then(|r|r.gpus.get(selection.selected()as usize)).cloned();
        let Some(gpu)=gpu else{return;};let Some(uuid)=gpu.device_uuid else{status.set_text("Identifiant unique du GPU (UUID) indisponible : le test ne peut pas sélectionner cette carte de façon fiable.");return;};
        if crate::amd_controls::awaiting_confirmation(){status.set_text("Attendre la fin de l’application du profil GPU avant le benchmark.");return;}
        if active.get(){status.set_text("Un autre test est en cours dans le laboratoire. Attendez sa fin ou annulez-le.");return;}active.set(true);
        let(width,height)=match resolution.selected(){0=>(1280,720),2=>(3840,2160),_=>(1920,1080)};
        let grid_side=match load.selected(){0=>16,2=>40,3=>48,_=>24};
        let requested_msaa=match quality.selected(){0=>4,2=>16,_=>8};
        let config=RunConfig{gpu_uuid:uuid,width,height,grid_side,requested_msaa,warmup_seconds:durations.preparation.value_as_int() as u32,measure_seconds:durations.measurement.value_as_int() as u32,repetitions:durations.repetitions.value_as_int() as u32,require_validation:require_validation.is_active(),duration_profile:Some(durations.effective_profile())};
        progress_bar.set_fraction(0.);progress_bar.set_text(Some("Initialisation"));
        let estimated_seconds=(config.warmup_seconds+config.measure_seconds)*config.repetitions;durations.root.set_sensitive(false);
        let flag=Arc::new(AtomicBool::new(false));*cancellation.borrow_mut()=Some(flag.clone());
        running.set(true);start.set_sensitive(false);cancel.set_sensitive(true);analyze.set_sensitive(false);selection.set_sensitive(false);resolution.set_sensitive(false);load.set_sensitive(false);quality.set_sensitive(false);
        while let Some(child)=output.first_child(){output.remove(&child);}
        status.set_text("Initialisation du test… La fenêtre de rendu va s’ouvrir.");
        let(sender,receiver)=std::sync::mpsc::sync_channel(1);
        let progress:gameshift_core::duration::ProgressSink=Default::default();let observed=progress.clone();
        let launched=std::thread::Builder::new().name("gameshift-bench-supervisor".into()).spawn(move||{
            let result=crate::bench_process::execute_with_progress(config,flag,observed).map(|result|{let path=crate::bench_process::save(&result);(result,path)});
            let _=sender.send(result);
        });
        if let Err(e)=launched{
            active.set(false);durations.root.set_sensitive(true);running.set(false);status.set_text(&format!("Impossible de lancer le test : {e}"));analyze.set_sensitive(true);selection.set_sensitive(true);resolution.set_sensitive(true);load.set_sensitive(true);quality.set_sensitive(true);cancel.set_sensitive(false);update();return;
        }
        let running=running.clone();let update=update.clone();let cancellation=cancellation.clone();
        let status=status.downgrade();let analyze=analyze.downgrade();let selection=selection.downgrade();let output=output.downgrade();let resolution=resolution.downgrade();let load=load.downgrade();let quality=quality.downgrade();let cancel=cancel.downgrade();
        let timing_controls=durations.root.downgrade();let progress_bar=progress_bar.downgrade();
        let active=active.clone();
        let started=Instant::now();
        glib::timeout_add_local(Duration::from_millis(150),move||{
            let(Some(status),Some(analyze),Some(selection),Some(output),Some(resolution),Some(load),Some(quality),Some(cancel))=(status.upgrade(),analyze.upgrade(),selection.upgrade(),output.upgrade(),resolution.upgrade(),load.upgrade(),quality.upgrade(),cancel.upgrade())else{
                if let Some(flag)=cancellation.borrow().as_ref(){flag.store(true,Ordering::Relaxed);}return glib::ControlFlow::Break;
            };
            let outcome=match receiver.try_recv(){
                Ok(outcome)=>outcome,
                Err(std::sync::mpsc::TryRecvError::Empty)=>{if let Some(bar)=progress_bar.upgrade(){if let Some(mut p)=progress.lock().ok().and_then(|slot|slot.clone()){p.elapsed_seconds=started.elapsed().as_secs_f64();crate::duration_ui::show_progress(&bar,&status,&p);}else{status.set_text(&format!("Initialisation · {} s écoulées · cible configurée : {estimated_seconds} s de rendu",started.elapsed().as_secs()));}}return glib::ControlFlow::Continue;}
                Err(std::sync::mpsc::TryRecvError::Disconnected)=>Err("Le superviseur a été interrompu".into()),
            };
            let completed=outcome.is_ok();
            match outcome{Ok((result,path))=>{status.set_text("Test terminé. La préparation du test est exclue des résultats.");results(&output,&result,path);},Err(error)=>status.set_text(&error)}
            if let Some(w)=timing_controls.upgrade(){w.set_sensitive(true);}
            if let Some(bar)=progress_bar.upgrade(){bar.set_fraction(if completed{1.}else{0.});bar.set_text(Some(if completed{"Terminé"}else{"Interrompu"}));}
            active.set(false);running.set(false);*cancellation.borrow_mut()=None;analyze.set_sensitive(true);selection.set_sensitive(true);resolution.set_sensitive(true);load.set_sensitive(true);quality.set_sensitive(true);cancel.set_sensitive(false);update();glib::ControlFlow::Break
        });
    });
    panel
}
