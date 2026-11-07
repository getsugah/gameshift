//! Distribution-neutral measurements and local report comparison, without GTK work off-thread.
use adw::prelude::*;
use gameshift_system_bench::{
    session::{SystemKind, MAX_REPORT_BYTES},
    store::{self, Entry},
    system::reporting,
};
use gtk::glib;
use std::os::unix::process::CommandExt;
use std::{
    cell::{Cell, RefCell},
    io::{BufRead, Read},
    process::{Command, Stdio},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
fn label(value: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(value));
    l.set_wrap(true);
    l.set_xalign(0.);
    l.set_selectable(true);
    l
}
type IndividualDisplay = (String, Vec<(String, String)>, String);
type ComparisonDisplay = (String, Vec<(String, Option<f64>)>);
enum Event {
    Progress(String, f64),
    Saved(Result<Entry, String>),
    Compared(Result<ComparisonDisplay, String>),
    Viewed(Result<IndividualDisplay, String>),
    Exported(Result<(), String>),
    Archived(Result<(), String>),
}
fn run(cancel: Arc<AtomicBool>, tx: mpsc::Sender<Event>, kind: &str) -> Result<Entry, String> {
    let mut executable = std::env::current_exe().map_err(|e| e.to_string())?;
    executable.set_file_name("gameshift-system-bench");
    run_program(&executable, cancel, tx, kind)
}
fn run_program(
    executable: &std::path::Path,
    cancel: Arc<AtomicBool>,
    tx: mpsc::Sender<Event>,
    kind: &str,
) -> Result<Entry, String> {
    let mut command = Command::new(executable);
    command
        .process_group(0)
        .args(["--evaluate", kind])
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let parent = std::process::id() as libc::pid_t;
    // The process also stops if GTK exits before its polling thread can cancel.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Auxiliaire de comparaison indisponible : {e}"))?;
    let stdout = child.stdout.take().ok_or("Sortie du test absente")?;
    let stderr = child.stderr.take().ok_or("Journal du test absent")?;
    let output = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut input = stdout;
        let mut buffer = [0; 16384];
        let mut exceeded = false;
        loop {
            let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            if bytes.len() + n > MAX_REPORT_BYTES as usize {
                exceeded = true;
            } else if !exceeded {
                bytes.extend_from_slice(&buffer[..n]);
            }
        }
        if exceeded {
            Err("Rapport trop volumineux".into())
        } else {
            Ok(bytes)
        }
    });
    let logs = std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stderr);
        let mut log = String::new();
        loop {
            let mut line = String::new();
            let n = reader.by_ref().take(65536).read_line(&mut line);
            match n {
                Ok(0) => break,
                Ok(_) => {
                    if let Some(json) = line.strip_prefix("GS_SYSTEM_PROGRESS ") {
                        if let Ok(p) = serde_json::from_str::<serde_json::Value>(json) {
                            if let (Some(phase), Some(fraction)) =
                                (p["phase"].as_str(), p["fraction"].as_f64())
                            {
                                let _ = tx.send(Event::Progress(phase.into(), fraction));
                            }
                        }
                    } else if log.len() < 65536 {
                        log.push_str(&line);
                    }
                }
                Err(_) => break,
            }
        }
        log
    });
    let outcome = loop {
        if cancel.load(Ordering::Relaxed) {
            // The helper inherits this dedicated group: cancel both processes.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            break child.wait().map_err(|e| e.to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                break Err(e.to_string());
            }
        }
    };
    if !matches!(&outcome,Ok(status) if status.success()) {
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    let output: Result<Vec<u8>, String> = output.join().map_err(|_| "Lecture interrompue")?;
    let log = logs.join().unwrap_or_default();
    let status = outcome?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Test annulé ; aucun résultat incomplet conservé.".into());
    }
    if !status.success() {
        return Err(format!("Évaluation interrompue : {log}"));
    }
    store::import_bytes(&store::root(), &output?)
}
fn view_report(id: String, reference: Option<String>, tx: mpsc::Sender<Event>) {
    std::thread::spawn(move || {
        let outcome = (|| {
            let root = store::root();
            let session = store::load(&root, &id)?;
            let mut text = reporting::individual(&session);
            if let Some(reference) = reference {
                if root
                    .join("references")
                    .join(format!("{reference}.json"))
                    .is_file()
                {
                    let baseline = store::load(&root, &reference)?;
                    text.push_str("\nINDICES RELATIFS À LA RÉFÉRENCE CONSERVÉE\n");
                    text.push_str(
                        &reporting::compare_text(&baseline, &session)
                            .unwrap_or_else(|e| format!("Indices indisponibles : {e}")),
                    );
                }
            }
            let mut summary = format!(
                "{:.1} s · {}\nCPU : {}\n",
                session.elapsed_seconds,
                session.protocol,
                session
                    .fingerprint
                    .cpu_model
                    .as_deref()
                    .unwrap_or("indisponible")
            );
            let measures = session
                .metrics
                .iter()
                .take(16)
                .map(|metric| {
                    (
                        metric.label.clone(),
                        format!(
                            "{:.3} {}\nVariation entre répétitions : {:.2} %",
                            metric.mean(),
                            metric.unit,
                            metric.cv() * 100.
                        ),
                    )
                })
                .collect::<Vec<_>>();
            if let Some(body) = &session.system {
                if let Some(name) =
                    body.parameters["gpu_profile_before"]["active_profile"]["name"].as_str()
                {
                    summary.push_str(&format!("Profil GPU observé avant le test : {name}\n"));
                }
            }
            Ok((summary, measures, text))
        })();
        let _ = tx.send(Event::Viewed(outcome));
    });
}

