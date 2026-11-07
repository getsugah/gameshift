//! Visible-page CPU telemetry, collected off GTK with at most one request in flight.
use adw::prelude::*;
use gameshift_cpu::environment::{busy_percent, parse_cpu_ticks, Snapshot};
use gtk::glib;
use std::{collections::BTreeMap, sync::mpsc, time::Duration};

type Ticks = BTreeMap<String, [u64; 2]>;
struct Reading {
    snapshot: Snapshot,
    error: Option<String>,
}
fn collect() -> Reading {
    let stat = std::fs::read_to_string("/proc/stat");
    let cpus = stat
        .as_ref()
        .ok()
        .map(|s| parse_cpu_ticks(s))
        .unwrap_or_default()
        .keys()
        .filter_map(|s| s.strip_prefix("cpu")?.parse::<usize>().ok())
        .collect::<Vec<_>>();
    let mut snapshot = Snapshot::read(&cpus);
    let error = match stat {
        Err(error) => Some(format!("/proc/stat : {error}")),
        Ok(_) if !snapshot.cpu_ticks.contains_key("cpu") => {
            Some("Compteurs CPU absents ou invalides dans /proc/stat".into())
        }
        _ => None,
    };
    if error.is_some() {
        snapshot.cpu_ticks.clear();
    }
    Reading { snapshot, error }
}
fn percent(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.1} %"))
        .unwrap_or_else(|| "Indisponible".into())
}
pub fn panel(stack: &gtk::Stack, detailed: bool) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
    panel.add_css_class("gs-info-panel");
    let heading = gtk::Label::new(Some("CPU · relevés en direct"));
    heading.set_xalign(0.);
    heading.add_css_class("heading");
    panel.append(&heading);
    let summary = gtk::Label::new(Some("Première mesure en attente…"));
    summary.set_xalign(0.);
    summary.set_wrap(true);
    summary.set_selectable(true);
    panel.append(&summary);
    let chart = crate::live_chart::PercentChart::new("Occupation CPU · %");
    panel.append(&chart.widget);
    let details = gtk::Label::new(None);
    details.set_xalign(0.);
    details.set_wrap(true);
    details.set_selectable(true);
    if detailed {
        let expander = gtk::Expander::new(Some("Processeurs logiques, fréquences et températures"));
        expander.set_child(Some(&details));
        panel.append(&expander);
    }
    let note = gtk::Label::new(Some("Occupation globale du système · intervalle réglable dans Paramètres. Collecte suspendue hors Accueil/CPU."));
    note.set_xalign(0.);
    note.set_wrap(true);
    note.add_css_class("dim-label");
    panel.append(&note);
    let weak_panel = panel.downgrade();
    let weak_stack = stack.downgrade();
    let weak_summary = summary.downgrade();
    let weak_details = details.downgrade();
    let weak_note = note.downgrade();
    let mut previous: Option<Ticks> = None;
    let mut pending: Option<mpsc::Receiver<Reading>> = None;
    let mut discard_pending = false;
    let mut was_visible = false;
    let mut last_request = std::time::Instant::now() - Duration::from_secs(1);
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let (Some(_panel), Some(stack), Some(summary), Some(note)) = (
            weak_panel.upgrade(),
            weak_stack.upgrade(),
            weak_summary.upgrade(),
            weak_note.upgrade(),
        ) else {
            return glib::ControlFlow::Break;
        };
        let target = if detailed { "Processeur" } else { "overview" };
        if stack.visible_child_name().as_deref() != Some(target)
            || crate::live_settings::benchmark_running()
        {
            previous = None;
            if was_visible {
                chart.push(None);
            }
            was_visible = false;
            // Let an existing read finish without starting another or keeping its result.
            if let Some(receiver) = &pending {
                if !matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                    pending = None;
                }
            }
            discard_pending = pending.is_some();
            return glib::ControlFlow::Continue;
        }
        was_visible = true;
        if let Some(receiver) = &pending {
            match receiver.try_recv() {
                Ok(reading) => {
                    pending = None;
                    if discard_pending {
                        discard_pending = false;
                        return glib::ControlFlow::Continue;
                    }
                    let snapshot = reading.snapshot;
                    let value = previous.as_ref().and_then(|before| {
                        busy_percent(*before.get("cpu")?, *snapshot.cpu_ticks.get("cpu")?)
                    });
                    summary.set_text(&format!("Occupation CPU : {}", percent(value)));
                    chart.push(value);
                    let source = format!(
                        "/proc/stat · horodatage Unix : {} ms",
                        snapshot
                            .unix_time_ms
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "indisponible".into())
                    );
                    summary.set_tooltip_text(Some(&source));
                    if let Some(details) = weak_details.upgrade() {
                        let mut lines = Vec::new();
                        for (cpu, end) in snapshot
                            .cpu_ticks
                            .iter()
                            .filter(|(cpu, _)| cpu.as_str() != "cpu")
                        {
                            let load = previous
                                .as_ref()
                                .and_then(|p| busy_percent(*p.get(cpu)?, *end));
                            let freq = cpu
                                .strip_prefix("cpu")
                                .and_then(|n| n.parse::<usize>().ok())
                                .and_then(|id| snapshot.frequency_khz.get(&id).copied().flatten());
                            lines.push(format!(
                                "{cpu} : {} · {}",
                                percent(load),
                                freq.map(|f| format!("{:.0} MHz", f as f64 / 1000.))
                                    .unwrap_or_else(|| "Fréquence indisponible".into())
                            ));
                        }
                        if snapshot.temperatures.is_empty() {
                            lines.push("Températures CPU : indisponibles".into());
                        }
                        for temp in &snapshot.temperatures {
                            lines.push(format!("{} : {:.1} °C", temp.source, temp.celsius));
                        }
                        details.set_text(&lines.join("\n"));
                        details.set_tooltip_text(Some("Occupation : /proc/stat ; fréquences observées : /sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq ; températures : hwmon k10temp/coretemp/zenpower/cpu_thermal. Les numéros CPU désignent des processeurs logiques."));
                    }
                    note.set_text(reading.error.as_deref().unwrap_or("Occupation globale du système · intervalle réglable dans Paramètres. Collecte suspendue hors Accueil/CPU."));
                    previous = if reading.error.is_some() {
                        None
                    } else {
                        Some(snapshot.cpu_ticks)
                    };
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    pending = None;
                    previous = None;
                    summary.set_text("Collecte CPU interrompue");
                    chart.push(None);
                }
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        if pending.is_none() && last_request.elapsed() >= crate::live_settings::interval() {
            let (sender, receiver) = mpsc::sync_channel(1);
            match std::thread::Builder::new()
                .name("gameshift-live-cpu".into())
                .spawn(move || {
                    let _ = sender.send(collect());
                }) {
                Ok(_) => {
                    pending = Some(receiver);
                }
                Err(error) => {
                    previous = None;
                    summary.set_text(&format!("Collecte CPU indisponible : {error}"));
                    chart.push(None);
                }
            }
            last_request = std::time::Instant::now();
        }
        glib::ControlFlow::Continue
    });
    panel
}
