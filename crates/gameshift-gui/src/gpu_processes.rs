//! Vendor-neutral fdinfo UI. A missing counter is unavailable, never zero.
use adw::prelude::*;
use gameshift_telemetry::{
    gpus,
    processes::{ProcessGpu, Tracker},
};
use gtk::{gio, glib};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
fn label(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::Word);
    l
}
fn bytes(row: &ProcessGpu, key: &str) -> String {
    row.memory_bytes
        .get(key)
        .map(|b| format!("{:.1} Mio", *b as f64 / 1048576.))
        .unwrap_or_else(|| "Indisponible".into())
}
fn field(row: &ProcessGpu, col: usize) -> String {
    match col {
        0 => row.name.clone(),
        1 => row.pid.to_string(),
        2 => row.pci.clone(),
        3 => bytes(row, "vram"),
        4 => bytes(row, "gtt"),
        5 => row
            .activity()
            .map(|v| format!("{v:.1} %"))
            .unwrap_or("Indisponible".into()),
        _ => {
            if row.engines.is_empty() {
                "Indisponible".into()
            } else {
                row.engines
                    .iter()
                    .map(|e| format!("{} {:.2} s", e.name, e.cumulative_ns as f64 / 1e9))
                    .collect::<Vec<_>>()
                    .join(" · ")
            }
        }
    }
}
fn details(row: &ProcessGpu) -> String {
    format!("{} · PID {}\nExécutable : {}\nGPU {} · pilote {} · {} clients DRM{}\n{}\nMémoire : {}\nSource : /proc/{}/fdinfo, compteurs DRM du pilote. Les temps des moteurs peuvent se chevaucher ; leur somme n’est pas un temps mural GPU. Puissance par processus, CU exactes et codec effectivement utilisé : non exposés ici.",row.name,row.pid,row.executable.as_deref().unwrap_or("Lecture refusée ou processus terminé"),row.pci,row.driver,row.clients,if row.shared_clients{" · clients partagés entre processus, ne pas additionner les lignes"}else{""},row.engines.iter().map(|e|format!("{} : {:.3} s cumulées · {} · capacité {}",e.name,e.cumulative_ns as f64/1e9,e.busy_percent.map(|v|format!("{v:.1} %")).unwrap_or("premier relevé, remise à zéro ou compteur absent".into()),e.capacity)).collect::<Vec<_>>().join("\n"),row.memory_bytes.iter().map(|(k,v)|format!("{k} {:.1} Mio",*v as f64/1048576.)).collect::<Vec<_>>().join(" · "),row.pid)
}
fn video(node: String) -> Result<String, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child=Command::new("vainfo").args(["--display","drm","--device",&node]).env_remove("LD_LIBRARY_PATH").env_remove("LD_PRELOAD").env_remove("LD_AUDIT").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|_|"vainfo absent : interrogation VA-API facultative indisponible, aucune installation automatique.".to_owned())?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let output = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.take(65536).read_to_string(&mut s);
        s
    });
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.take(65536).read_to_string(&mut s);
        s
    });
    let began = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if began.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = output.join();
            let _ = errors.join();
            return Err("Interrogation VA-API expirée (5 s)".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let text = output.join().unwrap_or_default();
    let errors = errors.join().unwrap_or_default();
    if !status.success() {
        return Err(format!(
            "VA-API non disponible pour ce nœud : {}",
            errors.lines().take(4).collect::<Vec<_>>().join("\n")
        ));
    }
    let codecs = text
        .lines()
        .filter(|s| s.contains("VAProfile") && s.contains("VAEntrypoint"))
        .collect::<Vec<_>>();
    if codecs.is_empty() {
        return Err("Aucun profil vidéo VA-API retourné".into());
    }
    Ok(format!("Source : vainfo / VA-API, {node}\n{}\nIl s’agit des capacités exposées par cette API, pas du codec utilisé par un processus ni d’une liste exhaustive des capacités du silicium.",codecs.join("\n")))
}
pub fn panel(names: std::collections::BTreeMap<String, String>) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.append(&label("Processus GPU · surveillance standard Linux"));
    root.append(&label("AMDGPU, i915, Xe et autres pilotes exposant DRM fdinfo. NVIDIA propriétaire peut ne pas fournir ces compteurs. Aucun helper requis. Les compteurs cycles seuls (certains Xe) ne sont pas encore décodés. Les métriques absentes restent indisponibles."));
    let selector = gtk::DropDown::from_strings(&[]);
    let interval = gtk::DropDown::from_strings(&[
        "Toutes les secondes",
        "Toutes les 2 secondes",
        "Toutes les 5 secondes",
    ]);
    interval.set_selected(1);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    selector.set_hexpand(true);
    top.append(&selector);
    top.append(&interval);
    root.append(&top);
    let sensor_label = label("Lecture de l’inventaire…");
    sensor_label.set_selectable(true);
    root.append(&sensor_label);
    let status = label("Première mesure : les taux nécessitent deux relevés du même client DRM.");
    root.append(&status);
    let store = gio::ListStore::new::<glib::BoxedAnyObject>();
    let sorted = gtk::SortListModel::new(Some(store.clone()), None::<gtk::Sorter>);
    let selection = gtk::SingleSelection::new(Some(sorted.clone()));
    let table = gtk::ColumnView::new(Some(selection.clone()));
    table.set_show_row_separators(true);
    table.set_hexpand(true);
    for (col, title) in [
        "Application",
        "PID",
        "GPU PCI",
        "VRAM attribuée",
        "GTT attribuée",
        "Moteur max.",
        "Temps moteurs cumulés",
    ]
    .iter()
    .enumerate()
    {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                let text = gtk::Label::new(None);
                text.set_xalign(0.);
                text.set_ellipsize(gtk::pango::EllipsizeMode::End);
                text.set_margin_start(8);
                text.set_margin_end(8);
                item.set_child(Some(&text));
            }
        });
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(object) = item
                .item()
                .and_then(|v| v.downcast::<glib::BoxedAnyObject>().ok())
            else {
                return;
            };
            if let Some(label) = item.child().and_then(|v| v.downcast::<gtk::Label>().ok()) {
                let row = object.borrow::<ProcessGpu>();
                let text = field(&row, col);
                label.set_text(&text);
                label.set_tooltip_text(Some(&text));
            }
        });
        let column = gtk::ColumnViewColumn::new(Some(title), Some(factory));
        column.set_resizable(true);
        column.set_expand(col == 0);
        let sorter = gtk::CustomSorter::new(move |a, b| {
            let a = a
                .downcast_ref::<glib::BoxedAnyObject>()
                .unwrap()
                .borrow::<ProcessGpu>();
            let b = b
                .downcast_ref::<glib::BoxedAnyObject>()
                .unwrap()
                .borrow::<ProcessGpu>();
            let order = match col {
                1 => a.pid.cmp(&b.pid),
                3 | 4 => a
                    .memory_bytes
                    .get(if col == 3 { "vram" } else { "gtt" })
                    .cmp(&b.memory_bytes.get(if col == 3 { "vram" } else { "gtt" })),
                5 => a
                    .activity()
                    .partial_cmp(&b.activity())
                    .unwrap_or(std::cmp::Ordering::Equal),
                _ => field(&a, col).cmp(&field(&b, col)),
            };
            match order {
                std::cmp::Ordering::Less => gtk::Ordering::Smaller,
                std::cmp::Ordering::Equal => gtk::Ordering::Equal,
                std::cmp::Ordering::Greater => gtk::Ordering::Larger,
            }
        });
        column.set_sorter(Some(&sorter));
        table.append_column(&column);
    }
    sorted.set_sorter(table.sorter().as_ref());
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_height(290)
        .vexpand(true)
        .child(&table)
        .build();
    root.append(&scroll);
    let detail = label(
        "Sélectionner un processus pour consulter les moteurs, l’exécutable et l’historique.",
    );
    detail.set_selectable(true);
    let expander = gtk::Expander::new(Some("Détails du processus"));
    expander.set_expanded(true);
    expander.set_child(Some(&detail));
    root.append(&expander);
    let history = Rc::new(RefCell::new(Vec::<Option<f64>>::new()));
    let chart = gtk::DrawingArea::new();
    chart.set_content_height(150);
    chart.set_hexpand(true);
    let data = history.clone();
    chart.set_draw_func(move |_, cr, width, height| {
        cr.set_source_rgb(0.07, 0.09, 0.12);
        let _ = cr.paint();
        let points = data.borrow();
        cr.set_source_rgb(0.25, 0.87, 0.7);
        cr.set_line_width(2.);
        let mut path = false;
        for (i, value) in points.iter().enumerate() {
            if let Some(value) = value {
                let x = 12.
                    + i as f64 / (points.len().saturating_sub(1).max(1) as f64)
                        * (width as f64 - 24.);
                let y = 12. + (1. - value.clamp(0., 100.) / 100.) * (height as f64 - 24.);
                if path {
                    cr.line_to(x, y);
                } else {
                    cr.move_to(x, y);
                    path = true;
                }
            } else {
                let _ = cr.stroke();
                path = false;
            }
        }
        let _ = cr.stroke();
    });
    root.append(&label("Historique du moteur le plus occupé · 60 derniers relevés · 0–100 %, sans cumul des moteurs"));
    root.append(&chart);
    selection.connect_selected_item_notify({
        let detail = detail.clone();
        let history = history.clone();
        let chart = chart.clone();
        move |selection| {
            if let Some(object) = selection
                .selected_item()
                .and_then(|v| v.downcast::<glib::BoxedAnyObject>().ok())
            {
                let row = object.borrow::<ProcessGpu>();
                detail.set_text(&details(&row));
                *history.borrow_mut() = row.history.clone();
            } else {
                detail.set_text("Processus terminé, masqué ou non sélectionné.");
                history.borrow_mut().clear();
            }
            chart.queue_draw();
        }
    });
    let capabilities=label("Les capacités vidéo nécessitent une interrogation explicite. Aucune déduction à partir du nom du GPU.");
    capabilities.set_selectable(true);
    let query = gtk::Button::with_label("Interroger les codecs disponibles (VA-API)");
    root.append(&query);
    root.append(&capabilities);
    selector.connect_selected_notify({
        let store = store.clone();
        let sensors = sensor_label.clone();
        let caps = capabilities.clone();
        move |_| {
            store.remove_all();
            sensors.set_text("GPU sélectionné : attente du prochain relevé.");
            caps.set_text(
                "Les capacités vidéo ne sont pas déduites ; interroger ce GPU explicitement.",
            );
        }
    });
    let devices = Rc::new(RefCell::new(Vec::<gpus::Gpu>::new()));
    query.connect_clicked({
        let devices = devices.clone();
        let selector = selector.clone();
        let label = capabilities.clone();
        move |_| {
            let node = devices
                .borrow()
                .get(selector.selected() as usize)
                .and_then(|g| g.nodes.iter().find(|n| n.contains("/renderD")).cloned());
            let Some(node) = node else {
                label.set_text("Aucun nœud render accessible pour ce GPU");
                return;
            };
            crate::profiles_page::task(&label, move || video(node));
        }
    });
    let tracker = Arc::new(Mutex::new(Tracker::default()));
    let (tx, rx) = mpsc::channel();
    let weak = root.downgrade();
    let mut next = Instant::now() - Duration::from_secs(5);
    let mut busy = false;
    let mut paused = true;
    let mut signature = Vec::<String>::new();
    let mut latest = None::<gameshift_telemetry::processes::Snapshot>;
    glib::timeout_add_local(Duration::from_millis(200), move || {
        let Some(root) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let suspended = !root.is_mapped()
            || crate::live_settings::benchmark_running()
            || gameshift_control::benchmark::benchmark_active();
        if suspended {
            paused = true;
            status.set_text("Collecte suspendue hors de la vue ou pendant un benchmark.");
        }
        if !suspended
            && !busy
            && next.elapsed() >= Duration::from_secs([1, 2, 5][interval.selected().min(2) as usize])
        {
            busy = true;
            next = Instant::now();
            let tx = tx.clone();
            let tracker = tracker.clone();
            let gap = paused;
            paused = false;
            std::thread::spawn(move || {
                let devices = gpus::discover().unwrap_or_default();
                let sensors = devices
                    .iter()
                    .map(|g| (g.pci.clone(), gpus::generic_sensors(g)))
                    .collect::<std::collections::BTreeMap<_, _>>();
                let mut tracker = tracker.lock().unwrap();
                if gap {
                    tracker.pause();
                }
                let snapshot = tracker.sample();
                let _ = tx.send((devices, sensors, snapshot));
            });
        }
        while let Ok((found, sensors, snapshot)) = rx.try_recv() {
            busy = false;
            if suspended {
                continue;
            }
            let ids = found.iter().map(|g| g.pci.clone()).collect::<Vec<_>>();
            let old = devices
                .borrow()
                .get(selector.selected() as usize)
                .map(|g| g.pci.clone());
            if ids != signature {
                signature = ids;
                let titles = found
                    .iter()
                    .map(|g| {
                        format!(
                            "{} · {} · {}",
                            names.get(&g.pci).unwrap_or(&g.name),
                            g.pci,
                            g.driver.as_deref().unwrap_or("sans pilote")
                        )
                    })
                    .collect::<Vec<_>>();
                selector.set_model(Some(&gtk::StringList::new(
                    &titles.iter().map(String::as_str).collect::<Vec<_>>(),
                )));
                selector.set_selected(
                    old.and_then(|id| found.iter().position(|g| g.pci == id))
                        .unwrap_or(0) as u32,
                );
            }
            *devices.borrow_mut() = found;
            let gpu = devices.borrow().get(selector.selected() as usize).cloned();
            if let Some(gpu) = gpu {
                let readings = sensors
                    .get(&gpu.pci)
                    .map(|v| {
                        v.iter()
                            .map(|m| format!("{} : {:.1} {}", m.label, m.value, m.unit))
                            .collect::<Vec<_>>()
                            .join(" · ")
                    })
                    .unwrap_or_default();
                sensor_label.set_text(&format!("{} · {}\n{}",names.get(&gpu.pci).unwrap_or(&gpu.name),gpu.pci,if readings.is_empty(){"Capteurs hwmon indisponibles ; pas de substitution par une valeur inventée."}else{&readings}));
            } else {
                sensor_label.set_text("Aucun GPU PCI détecté. Aucune capacité du PC de test n’est déduite de cette VM.");
            }
            latest = Some(snapshot);
        }
        if !suspended {
            if let Some(snapshot) = latest.as_ref() {
                let pci = devices
                    .borrow()
                    .get(selector.selected() as usize)
                    .map(|g| g.pci.clone());
                let old = selection
                    .selected_item()
                    .and_then(|v| v.downcast::<glib::BoxedAnyObject>().ok())
                    .map(|o| {
                        let r = o.borrow::<ProcessGpu>();
                        (r.pid, r.start_ticks)
                    });
                store.remove_all();
                for row in snapshot
                    .rows
                    .iter()
                    .filter(|r| Some(&r.pci) == pci.as_ref())
                {
                    store.append(&glib::BoxedAnyObject::new(row.clone()));
                }
                if let Some(old) = old {
                    for i in 0..sorted.n_items() {
                        if let Some(o) = sorted
                            .item(i)
                            .and_then(|v| v.downcast::<glib::BoxedAnyObject>().ok())
                        {
                            let row = o.borrow::<ProcessGpu>();
                            if (row.pid, row.start_ticks) == old {
                                selection.set_selected(i);
                                break;
                            }
                        }
                    }
                }
                status.set_text(&format!("{} processus visibles · {} lectures refusées · {} disparitions · {} clients non attribuables{} · collecte {:.0} ms. Mémoire attribuée, non additionnable globalement.",store.n_items(),snapshot.denied,snapshot.vanished,snapshot.ambiguous,if snapshot.truncated{" · collecte partielle (budget limité)"}else{""},snapshot.elapsed_seconds*1000.));
                latest = None;
            }
        }
        glib::ControlFlow::Continue
    });
    root
}
