//! Shared GPU measurement labels; home telemetry now lives in overview.
use gameshift_telemetry::device::{Measurement, Metric, Unit};
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
