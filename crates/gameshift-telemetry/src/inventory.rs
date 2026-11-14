//! Shared discovery for monitoring workers. Explicit diagnostic/control discovery stays fresh.
use crate::{
    device::{DeviceSensors, Measurement},
    gpus::{self, GenericSensor, Gpu, SensorValue},
};
use gameshift_core::discovery::Cache;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

pub struct Inventory {
    pub gpus: Vec<Gpu>,
    pub devices: Vec<DeviceSensors>,
    pub errors: Vec<String>,
    generic: BTreeMap<String, Vec<GenericSensor>>,
    fan: BTreeMap<String, gameshift_control::Sysfs>,
}
impl Inventory {
    fn discover(root: &Path) -> (Self, Vec<PathBuf>) {
        let mut watched: Vec<PathBuf> = ["sys/bus/pci/devices", "sys/class/drm", "sys/class/hwmon"]
            .iter()
            .map(|p| root.join(p))
            .collect();
        let mut out = Self {
            gpus: Vec::new(),
            devices: Vec::new(),
            errors: Vec::new(),
            generic: BTreeMap::new(),
            fan: BTreeMap::new(),
        };
        match gpus::discover_at(root) {
            Ok(gpus) => out.gpus = gpus,
            Err(e) => out.errors.push(e),
        }
        for gpu in &out.gpus {
            let base = root.join("sys/bus/pci/devices").join(&gpu.pci);
            watched.extend([
                base.clone(),
                base.join("driver"),
                base.join("hwmon"),
                base.join("drm"),
            ]);
            if gpu.driver.as_deref() == Some("amdgpu") {
                match DeviceSensors::discover_at(root, &gpu.pci) {
                    Ok(device) => {
                        watched.extend(
                            device
                                .sensors
                                .iter()
                                .filter_map(|s| s.path.parent().map(Path::to_path_buf)),
                        );
                        out.errors.extend(device.discovery_errors.iter().cloned());
                        out.devices.push(device);
                    }
                    Err(e) => out.errors.push(e),
                }
                if root == Path::new("/") {
                    if let Ok(io) = gameshift_control::Sysfs::open(&gpu.pci) {
                        out.fan.insert(gpu.pci.clone(), io);
                    }
                }
            }
            let paths = gpus::generic_paths_at(root, gpu);
            watched.extend(
                paths
                    .iter()
                    .filter_map(|s| s.path.parent().map(Path::to_path_buf)),
            );
            out.generic.insert(gpu.pci.clone(), paths);
        }
        (out, watched)
    }
    pub fn read_device(&self, pci: &str) -> Vec<Measurement> {
        self.devices
            .iter()
            .find(|d| d.pci_address == pci)
            .map(DeviceSensors::read)
            .unwrap_or_default()
    }
    pub fn generic_sensors(&self, pci: &str) -> Vec<SensorValue> {
        self.generic
            .get(pci)
            .into_iter()
            .flatten()
            .filter_map(GenericSensor::read)
            .collect()
    }
    pub fn generic_sensor_count(&self, pci: &str) -> usize {
        self.generic.get(pci).map_or(0, Vec::len)
    }
    pub fn fan_curve(&self, pci: &str) -> Option<gameshift_control::Curve> {
        use gameshift_control::DeviceIo;
        self.fan
            .get(pci)?
            .read("gpu_od/fan_ctrl/fan_curve")
            .and_then(|s| gameshift_control::parse_curve(&s))
            .ok()
    }
}
#[derive(Default)]
pub struct Discovery {
    cache: Cache<Arc<Inventory>>,
}
impl Discovery {
    pub fn get_at(&mut self, root: &Path, now: Instant) -> Arc<Inventory> {
        let inventory = self
            .cache
            .get(root, now, |root| {
                let (inventory, watched) = Inventory::discover(root);
                (Arc::new(inventory), watched)
            })
            .clone();
        if !inventory.errors.is_empty() {
            self.cache.failed(true);
        }
        inventory
    }
    pub fn failed(&mut self, failed: bool) {
        self.cache.failed(failed);
    }
}
fn shared() -> &'static Mutex<Discovery> {
    static SHARED: OnceLock<Mutex<Discovery>> = OnceLock::new();
    SHARED.get_or_init(Default::default)
}
pub fn current() -> Arc<Inventory> {
    shared()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_at(Path::new("/"), Instant::now())
}
pub fn failed(failed: bool) {
    shared()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .failed(failed);
}
