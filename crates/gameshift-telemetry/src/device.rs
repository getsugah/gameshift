//! Read-only AMDGPU discovery. PCI identity owns sensors; hwmon indices are never identities.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    Temperature,
    CoreClock,
    MemoryClock,
    Power,
    FanSpeed,
    Utilization,
    VramUsed,
    VramTotal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    Celsius,
    Megahertz,
    Watts,
    Rpm,
    Percent,
    Bytes,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Measurement {
    pub metric: Metric,
    pub label: String,
    pub unit: Unit,
    pub source: String,
    pub captured_unix_millis: u64,
    pub value: Option<f64>,
    pub error: Option<String>,
}
#[derive(Debug, Clone)]
pub struct Sensor {
    pub metric: Metric,
    pub label: String,
    pub unit: Unit,
    pub path: PathBuf,
    divisor: f64,
    dpm: bool,
}
impl Sensor {
    pub fn read(&self) -> Measurement {
        let captured_unix_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let result = fs::read_to_string(&self.path)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                let value = if self.dpm {
                    super::sensors::active_mhz(&text)
                } else {
                    text.trim().parse::<f64>().ok().map(|v| v / self.divisor)
                }
                .filter(|v| v.is_finite() && *v >= 0.);
                value
                    .filter(|v| self.unit != Unit::Percent || *v <= 100.)
                    .ok_or_else(|| "Valeur du capteur invalide".to_string())
            });
        let (value, error) = match result {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e)),
        };
        Measurement {
            metric: self.metric,
            label: self.label.clone(),
            unit: self.unit,
            source: self.path.display().to_string(),
            captured_unix_millis,
            value,
            error,
        }
    }
}
#[derive(Debug)]
pub struct DeviceSensors {
    pub pci_address: String,
    pub driver: String,
    pub sensors: Vec<Sensor>,
    pub discovery_errors: Vec<String>,
}
impl DeviceSensors {
    pub fn discover(address: &str) -> Result<Self, String> {
        Self::discover_at(Path::new("/"), address)
    }
    /// Discover every device bound to amdgpu. Other PCI drivers are not GPU substitutes.
    pub fn discover_all() -> Result<Vec<Result<Self, String>>, String> {
        Self::discover_all_at(Path::new("/"))
    }
    pub fn discover_all_at(root: &Path) -> Result<Vec<Result<Self, String>>, String> {
        let base = root.join("sys/bus/pci/devices");
        let mut addresses = Vec::new();
        let mut failures = Vec::new();
        for entry in fs::read_dir(&base).map_err(|e| format!("{} : {e}", base.display()))? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    failures.push(Err(error.to_string()));
                    continue;
                }
            };
            let driver_path = entry.path().join("driver");
            match fs::read_link(&driver_path) {
                Ok(driver) if driver.file_name().is_some_and(|n| n == "amdgpu") => {
                    addresses.push(entry.file_name().to_string_lossy().into_owned());
                }
                Ok(_) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => failures.push(Err(format!("{} : {error}", driver_path.display()))),
            }
        }
        addresses.sort();
        failures.extend(addresses.into_iter().map(|address| {
            Self::discover_at(root, &address).map_err(|e| format!("{address} : {e}"))
        }));
        Ok(failures)
    }
    /// Injectable filesystem root for tests. No device index or model assumption.
    pub fn discover_at(root: &Path, address: &str) -> Result<Self, String> {
        let address = super::canonical_address(address).ok_or("Adresse PCI invalide")?;
        let base = root.join("sys/bus/pci/devices").join(&address);
        let driver = fs::read_link(base.join("driver"))
            .map_err(|e| format!("Pilote PCI indisponible : {e}"))?;
        let driver = driver
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("Pilote PCI invalide")?
            .to_owned();
        if driver != "amdgpu" {
            return Err(format!(
                "Pilote {driver} : collecteur AMDGPU non applicable"
            ));
        }
        let mut device = Self {
            pci_address: address,
            driver,
            sensors: vec![],
            discovery_errors: vec![],
        };
        match fs::read_dir(base.join("hwmon")) {
            Ok(entries) => {
                let mut paths = Vec::new();
                for entry in entries {
                    match entry {
                        Ok(e) => paths.push(e.path()),
                        Err(e) => device.discovery_errors.push(e.to_string()),
                    }
                }
                paths.sort();
                for hwmon in paths {
                    match fs::read_to_string(hwmon.join("name")) {
                        Ok(name) if name.trim() == "amdgpu" => (),
                        Ok(_) => continue,
                        Err(e) => {
                            device
                                .discovery_errors
                                .push(format!("{} : {e}", hwmon.display()));
                            continue;
                        }
                    }
                    let entries = match fs::read_dir(&hwmon) {
                        Ok(e) => e,
                        Err(e) => {
                            device.discovery_errors.push(e.to_string());
                            continue;
                        }
                    };
                    let mut inputs = Vec::new();
                    for entry in entries {
                        match entry {
                            Ok(e) => inputs.push(e.path()),
                            Err(e) => device.discovery_errors.push(e.to_string()),
                        }
                    }
                    inputs.sort();
                    for path in inputs {
                        let Some(stem) = path.file_name().and_then(|s| s.to_str()).and_then(|s| {
                            s.strip_suffix("_input")
                                .or_else(|| s.strip_suffix("_average"))
                        }) else {
                            continue;
                        };
                        let label = fs::read_to_string(hwmon.join(format!("{stem}_label")))
                            .ok()
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| stem.to_string());
                        let numbered = |prefix: &str| {
                            stem.strip_prefix(prefix).is_some_and(|s| {
                                !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
                            })
                        };
                        let kind = if numbered("temp") {
                            Some((Metric::Temperature, Unit::Celsius, 1000.))
                        } else if numbered("freq") {
                            match label.to_ascii_lowercase().as_str() {
                                "sclk" | "gfx" | "gfxclk" => {
                                    Some((Metric::CoreClock, Unit::Megahertz, 1e6))
                                }
                                "mclk" | "mem" | "memclk" => {
                                    Some((Metric::MemoryClock, Unit::Megahertz, 1e6))
                                }
                                _ => None,
                            }
                        } else if numbered("power") {
                            Some((Metric::Power, Unit::Watts, 1e6))
                        } else if numbered("fan") {
                            Some((Metric::FanSpeed, Unit::Rpm, 1.))
                        } else {
                            None
                        };
                        if let Some((metric, unit, divisor)) = kind {
                            device.sensors.push(Sensor {
                                metric,
                                label,
                                unit,
                                path,
                                divisor,
                                dpm: false,
                            });
                        }
                    }
                }
            }
            Err(e) => device
                .discovery_errors
                .push(format!("{} : {e}", base.join("hwmon").display())),
        }
        for (file, metric, unit, dpm) in [
            (
                "gpu_busy_percent",
                Metric::Utilization,
                Unit::Percent,
                false,
            ),
            ("mem_info_vram_used", Metric::VramUsed, Unit::Bytes, false),
            ("mem_info_vram_total", Metric::VramTotal, Unit::Bytes, false),
            ("pp_dpm_sclk", Metric::CoreClock, Unit::Megahertz, true),
            ("pp_dpm_mclk", Metric::MemoryClock, Unit::Megahertz, true),
        ] {
            if dpm && device.sensors.iter().any(|s| s.metric == metric) {
                continue;
            }
            let path = base.join(file);
            let exists = match path.try_exists() {
                Ok(exists) => exists,
                Err(error) => {
                    device
                        .discovery_errors
                        .push(format!("{} : {error}", path.display()));
                    false
                }
            };
            if exists {
                device.sensors.push(Sensor {
                    metric,
                    label: file.into(),
                    unit,
                    path,
                    divisor: 1.,
                    dpm,
                });
            }
        }
        Ok(device)
    }
    pub fn read(&self) -> Vec<Measurement> {
        self.sensors.iter().map(Sensor::read).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "gameshift-sensors-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn device(&self, pci: &str, driver: &str, hwmon: &str) -> PathBuf {
            let base = self.0.join("sys/bus/pci/devices").join(pci);
            let monitor = base.join("hwmon").join(hwmon);
            fs::create_dir_all(&monitor).unwrap();
            std::os::unix::fs::symlink(
                format!("/sys/bus/pci/drivers/{driver}"),
                base.join("driver"),
            )
            .unwrap();
            fs::write(monitor.join("name"), "amdgpu\n").unwrap();
            monitor
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn one_invalid_driver_entry_does_not_hide_a_healthy_gpu() {
        let fixture = Fixture::new();
        fixture.device("0000:09:00.0", "amdgpu", "hwmon73");
        let broken = fixture.0.join("sys/bus/pci/devices/0000:0a:00.0/driver");
        fs::create_dir_all(broken).unwrap();
        let results = DeviceSensors::discover_all_at(&fixture.0).unwrap();
        assert!(results.iter().any(|r| r.is_err()));
        assert!(results
            .iter()
            .any(|r| r.as_ref().is_ok_and(|d| d.pci_address == "0000:09:00.0")));
    }
    #[test]
    fn enumerates_all_amdgpu_devices_without_including_virtual_drivers() {
        let fixture = Fixture::new();
        fixture.device("0000:0a:00.0", "amdgpu", "hwmon73");
        fixture.device("0000:09:00.0", "amdgpu", "hwmon22");
        fixture.device("0000:01:00.0", "virtio-pci", "hwmon1");
        let devices = DeviceSensors::discover_all_at(&fixture.0).unwrap();
        let addresses = devices
            .into_iter()
            .map(|d| d.unwrap().pci_address)
            .collect::<Vec<_>>();
        assert_eq!(addresses, ["0000:09:00.0", "0000:0a:00.0"]);
    }
    #[test]
    fn isolates_pci_devices_and_reads_labels_without_fixed_indices() {
        let fixture = Fixture::new();
        let monitor = fixture.device("0000:09:00.0", "amdgpu", "hwmon73");
        let other = fixture.device("0000:0a:00.0", "amdgpu", "hwmon2");
        fs::write(other.join("temp1_input"), "99000").unwrap();
        for (file, value) in [
            ("temp7_input", "61000"),
            ("temp7_label", "junction"),
            ("freq8_input", "2300000000"),
            ("freq8_label", "sclk"),
            ("power9_average", "140000000"),
            ("fan4_input", "1200"),
        ] {
            fs::write(monitor.join(file), value).unwrap();
        }
        let device = DeviceSensors::discover_at(&fixture.0, "0000:09:00.0").unwrap();
        let values = device.read();
        assert_eq!(values.len(), 4);
        assert!(values.iter().all(|v| v.source.contains("0000:09:00.0")
            && v.error.is_none()
            && v.captured_unix_millis > 0));
        assert!(values
            .iter()
            .any(|v| v.label == "junction" && v.value == Some(61.) && v.unit == Unit::Celsius));
        assert!(values
            .iter()
            .any(|v| v.metric == Metric::CoreClock && v.value == Some(2300.)));
        assert!(values
            .iter()
            .any(|v| v.unit == Unit::Watts && v.value == Some(140.)));
    }
    #[test]
    fn missing_invalid_and_disappearing_sensors_never_become_zero() {
        let fixture = Fixture::new();
        let monitor = fixture.device("0000:09:00.0", "amdgpu", "hwmon42");
        fs::write(monitor.join("temp8_input"), "NaN").unwrap();
        let device = DeviceSensors::discover_at(&fixture.0, "0000:09:00.0").unwrap();
        assert_eq!(device.read()[0].value, None);
        fs::remove_file(monitor.join("temp8_input")).unwrap();
        let value = device.read().remove(0);
        assert!(value.value.is_none() && value.error.is_some());
        assert!(DeviceSensors::discover_at(&fixture.0, "../../etc/passwd").is_err());
        assert!(DeviceSensors::discover_at(&fixture.0, "0000:01:00.0").is_err());
    }
    #[test]
    fn rejects_other_drivers_and_falls_back_to_dpm_without_guessing_frequency_labels() {
        let fixture = Fixture::new();
        fixture.device("0000:01:00.0", "virtio-pci", "hwmon0");
        assert!(DeviceSensors::discover_at(&fixture.0, "0000:01:00.0").is_err());
        let monitor = fixture.device("0000:09:00.0", "amdgpu", "hwmon91");
        fs::write(monitor.join("freq4_input"), "999000000").unwrap();
        fs::write(
            monitor
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("pp_dpm_sclk"),
            "0: 500Mhz\n1: 2.4GHz *",
        )
        .unwrap();
        let device = DeviceSensors::discover_at(&fixture.0, "0000:09:00.0").unwrap();
        assert_eq!(device.read().len(), 1);
        assert_eq!(device.read()[0].value, Some(2400.));
    }
}
