//! Live AMDGPU measurements. PCI enumeration and sysfs reads run outside GTK.
use adw::prelude::*;
use gameshift_telemetry::device::{DeviceSensors, Measurement, Metric, Unit};
use gtk::glib;
use std::{
    collections::BTreeMap,
    sync::mpsc,
    time::{Duration, Instant},
};
struct Reading {
    summary: String,
    sources: String,
    occupancy: BTreeMap<String, Option<f64>>,
}
fn unit(unit: Unit) -> &'static str {
    match unit {
        Unit::Celsius => "°C",
        Unit::Megahertz => "MHz",
        Unit::Watts => "W",
        Unit::Rpm => "tr/min",
        Unit::Percent => "%",
        Unit::Bytes => "octets",
    }
}
fn metric_title(metric: Metric) -> &'static str {
    match metric {
        Metric::Temperature => "Température",
        Metric::CoreClock => "Fréquence GPU",
        Metric::MemoryClock => "Fréquence VRAM",
        Metric::Power => "Puissance",
        Metric::FanSpeed => "Ventilateur",
        Metric::Utilization => "Occupation",
        Metric::VramUsed => "VRAM utilisée",
        Metric::VramTotal => "VRAM totale",
    }
}
pub(crate) fn format_measurement(m: &Measurement) -> String {
    let title = metric_title(m.metric);
    let value = match m.value {
        Some(value) if m.unit == Unit::Bytes => format!("{:.2} Gio", value / 1_073_741_824.),
        Some(value) => format!("{value:.1} {}", unit(m.unit)),
        None => format!(
            "Indisponible ({})",
            m.error.as_deref().unwrap_or("valeur absente")
        ),
    };
    format!("{title} · {} : {value}", m.label)
}
fn collect(detailed: bool) -> Reading {
    let devices = match DeviceSensors::discover_all() {
        Ok(devices) => devices,
        Err(error) => {
            return Reading {
                summary: format!("Surveillance AMDGPU indisponible : {error}"),
                sources: String::new(),
                occupancy: BTreeMap::new(),
            }
        }
    };
    if devices.is_empty() {
        return Reading {summary: "Aucun périphérique PCI lié au pilote amdgpu détecté. Télémétrie Radeon indisponible dans cet environnement.".into(), sources: String::new(), occupancy: BTreeMap::new()};
    }
    let mut summary = Vec::new();
    let mut sources = Vec::new();
    let mut occupancy = BTreeMap::new();
    for device in devices {
        let device = match device {
            Ok(device) => device,
            Err(error) => {
                summary.push(error);
                continue;
            }
        };
        let identity = format!("GPU AMD · {} · {}", device.pci_address, device.driver);
        summary.push(identity.clone());
        sources.push(identity);
        let values = device.read();
        occupancy.insert(
            device.pci_address.clone(),
            values
                .iter()
                .find(|m| m.metric == Metric::Utilization)
                .and_then(|m| m.value),
        );
        if values.is_empty() {
            summary.push("Aucun capteur disponible".into());
        }
        for m in values {
            if detailed
                || matches!(
                    m.metric,
                    Metric::Temperature | Metric::Utilization | Metric::Power
                )
            {
                summary.push(format_measurement(&m));
            }
            sources.push(format!(
                "{} · {} · Unix {} ms{}",
                m.label,
                m.source,
                m.captured_unix_millis,
                m.error
                    .as_ref()
                    .map(|e| format!(" · {e}"))
                    .unwrap_or_default()
            ));
        }
        for metric in [
            Metric::Temperature,
            Metric::CoreClock,
            Metric::MemoryClock,
            Metric::Power,
            Metric::FanSpeed,
            Metric::Utilization,
            Metric::VramUsed,
            Metric::VramTotal,
        ] {
            if !device.sensors.iter().any(|s| s.metric == metric) && detailed {
                summary.push(format!("{} : capteur non exposé", metric_title(metric)));
            }
        }
        sources.extend(device.discovery_errors);
    }
    Reading {
        summary: summary.join("\n"),
        sources: sources.join("\n"),
        occupancy,
    }
}
pub fn panel(stack: &gtk::Stack, detailed: bool) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
    panel.add_css_class("gs-info-panel");
    let title = gtk::Label::new(Some("GPU AMD · relevés en direct"));
    title.set_xalign(0.);
    title.add_css_class("heading");
    panel.append(&title);
    let summary = gtk::Label::new(Some("Détection en attente…"));
    summary.set_xalign(0.);
    summary.set_wrap(true);
    summary.set_selectable(true);
    panel.append(&summary);
    let charts_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    panel.append(&charts_box);
    let sources = gtk::Label::new(None);
    sources.set_xalign(0.);
    sources.set_wrap(true);
    sources.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    sources.set_selectable(true);
    let advanced = gtk::Expander::new(Some("Sources, horodatages et erreurs"));
    advanced.set_child(Some(&sources));
    panel.append(&advanced);
    let note = gtk::Label::new(Some("Occupation globale du GPU, autres applications incluses. Capteurs selon disponibilité ; intervalle réglable dans Paramètres."));
    note.set_xalign(0.);
    note.set_wrap(true);
    note.add_css_class("dim-label");
    panel.append(&note);
    let weak_panel = panel.downgrade();
    let weak_stack = stack.downgrade();
    let weak_summary = summary.downgrade();
    let weak_sources = sources.downgrade();
    let weak_charts_box = charts_box.downgrade();
    let mut charts = BTreeMap::<String, crate::live_chart::PercentChart>::new();
    let mut was_visible = false;
    let mut pending: Option<mpsc::Receiver<Reading>> = None;
    let mut discard_pending = false;
    let mut last = Instant::now() - Duration::from_secs(5);
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let (Some(_panel), Some(stack), Some(summary), Some(sources)) = (
            weak_panel.upgrade(),
            weak_stack.upgrade(),
            weak_summary.upgrade(),
            weak_sources.upgrade(),
        ) else {
            return glib::ControlFlow::Break;
        };
        let target = if detailed {
            "Carte graphique"
        } else {
            "overview"
        };
        if stack.visible_child_name().as_deref() != Some(target)
            || crate::live_settings::benchmark_running()
        {
            if was_visible {
                for chart in charts.values() {
                    chart.push(None);
                }
            }
            was_visible = false;
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
                    summary.set_text(&reading.summary);
                    sources.set_text(&reading.sources);
                    if let Some(container) = weak_charts_box.upgrade() {
                        // Remove disappeared devices so repeated hotplug cannot grow the widget set.
                        charts.retain(|pci, chart| {
                            if reading.occupancy.contains_key(pci) {
                                true
                            } else {
                                if let Some(label) = chart.widget.prev_sibling() {
                                    container.remove(&label);
                                }
                                container.remove(&chart.widget);
                                false
                            }
                        });
                        for (pci, value) in &reading.occupancy {
                            let chart = charts.entry(pci.clone()).or_insert_with(|| {
                                let title = format!("Occupation GPU · {pci} · %");
                                let label = gtk::Label::new(Some(&title));
                                label.set_xalign(0.);
                                container.append(&label);
                                let chart = crate::live_chart::PercentChart::new(&title);
                                container.append(&chart.widget);
                                chart
                            });
                            chart.push(*value);
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    pending = None;
                    summary.set_text("Collecte GPU interrompue");
                    for chart in charts.values() {
                        chart.push(None);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        if pending.is_none() && last.elapsed() >= crate::live_settings::interval() {
            let (sender, receiver) = mpsc::sync_channel(1);
            match std::thread::Builder::new()
                .name("gameshift-live-gpu".into())
                .spawn(move || {
                    let _ = sender.send(collect(detailed));
                }) {
                Ok(_) => pending = Some(receiver),
                Err(error) => {
                    summary.set_text(&format!("Collecte GPU indisponible : {error}"));
                    for chart in charts.values() {
                        chart.push(None);
                    }
                }
            }
            last = Instant::now();
        }
        glib::ControlFlow::Continue
    });
    panel
}
