//! Standard Linux GPU identities. Discovery never implies sensor or control support.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gpu {
    pub pci: String,
    pub name: String,
    pub vendor: String,
    pub device: String,
    pub driver: Option<String>,
    pub nodes: Vec<String>,
}
pub fn discover() -> Result<Vec<Gpu>, String> {
    discover_at(Path::new("/"))
}
pub fn discover_at(root: &Path) -> Result<Vec<Gpu>, String> {
    let entries = fs::read_dir(root.join("sys/bus/pci/devices")).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let base = entry.path();
        let read = |name: &str| {
            fs::read_to_string(base.join(name))
                .ok()
                .map(|s| s.trim().to_owned())
        };
        if !read("class").is_some_and(|v| v.starts_with("0x03")) {
            continue;
        }
        let pci = entry.file_name().to_string_lossy().into_owned();
        if gameshift_control::canonical_pci(&pci).is_err() {
            continue;
        }
        let vendor = read("vendor").unwrap_or_default();
        let device = read("device").unwrap_or_default();
        let brand = match vendor.as_str() {
            "0x1002" => "AMD",
            "0x10de" => "NVIDIA",
            "0x8086" => "Intel",
            _ => "GPU",
        };
        let driver = fs::read_link(base.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()));
        let mut nodes = Vec::new();
        if let Ok(entries) = fs::read_dir(base.join("drm")) {
            for n in entries.flatten() {
                let name = n.file_name().to_string_lossy().into_owned();
                if name
                    .strip_prefix("card")
                    .or_else(|| name.strip_prefix("renderD"))
                    .is_some_and(|s| s.parse::<u32>().is_ok())
                {
                    nodes.push(format!("/dev/dri/{name}"));
                }
            }
        }
        let name = read("product_name")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("{brand} {}", device.trim_start_matches("0x")));
        out.push(Gpu {
            pci,
            name,
            vendor,
            device,
            driver,
            nodes,
        });
    }
    out.sort_by(|a, b| a.pci.cmp(&b.pci));
    Ok(out)
}
#[derive(Debug, Clone, Serialize)]
pub struct SensorValue {
    pub label: String,
    pub value: f64,
    pub unit: String,
    pub source: String,
}
/// Only hwmon's labelled, documented units. Never synthesize a vendor utilization counter.
pub fn generic_sensors(gpu: &Gpu) -> Vec<SensorValue> {
    generic_sensors_at(Path::new("/"), gpu)
}
pub fn generic_sensors_at(root: &Path, gpu: &Gpu) -> Vec<SensorValue> {
    let mut out = Vec::new();
    let base = root
        .join("sys/bus/pci/devices")
        .join(&gpu.pci)
        .join("hwmon");
    let Ok(entries) = fs::read_dir(base) else {
        return out;
    };
    for hwmon in entries.flatten() {
        let Ok(files) = fs::read_dir(hwmon.path()) else {
            continue;
        };
        for file in files.flatten().take(256) {
            let name = file.file_name().to_string_lossy().into_owned();
            let Some(key) = name
                .strip_suffix("_input")
                .or_else(|| name.strip_suffix("_average"))
            else {
                continue;
            };
            let (unit, divisor) = if key
                .strip_prefix("temp")
                .is_some_and(|n| n.parse::<u32>().is_ok())
            {
                ("°C", 1000.)
            } else if key
                .strip_prefix("power")
                .is_some_and(|n| n.parse::<u32>().is_ok())
            {
                ("W", 1e6)
            } else if key
                .strip_prefix("freq")
                .is_some_and(|n| n.parse::<u32>().is_ok())
            {
                ("MHz", 1e6)
            } else if key
                .strip_prefix("fan")
                .is_some_and(|n| n.parse::<u32>().is_ok())
            {
                ("tr/min", 1.)
            } else {
                continue;
            };
            let Some(value) = fs::read_to_string(file.path())
                .ok()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= 0.)
            else {
                continue;
            };
            let label = fs::read_to_string(hwmon.path().join(format!("{key}_label")))
                .ok()
                .map(|s| s.trim().to_owned())
                .unwrap_or_else(|| key.into());
            out.push(SensorValue {
                label,
                value: value / divisor,
                unit: unit.into(),
                source: file.path().display().to_string(),
            });
        }
    }
    out
}
pub fn names() -> BTreeMap<String, String> {
    discover()
        .unwrap_or_default()
        .into_iter()
        .map(|g| (g.pci, g.name))
        .collect()
}
