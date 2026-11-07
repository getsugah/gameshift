//! Shared Aurelia presentation. Reads run off-thread; values are never guessed.
use adw::prelude::*;
use gtk::glib;
use std::{
    cell::Cell,
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};
fn label(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::Word);
    l
}
pub fn hardware() -> gtk::Box {
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 8);
    box_.add_css_class("gs-card");
    let heading = label("Matériel du test");
    heading.add_css_class("heading");
    box_.append(&heading);
    let value = label("Lecture du CPU et des GPU…");
    box_.append(&value);
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let cpu = std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|s| {
                s.lines().find_map(|l| {
                    l.strip_prefix("model name")
                        .and_then(|s| s.split_once(':'))
                        .map(|(_, v)| v.trim().to_owned())
                })
            })
            .unwrap_or_else(|| "CPU indisponible".into());
        let gpus = gameshift_telemetry::gpus::discover().unwrap_or_default();
        let text = format!(
            "{}\n{}",
            cpu,
            gpus.iter()
                .map(|g| format!(
                    "{} · {} · {}",
                    g.name,
                    g.pci,
                    g.driver.as_deref().unwrap_or("pilote indisponible")
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let _ = tx.send(text);
    });
    let weak = value.downgrade();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let Some(l) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if let Ok(text) = rx.try_recv() {
            l.set_text(&text);
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
    box_
}
pub fn latest() -> gtk::Box {
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 8);
    box_.add_css_class("gs-card");
    let value = label("Dernier résultat : lecture de l’historique…");
    box_.append(&value);
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let text = gameshift_system_bench::store::list(&gameshift_system_bench::store::root())
            .map(|mut entries| {
                entries.sort_by_key(|e| std::cmp::Reverse(e.completed_unix_seconds));
                entries
                    .first()
                    .map(|e| {
                        format!(
                            "Dernier résultat système · {}\n{} · consulter Résultats / Historique",
                            e.distribution, e.protocol
                        )
                    })
                    .unwrap_or("Aucun résultat système conservé pour le moment.".into())
            })
            .unwrap_or_else(|e| format!("Historique indisponible : {e}"));
        let _ = tx.send(text);
    });
    let weak = value.downgrade();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let Some(l) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        match rx.try_recv() {
            Ok(text) => {
                l.set_text(&text);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            _ => glib::ControlFlow::Continue,
        }
    });
    box_
}
pub fn telemetry(active: Rc<Cell<bool>>) -> gtk::Box {
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 8);
    box_.add_css_class("gs-gpu-surface");
    let title = label("Suivi de la mesure");
    title.add_css_class("heading");
    box_.append(&title);
    let elapsed = label("En attente d’un test");
    box_.append(&elapsed);
    let sensors=label("Les capteurs absents resteront indisponibles. Stabilité calculée uniquement sur les répétitions terminées.");
    box_.append(&sensors);
    let weak = box_.downgrade();
    let (tx, rx) = mpsc::channel();
    let mut busy = false;
    let mut started = None::<Instant>;
    let mut next = Instant::now() - Duration::from_secs(2);
    glib::timeout_add_local(Duration::from_millis(200), move || {
        let Some(box_) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        box_.set_visible(active.get());
        if active.get() {
            let began = started.get_or_insert_with(Instant::now);
            elapsed.set_text(&format!(
                "Temps écoulé : {:.0} s · aucune estimation du temps restant",
                began.elapsed().as_secs_f64()
            ));
        } else {
            started = None;
        }
        if active.get() && box_.is_mapped() && !busy && next.elapsed() >= Duration::from_secs(2) {
            busy = true;
            next = Instant::now();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let cpus = gameshift_cpu::topology::Topology::detect()
                    .allowed_logical_cpus
                    .into_iter()
                    .take(1)
                    .collect::<Vec<_>>();
                let cpu = gameshift_cpu::environment::Snapshot::read(&cpus);
                let mut text = cpu
                    .temperatures
                    .iter()
                    .map(|v| format!("CPU · {:.1} °C", v.celsius))
                    .collect::<Vec<_>>();
                for (core, freq) in &cpu.frequency_khz {
                    if let Some(freq) = freq {
                        text.push(format!("CPU {core} : {:.0} MHz", *freq as f64 / 1000.));
                    }
                }
                if let Ok(gpus) = gameshift_telemetry::gpus::discover() {
                    for gpu in gpus {
                        let readings = if gpu.driver.as_deref() == Some("amdgpu") {
                            gameshift_telemetry::device::DeviceSensors::discover(&gpu.pci).map(|d|d.read().into_iter().filter(|m|matches!(m.metric,gameshift_telemetry::device::Metric::Temperature|gameshift_telemetry::device::Metric::Power|gameshift_telemetry::device::Metric::CoreClock)).filter_map(|m|m.value.map(|v|format!("{} : {:.1} {:?}",m.label,v,m.unit))).collect::<Vec<_>>()).unwrap_or_default()
                        } else {
                            gameshift_telemetry::gpus::generic_sensors(&gpu)
                                .into_iter()
                                .map(|m| format!("{} : {:.1} {}", m.label, m.value, m.unit))
                                .collect()
                        };
                        if !readings.is_empty() {
                            text.push(format!("{} · {}", gpu.name, readings.join(" · ")));
                        }
                    }
                }
                if text.is_empty() {
                    text.push("Températures, puissances et fréquences : indisponibles sur les interfaces accessibles.".into());
                }
                let _ = tx.send(text.join("\n"));
            });
        }
        while let Ok(text) = rx.try_recv() {
            busy = false;
            if active.get() {
                sensors.set_text(&text);
            }
        }
        glib::ControlFlow::Continue
    });
    box_
}