fn title(entry: &Entry) -> String {
    let date = glib::DateTime::from_unix_local(entry.completed_unix_seconds as i64)
        .ok()
        .and_then(|t| t.format("%d/%m/%Y %H:%M").ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| entry.completed_unix_seconds.to_string());
    format!(
        "{} · {} · {} · {}{}",
        entry.distribution,
        if entry.kind == SystemKind::Original {
            "origine déclarée"
        } else {
            "optimisé déclaré"
        },
        date,
        &entry.id[..8],
        if store::root()
            .join("references")
            .join(format!("{}.json", entry.id))
            .is_file()
        {
            " · Référence conservée"
        } else {
            ""
        }
    )
}
fn refresh(
    entries: &Rc<RefCell<Vec<Entry>>>,
    left: &gtk::DropDown,
    right: &gtk::DropDown,
    query: &str,
) -> Result<(), String> {
    let old_left = selected(entries, left).map(|e| e.id);
    let old_right = selected(entries, right).map(|e| e.id);
    let query = query.to_lowercase();
    let list = store::list(&store::root())?
        .into_iter()
        .filter(|entry| {
            query.is_empty()
                || format!("{} {} {}", title(entry), entry.id, entry.protocol)
                    .to_lowercase()
                    .contains(&query)
        })
        .collect::<Vec<_>>();
    let names: Vec<_> = list.iter().map(title).collect();
    let refs: Vec<_> = names.iter().map(String::as_str).collect();
    for dropdown in [left, right] {
        dropdown.set_model(Some(&gtk::StringList::new(&refs)));
    }
    let reference = list.iter().position(|e| {
        store::root()
            .join("references")
            .join(format!("{}.json", e.id))
            .is_file()
    });
    left.set_selected(
        old_left
            .and_then(|id| list.iter().position(|e| e.id == id))
            .or(reference)
            .unwrap_or(if list.len() > 1 { 1 } else { 0 }) as u32,
    );
    right.set_selected(
        old_right
            .and_then(|id| list.iter().position(|e| e.id == id))
            .unwrap_or(0) as u32,
    );
    *entries.borrow_mut() = list;
    Ok(())
}
fn selected(entries: &Rc<RefCell<Vec<Entry>>>, dropdown: &gtk::DropDown) -> Option<Entry> {
    entries.borrow().get(dropdown.selected() as usize).cloned()
}
pub fn panel(active: Rc<Cell<bool>>) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
    panel.add_css_class("gs-card");
    let heading = label("Aurelia · Évaluation du système");
    heading.add_css_class("gs-section-title");
    panel.append(&heading);
    panel.append(&label("Sept catégories : CPU, cache, RAM native, GPU Vulkan, charge combinée, glibc et bibliothèques système. Les résultats sont conservés automatiquement. Cible : environ deux minutes ; la durée réelle peut dépasser cette estimation."));
    let spaces = gtk::Stack::new();
    spaces.set_vhomogeneous(false);
    spaces.set_hhomogeneous(false);
    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&spaces));
    panel.append(&switcher);
    panel.append(&spaces);
    let launch = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let tracking = gtk::Box::new(gtk::Orientation::Vertical, 14);
    let history = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let results = gtk::Box::new(gtk::Orientation::Vertical, 14);
    spaces.add_titled(&launch, Some("launch"), "Lancer");
    spaces.add_titled(&tracking, Some("tracking"), "Suivi");
    spaces.add_titled(&results, Some("results"), "Résultats");
    spaces.add_titled(&history, Some("history"), "Historique & comparaison");
    let advanced = gtk::Expander::new(Some("Protocole et conditions de comparaison"));
    let advanced_content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    advanced_content.append(&crate::context_help::heading("Même protocole, même AppImage", "Les indices sont relatifs à la session de référence (100). Le score natif exige cinq catégories ; le score applicatif reste indépendant. Les phases indisponibles ne sont pas remplacées. Une différence de version ne prouve pas la cause d’un gain. Les mesures incertaines restent visibles avec « Écart à confirmer »."));
    advanced.set_child(Some(&advanced_content));
    launch.append(&advanced);
    let kind = gtk::DropDown::from_strings(&[
        "Configuration d’origine déclarée",
        "Configuration optimisée déclarée",
    ]);
    advanced_content.append(&kind);
    let buttons = gtk::FlowBox::new();
    buttons.set_selection_mode(gtk::SelectionMode::None);
    buttons.set_column_spacing(8);
    buttons.set_row_spacing(8);
    buttons.set_max_children_per_line(4);
    let start = gtk::Button::with_label("Évaluer mon système");
    start.add_css_class("suggested-action");
    let cancel_button = gtk::Button::with_label("Annuler");
    cancel_button.set_sensitive(false);
    let import = gtk::Button::with_label("Importer un résultat");
    let export = gtk::Button::with_label("Exporter le résultat B");
    buttons.insert(&start, -1);
    launch.append(&buttons);
    tracking.append(&cancel_button);
    tracking.append(&crate::benchmark_dashboard::telemetry(active.clone()));
    let progress = gtk::ProgressBar::new();
    progress.set_show_text(true);
    tracking.append(&label(
        "Avancement des catégories terminées ; la barre ne prédit pas la durée restante.",
    ));
    tracking.append(&progress);
    let status = label("Aucune modification du système. Les résultats restent sur cette machine.");
    panel.append(&status);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Rechercher : machine, protocole, date ou identifiant"));
    history.append(&search);
    let left = gtk::DropDown::from_strings(&[]);
    let right = gtk::DropDown::from_strings(&[]);
    history.append(&label("Session A · référence à 100"));
    history.append(&left);
    history.append(&label("Session B · résultat à comparer / exporter"));
    history.append(&right);
    let actions = gtk::FlowBox::new();
    actions.set_selection_mode(gtk::SelectionMode::None);
    actions.set_column_spacing(8);
    actions.set_row_spacing(8);
    let compare_button = gtk::Button::with_label("Comparer A et B");
    let reference_button = gtk::Button::with_label("Conserver A comme référence");
    let view_button = gtk::Button::with_label("Consulter le rapport B");
    let reload = gtk::Button::with_label("Actualiser l’historique");
    let archive_button = gtk::Button::with_label("Retirer B de l’historique");
    for button in [
        &view_button,
        &compare_button,
        &reference_button,
        &reload,
        &archive_button,
    ] {
        actions.insert(button, -1);
    }
    history.append(&actions);
    let cross_hardware = gtk::CheckButton::with_label(
        "Comparer des machines différentes (épreuves compatibles uniquement)",
    );
    history.append(&cross_hardware);
    let result=label("Consultez un rapport individuel sans référence ni comparaison. Pour comparer deux distributions, conservez exactement la même AppImage et exportez le JSON.");
    results.append(&result);
    let comparison_bars = gtk::Box::new(gtk::Orientation::Vertical, 8);
    results.append(&comparison_bars);
    let details = label("");
    let expander = gtk::Expander::new(Some("Mesures détaillées et variation entre répétitions"));
    expander.set_child(Some(&details));
    results.append(&expander);
    let transfer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    transfer.append(&import);
    transfer.append(&export);
    history.append(&transfer);
    let entries = Rc::new(RefCell::new(Vec::new()));
    if let Err(e) = refresh(&entries, &left, &right, &search.text()) {
        status.set_text(&e);
    }
    {
        let entries = entries.clone();
        let left = left.clone();
        let right = right.clone();
        let status = status.clone();
        search.connect_search_changed(move |search| {
            if let Err(error) = refresh(&entries, &left, &right, &search.text()) {
                status.set_text(&error);
            }
        });
    }
    let busy = Rc::new(Cell::new(false));
    let evaluating = Rc::new(Cell::new(false));
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Event>();
    {
        let active = active.clone();
        let busy = busy.clone();
        let tx = tx.clone();
        let cancel = cancel.clone();
        let kind = kind.clone();
        let status = status.clone();
        let progress = progress.clone();
        let evaluating = evaluating.clone();
        let spaces = spaces.clone();
        start.connect_clicked(move |_| {
            if crate::amd_controls::awaiting_confirmation() {
                status
                    .set_text("Confirmer ou restaurer les réglages matériels avant le benchmark.");
                return;
            }
            if active.get() || busy.get() {
                status.set_text("Un autre test ou traitement est en cours.");
                return;
            }
            spaces.set_visible_child_name("tracking");
            busy.set(true);
            active.set(true);
            evaluating.set(true);
            cancel.store(false, Ordering::Relaxed);
            progress.set_fraction(0.);
            status.set_text("Préparation de l’évaluation…");
            let tx = tx.clone();
            let cancel = cancel.clone();
            let state = if kind.selected() == 1 {
                "optimized"
            } else {
                "original"
            };
            std::thread::spawn(move || {
                let result = run(cancel, tx.clone(), state);
                let _ = tx.send(Event::Saved(result));
            });
        });
    }
    {
        let cancel = cancel.clone();
        cancel_button.connect_clicked(move |_| {
            cancel.store(true, Ordering::Relaxed);
        });
    }
    {
        let cancel = cancel.clone();
        panel.connect_unrealize(move |_| {
            cancel.store(true, Ordering::Relaxed);
        });
    }
    {
        let entries = entries.clone();
        let left = left.clone();
        let right = right.clone();
        let status = status.clone();
        let search = search.clone();
        reload.connect_clicked(move |_| {
            if let Err(e) = refresh(&entries, &left, &right, &search.text()) {
                status.set_text(&e);
            }
        });
    }
    {
        let entries = entries.clone();
        let left = left.clone();
        let status = status.clone();
        let busy = busy.clone();
        let active = active.clone();
        let tx = tx.clone();
        reference_button.connect_clicked(move |_| {
            if busy.get() || active.get() {
                return;
            }
            if let Some(entry) = selected(&entries, &left) {
                busy.set(true);
                active.set(true);
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let outcome = store::reference(&store::root(), &entry.id);
                    let _ = tx.send(Event::Exported(outcome));
                });
                status.set_text("Conservation de la référence…");
            }
        });
    }
    {
        let entries = entries.clone();
        let left = left.clone();
        let right = right.clone();
        let busy = busy.clone();
        let active = active.clone();
        let tx = tx.clone();
        let status = status.clone();
        let cross_hardware = cross_hardware.clone();
        compare_button.connect_clicked(move |_| {
            if busy.get() || active.get() {
                return;
            }
            let (Some(a), Some(b)) = (selected(&entries, &left), selected(&entries, &right)) else {
                status.set_text("Sélectionnez deux sessions compatibles.");
                return;
            };
            busy.set(true);
            active.set(true);
            let tx = tx.clone();
            let across_hardware = cross_hardware.is_active();
            std::thread::spawn(move || {
                let outcome = (|| {
                    let root = store::root();
                    let a = store::load(&root, &a.id)?;
                    let b = store::load(&root, &b.id)?;
                    let text = reporting::compare_text_with_hardware(&a, &b, across_hardware)?;
                    let bars = if a.system.is_some() && b.system.is_some() {
                        reporting::compare_with_hardware(&a, &b, across_hardware)?
                            .categories
                            .into_iter()
                            .collect()
                    } else {
                        Vec::new()
                    };
                    Ok((text, bars))
                })();
                let _ = tx.send(Event::Compared(outcome));
            });
        });
    }
    {
        let tx = tx.clone();
        let busy = busy.clone();
        let active = active.clone();
        import.connect_clicked(move |button| {
            if busy.get() || active.get() {
                return;
            }
            let dialog = gtk::FileDialog::builder()
                .title("Importer un résultat Aurelia")
                .accept_label("Importer")
                .build();
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Rapports GameShift (.gsbench) et JSON"));
            filter.add_pattern("*.gsbench");
            filter.add_pattern("*.json");
            let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            dialog.set_filters(Some(&filters));
            let tx = tx.clone();
            let busy = busy.clone();
            let active = active.clone();
            let parent = button.root().and_then(|r| r.downcast::<gtk::Window>().ok());
            dialog.open(
                parent.as_ref(),
                None::<&gtk::gio::Cancellable>,
                move |response| {
                    if let Ok(file) = response {
                        if let Some(path) = file.path() {
                            if active.get() || busy.get() {
                                return;
                            }
                            busy.set(true);
                            active.set(true);
                            let tx = tx.clone();
                            std::thread::spawn(move || {
                                let result = store::import(&store::root(), &path);
                                let _ = tx.send(Event::Saved(result));
                            });
                        }
                    }
                },
            );
        });
    }
    {
        let tx = tx.clone();
        let busy = busy.clone();
        let active = active.clone();
        let entries = entries.clone();
        let right = right.clone();
        let status = status.clone();
        export.connect_clicked(move |button| {
            if busy.get() || active.get() {
                return;
            }
            let Some(entry) = selected(&entries, &right) else {
                status.set_text("Sélectionnez une session B à exporter.");
                return;
            };
            let dialog = gtk::FileDialog::builder()
                .title("Exporter une copie du résultat")
                .accept_label("Exporter")
                .initial_name(format!("Aurelia-{}.gsbench", &entry.id[..12]))
                .build();
            let tx = tx.clone();
            let busy = busy.clone();
            let active = active.clone();
            let parent = button.root().and_then(|r| r.downcast::<gtk::Window>().ok());
            dialog.save(
                parent.as_ref(),
                None::<&gtk::gio::Cancellable>,
                move |response| {
                    if let Ok(file) = response {
                        if let Some(path) = file.path() {
                            if active.get() || busy.get() {
                                return;
                            }
                            busy.set(true);
                            active.set(true);
                            let tx = tx.clone();
                            let id = entry.id.clone();
                            std::thread::spawn(move || {
                                let outcome = store::export(&store::root(), &id, &path);
                                let _ = tx.send(Event::Exported(outcome));
                            });
                        }
                    }
                },
            );
        });
    }
    {
        let tx = tx.clone();
        let entries = entries.clone();
        let right = right.clone();
        let left = left.clone();
        view_button.connect_clicked(move |_| {
            if let Some(entry) = selected(&entries, &right) {
                view_report(
                    entry.id,
                    selected(&entries, &left).map(|v| v.id),
                    tx.clone(),
                );
            }
        });
    }
    {
        let entries = entries.clone();
        let right = right.clone();
        let busy = busy.clone();
        let active = active.clone();
        let tx = tx.clone();
        archive_button.connect_clicked(move |button| {
            if busy.get() || active.get(){return;}
            let Some(entry)=selected(&entries,&right) else{return};
            let dialog=gtk::AlertDialog::builder().modal(true).message("Retirer ce résultat de l’historique ?")
                .detail("Le résultat sera conservé dans le sous-dossier removed de l’historique interne. Les copies exportées restent disponibles.")
                .buttons(["Annuler","Retirer"]).cancel_button(0).default_button(0).build();
            let parent=button.root().and_then(|r|r.downcast::<gtk::Window>().ok());
            let busy=busy.clone();let active=active.clone();let tx=tx.clone();
            dialog.choose(parent.as_ref(),None::<&gtk::gio::Cancellable>,move |response| {
                if response==Ok(1) && !busy.get() && !active.get(){
                    busy.set(true);active.set(true);std::thread::spawn(move ||{let _=tx.send(Event::Archived(store::archive(&store::root(),&entry.id)));});
                }
            });
        });
    }
    let weak = panel.downgrade();
    let clock = Instant::now();
    let phase = Rc::new(RefCell::new(String::new()));
    let began = Rc::new(Cell::new(0.));
    glib::timeout_add_local(Duration::from_millis(100), move || {
        if weak.upgrade().is_none() {
            cancel.store(true, Ordering::Relaxed);
            return glib::ControlFlow::Break;
        }
        let running = busy.get();
        for widget in [
            &start,
            &import,
            &export,
            &compare_button,
            &reference_button,
            &reload,
            &archive_button,
        ] {
            widget.set_sensitive(!running && !active.get());
        }
        compare_button.set_sensitive(!running && !active.get() && entries.borrow().len() >= 2);
        left.set_sensitive(!running);
        right.set_sensitive(!running);
        kind.set_sensitive(!running);
        search.set_sensitive(!running && !active.get());
        cross_hardware.set_sensitive(!running && !active.get());
        view_button.set_sensitive(!running && !active.get() && !entries.borrow().is_empty());
        cancel_button.set_sensitive(evaluating.get());
        while let Ok(event) = rx.try_recv() {
            match event {
                Event::Progress(name, fraction) => {
                    if began.get() == 0. {
                        began.set(clock.elapsed().as_secs_f64());
                    }
                    *phase.borrow_mut() = name;
                    progress.set_fraction(fraction.clamp(0., 1.));
                }
                Event::Saved(outcome) => {
                    evaluating.set(false);
                    progress.set_text(None);
                    busy.set(false);
                    active.set(false);
                    began.set(0.);
                    match outcome {
                        Ok(entry) => {
                            progress.set_fraction(1.);
                            status.set_text(&format!(
                                "Évaluation enregistrée : {}. Ouverture du rapport individuel…",
                                entry.distribution
                            ));
                            view_report(
                                entry.id.clone(),
                                selected(&entries, &left).map(|v| v.id),
                                tx.clone(),
                            );
                            search.set_text("");
                            if let Err(e) = refresh(&entries, &left, &right, &search.text()) {
                                status.set_text(&e);
                            } else if let Some(index) = entries
                                .borrow()
                                .iter()
                                .position(|candidate| candidate.id == entry.id)
                            {
                                right.set_selected(index as u32);
                            }
                        }
                        Err(e) => {
                            status.set_text(&e);
                            progress.set_fraction(0.);
                        }
                    }
                }
                Event::Viewed(outcome) => match outcome {
                    Ok((summary, measures, text)) => {
                        while let Some(child) = comparison_bars.first_child() {
                            comparison_bars.remove(&child);
                        }
                        let grid = gtk::FlowBox::new();
                        grid.set_selection_mode(gtk::SelectionMode::None);
                        grid.set_min_children_per_line(1);
                        grid.set_max_children_per_line(3);
                        grid.set_homogeneous(true);
                        grid.set_row_spacing(12);
                        grid.set_column_spacing(12);
                        for (name, value) in measures {
                            let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
                            card.add_css_class("gs-card");
                            let heading = label(&name);
                            heading.add_css_class("heading");
                            card.append(&heading);
                            let value = label(&value);
                            value.add_css_class("gs-section-title");
                            card.append(&value);
                            grid.insert(&card, -1);
                        }
                        comparison_bars.append(&grid);
                        result.set_text(&summary);
                        details.set_text(&text);
                        spaces.set_visible_child_name("results");
                        status.set_text("Rapport individuel disponible ; les comparaisons restent facultatives.");
                    }
                    Err(e) => status.set_text(&e),
                },
                Event::Compared(outcome) => {
                    busy.set(false);
                    active.set(false);
                    match outcome {
                        Ok((comparison, bars)) => {
                            spaces.set_visible_child_name("results");
                            result.set_text(
                                &comparison.lines().take(6).collect::<Vec<_>>().join("\n"),
                            );
                            details.set_text(&comparison);
                            while let Some(child) = comparison_bars.first_child() {
                                comparison_bars.remove(&child);
                            }
                            let maximum =
                                bars.iter().filter_map(|(_, v)| *v).fold(100., f64::max) * 1.05;
                            for (name, value) in bars {
                                let name = match name.as_str() {
                                    "cpu" => "CPU",
                                    "cache" => "Cache",
                                    "ram" => "RAM",
                                    "gpu" => "GPU",
                                    "combined" => "Charge combinée",
                                    "glibc" => "glibc",
                                    "applications" => "Bibliothèques",
                                    other => other,
                                };
                                comparison_bars.append(&label(&format!(
                                    "{name} · A = 100 · B = {}",
                                    value
                                        .map(|v| format!("{v:.2}"))
                                        .unwrap_or_else(|| "Non comparable".into())
                                )));
                                if let Some(value) = value {
                                    for (caption, index) in
                                        [("Référence A", 100.), ("Résultat B", value)]
                                    {
                                        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                                        let name = label(caption);
                                        name.set_width_chars(12);
                                        row.append(&name);
                                        let bar = gtk::LevelBar::for_interval(0., maximum);
                                        bar.set_value(index);
                                        bar.set_hexpand(true);
                                        row.append(&bar);
                                        comparison_bars.append(&row);
                                    }
                                }
                            }

                            status.set_text(
                                "Comparaison calculée ; les résultats bruts sont conservés.",
                            );
                        }
                        Err(e) => {
                            result.set_text(&e);
                            while let Some(child) = comparison_bars.first_child() {
                                comparison_bars.remove(&child);
                            }
                        }
                    }
                }
                Event::Archived(outcome) => {
                    busy.set(false);
                    active.set(false);
                    match outcome {
                        Ok(()) => {
                            if let Err(error) = refresh(&entries, &left, &right, &search.text()) {
                                status.set_text(&error);
                            } else {
                                status.set_text(
                                    "Résultat retiré de la liste et conservé dans removed.",
                                );
                            }
                        }
                        Err(error) => status.set_text(&error),
                    }
                }
                Event::Exported(outcome) => {
                    busy.set(false);
                    active.set(false);
                    status.set_text(&match outcome {
                        Ok(()) => "Fichier enregistré sans modifier les résultats bruts.".into(),
                        Err(e) => e,
                    });
                }
            }
        }
        if busy.get() && active.get() && began.get() != 0. {
            progress.set_text(Some(&format!(
                "{} · {:.0} s écoulées",
                phase.borrow(),
                clock.elapsed().as_secs_f64() - began.get()
            )));
        }
        glib::ControlFlow::Continue
    });
    panel
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn cancellation_terminates_the_auxiliary_process_group() {
        let root =
            std::env::temp_dir().join(format!("gameshift-cancel-group-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("worker");
        let pidfile = root.join("child.pid");
        std::fs::write(
            &script,
            "#!/bin/sh\nsleep 30 &\nprintf '%s' \"$!\" > \"${0%/*}/child.pid\"\nwait\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let cancel = flag.clone();
        let (tx, _rx) = mpsc::channel();
        let task = std::thread::spawn(move || run_program(&script, cancel, tx, "original"));
        let start = Instant::now();
        while !std::fs::read_to_string(&pidfile).is_ok_and(|s| s.parse::<u32>().is_ok())
            && start.elapsed() < Duration::from_secs(3)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        flag.store(true, Ordering::Relaxed);
        let result = task.join().unwrap();
        assert!(result.unwrap_err().contains("annulé"));
        let pid = std::fs::read_to_string(&pidfile).unwrap();
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        assert!(
            status.is_empty()
                || status
                    .lines()
                    .any(|line| line.starts_with("State:") && line.contains('Z')),
            "auxiliary still active: {status}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
