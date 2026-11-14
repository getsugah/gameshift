//! Headless sysfs fixtures: no GTK initialization, helpers, or hardware writes.
use super::home_sensors;
use gameshift_cpu::environment::TemperatureSampler;
use gameshift_telemetry::{
    device::{Metric, Unit},
    inventory::Discovery,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "gameshift-discovery-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let fixture = Self(root);
        fixture.write("sys/devices/platform/cpu/hwmon/hwmon0/name", "k10temp");
        fixture.write("sys/devices/platform/cpu/hwmon/hwmon0/temp1_label", "Tctl");
        fixture.write("sys/devices/platform/cpu/hwmon/hwmon0/temp1_input", "44125");
        fixture.link(
            "sys/devices/platform/cpu/hwmon/hwmon0",
            "sys/class/hwmon/hwmon0",
        );
        for (pci, hwmon, temperature) in [
            ("0000:03:00.0", "hwmon1", "36000"),
            ("0000:08:00.0", "hwmon2", "52000"),
        ] {
            fixture.gpu(pci, hwmon, "amdgpu", "0x1002", temperature);
        }
        fixture.gpu("0000:09:00.0", "hwmon3", "nouveau", "0x10de", "48000");
        fixture
    }
    fn write(&self, path: &str, value: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }
    fn link(&self, target: &str, path: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(self.0.join(target), path).unwrap();
    }
    fn gpu(&self, pci: &str, hwmon: &str, driver: &str, vendor: &str, temp: &str) {
        fs::create_dir_all(self.0.join(format!("sys/bus/pci/drivers/{driver}"))).unwrap();
        let base = format!("sys/bus/pci/devices/{pci}");
        for (file, value) in [
            ("class", "0x030000"),
            ("vendor", vendor),
            ("device", "0x747e"),
            ("product_name", driver),
            ("gpu_busy_percent", "27"),
            ("mem_info_vram_used", "2147483648"),
            ("mem_info_vram_total", "17179869184"),
        ] {
            self.write(&format!("{base}/{file}"), value);
        }
        self.link(
            &format!("sys/bus/pci/drivers/{driver}"),
            &format!("{base}/driver"),
        );
        let monitor = format!("{base}/hwmon/{hwmon}");
        for (file, value) in [
            ("name", driver),
            ("temp1_label", "edge"),
            ("temp1_input", temp),
            ("freq1_label", "sclk"),
            ("freq1_input", "2400000000"),
            ("power1_average", "140500000"),
            ("fan1_input", "1200"),
        ] {
            self.write(&format!("{monitor}/{file}"), value);
        }
        self.link(&monitor, &format!("sys/class/hwmon/{hwmon}"));
        let card = hwmon.strip_prefix("hwmon").unwrap();
        fs::create_dir_all(self.0.join(format!("{base}/drm/card{card}"))).unwrap();
        self.link(&base, &format!("sys/class/drm/card{card}/device"));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn values(
    discovery: &mut Discovery,
    root: &Path,
    now: Instant,
) -> Vec<(String, Metric, Unit, Option<f64>, String)> {
    let inventory = discovery.get_at(root, now);
    inventory
        .devices
        .iter()
        .flat_map(|d| {
            d.read()
                .into_iter()
                .map(|m| (d.pci_address.clone(), m.metric, m.unit, m.value, m.label))
        })
        .collect()
}
#[test]
fn cached_values_match_discovery_and_dynamic_values_remain_fresh() {
    let fixture = Fixture::new();
    let now = Instant::now();
    let mut cached = Discovery::default();
    let first = values(&mut cached, &fixture.0, now);
    assert_eq!(first.len(), 14);
    assert!(first
        .iter()
        .any(|(pci, _, _, v, _)| pci == "0000:08:00.0" && *v == Some(52.)));
    let before = cached.get_at(&fixture.0, now);
    let legacy = gameshift_telemetry::engines::discover_at(&fixture.0).unwrap();
    let monitored: Vec<_> = before
        .gpus
        .iter()
        .filter(|g| {
            g.driver.as_deref() == Some("amdgpu")
                && g.nodes.iter().any(|n| n.starts_with("/dev/dri/card"))
        })
        .map(|g| &g.pci)
        .collect();
    assert_eq!(monitored, legacy.iter().map(|g| &g.pci).collect::<Vec<_>>());
    assert_eq!(monitored.len(), 2);
    for cycle in 1..=6 {
        fixture.write(
            "sys/bus/pci/devices/0000:03:00.0/hwmon/hwmon1/temp1_input",
            &(36000 + cycle * 125).to_string(),
        );
        let time = now + Duration::from_millis(cycle as u64 * 500);
        assert_eq!(
            values(&mut cached, &fixture.0, time),
            values(&mut Discovery::default(), &fixture.0, time)
        );
        assert!(std::sync::Arc::ptr_eq(
            &before,
            &cached.get_at(&fixture.0, time)
        ));
    }
    assert_eq!(before.generic_sensors("0000:09:00.0").len(), 4);
    assert!(first
        .iter()
        .any(|(_, metric, unit, value, _)| *metric == Metric::Power
            && *unit == Unit::Watts
            && *value == Some(140.5)));
    assert!(first
        .iter()
        .any(|(_, metric, unit, value, _)| *metric == Metric::CoreClock
            && *unit == Unit::Megahertz
            && *value == Some(2400.)));
    assert!(first
        .iter()
        .any(|(_, metric, unit, value, _)| *metric == Metric::Utilization
            && *unit == Unit::Percent
            && *value == Some(27.)));
    assert!(first
        .iter()
        .any(|(_, metric, unit, value, _)| *metric == Metric::VramTotal
            && *unit == Unit::Bytes
            && *value == Some(17179869184.)));
}
#[test]
fn metadata_is_not_reopened_between_samples_and_cpu_values_are_not_cached() {
    let fixture = Fixture::new();
    let now = Instant::now();
    let mut cpu = TemperatureSampler::default();
    let mut home = home_sensors::Sampler::default();
    let first = home.sample_at(&fixture.0, now);
    assert_eq!(cpu.sample_at(&fixture.0, now)[0].celsius, 44.125);
    fixture.write("sys/devices/platform/cpu/hwmon/hwmon0/temp1_input", "45125");
    fs::remove_file(fixture.0.join("sys/devices/platform/cpu/hwmon/hwmon0/name")).unwrap();
    fs::remove_file(
        fixture
            .0
            .join("sys/devices/platform/cpu/hwmon/hwmon0/temp1_label"),
    )
    .unwrap();
    let reading = cpu.sample_at(&fixture.0, now + Duration::from_millis(500));
    assert_eq!(reading[0].source, "k10temp/Tctl");
    assert_eq!(reading[0].celsius, 45.125);
    let next = home.sample_at(&fixture.0, now + Duration::from_millis(500));
    assert_eq!(
        first.iter().map(|s| &s.id).collect::<Vec<_>>(),
        next.iter().map(|s| &s.id).collect::<Vec<_>>()
    );
    assert!(next
        .iter()
        .any(|s| s.label == "CPU Tctl" && s.value == Some(45.125)));
}
#[test]
fn disappearance_renumbering_and_hotplug_preserve_other_gpus() {
    let fixture = Fixture::new();
    let now = Instant::now();
    let mut cached = Discovery::default();
    let old = cached.get_at(&fixture.0, now);
    let monitor = fixture
        .0
        .join("sys/bus/pci/devices/0000:03:00.0/hwmon/hwmon1");
    fs::rename(&monitor, monitor.with_file_name("hwmon71")).unwrap();
    let readings = old.read_device("0000:03:00.0");
    assert!(readings
        .iter()
        .filter(|m| m.metric == Metric::Temperature)
        .all(|m| m.value.is_none() && m.error.is_some()));
    for _ in 0..3 {
        cached.failed(true);
    }
    assert!(std::sync::Arc::ptr_eq(
        &old,
        &cached.get_at(&fixture.0, now + Duration::from_secs(4))
    ));
    let recovered = cached.get_at(&fixture.0, now + Duration::from_secs(5));
    assert!(recovered
        .read_device("0000:03:00.0")
        .iter()
        .any(|m| m.value == Some(36.) && m.source.contains("hwmon71")));
    assert!(recovered
        .read_device("0000:08:00.0")
        .iter()
        .any(|m| m.value == Some(52.)));
    fixture.gpu("0000:0a:00.0", "hwmon4", "amdgpu", "0x1002", "58000");
    // Operations run within one filesystem timestamp tick; inject the topology
    // timestamp instead of depending on wall-clock scheduling in this test.
    fs::File::open(fixture.0.join("sys/bus/pci/devices"))
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(10))
        .unwrap();
    let added = cached.get_at(&fixture.0, now + Duration::from_secs(10));
    assert_eq!(added.gpus.len(), 4);
    assert!(added
        .read_device("0000:0a:00.0")
        .iter()
        .any(|m| m.value == Some(58.)));
    // A PCI device disappears entirely: no stale sample, then no stale identity.
    fs::remove_dir_all(fixture.0.join("sys/bus/pci/devices/0000:03:00.0")).unwrap();
    fs::File::open(fixture.0.join("sys/bus/pci/devices"))
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(15))
        .unwrap();
    assert!(added
        .read_device("0000:03:00.0")
        .iter()
        .all(|m| m.value.is_none()));
    assert_eq!(
        cached
            .get_at(&fixture.0, now + Duration::from_secs(15))
            .gpus
            .len(),
        3
    );
}
#[test]
fn invalid_values_retry_without_rebuilding_each_cycle() {
    let fixture = Fixture::new();
    let now = Instant::now();
    let mut cached = Discovery::default();
    let initial = cached.get_at(&fixture.0, now);
    fixture.write(
        "sys/bus/pci/devices/0000:03:00.0/hwmon/hwmon1/temp1_input",
        "NaN",
    );
    for i in 1..=9 {
        let current = cached.get_at(&fixture.0, now + Duration::from_millis(i * 500));
        assert!(std::sync::Arc::ptr_eq(&current, &initial));
        let sample = current.read_device("0000:03:00.0");
        assert!(sample
            .iter()
            .any(|m| m.metric == Metric::Temperature && m.error.is_some()));
        cached.failed(true);
    }
    let refreshed = cached.get_at(&fixture.0, now + Duration::from_secs(5));
    assert!(!std::sync::Arc::ptr_eq(&initial, &refreshed));
    fixture.write(
        "sys/bus/pci/devices/0000:03:00.0/hwmon/hwmon1/temp1_input",
        "37000",
    );
    assert!(refreshed
        .read_device("0000:03:00.0")
        .iter()
        .any(|m| m.value == Some(37.)));
}
#[test]
fn cached_cpu_and_selected_temperature_ids_survive_hwmon_renumbering() {
    let fixture = Fixture::new();
    let now = Instant::now();
    let mut home = home_sensors::Sampler::default();
    let mut cpu = TemperatureSampler::default();
    let first = home.sample_at(&fixture.0, now);
    assert_eq!(cpu.sample_at(&fixture.0, now)[0].source, "k10temp/Tctl");
    let old = fixture.0.join("sys/devices/platform/cpu/hwmon/hwmon0");
    fs::rename(&old, old.with_file_name("hwmon70")).unwrap();
    fs::remove_file(fixture.0.join("sys/class/hwmon/hwmon0")).unwrap();
    fixture.link(
        "sys/devices/platform/cpu/hwmon/hwmon70",
        "sys/class/hwmon/hwmon70",
    );
    assert!(cpu
        .sample_at(&fixture.0, now + Duration::from_millis(500))
        .is_empty());
    let recovered = home.sample_at(&fixture.0, now + Duration::from_secs(5));
    assert_eq!(
        first.iter().map(|s| &s.id).collect::<Vec<_>>(),
        recovered.iter().map(|s| &s.id).collect::<Vec<_>>()
    );
    assert!(recovered
        .iter()
        .any(|s| s.source.contains("hwmon70") && s.value == Some(44.125)));
    assert_eq!(
        cpu.sample_at(&fixture.0, now + Duration::from_secs(5))[0].celsius,
        44.125
    );
}
#[test]
fn rebind_and_delayed_metadata_audit_refresh_gpu_identity() {
    let fixture = Fixture::new();
    let now = Instant::now();
    let mut discovery = Discovery::default();
    let first = discovery.get_at(&fixture.0, now);
    let driver = fixture.0.join("sys/bus/pci/devices/0000:03:00.0/driver");
    fs::remove_file(&driver).unwrap();
    fixture.link(
        "sys/bus/pci/drivers/nouveau",
        "sys/bus/pci/devices/0000:03:00.0/driver",
    );
    let rebound = discovery.get_at(&fixture.0, now + Duration::from_secs(5));
    assert!(!std::sync::Arc::ptr_eq(&first, &rebound));
    assert!(rebound.read_device("0000:03:00.0").is_empty());
    assert!(rebound
        .read_device("0000:08:00.0")
        .iter()
        .any(|m| m.value == Some(52.)));
    fixture.write(
        "sys/bus/pci/devices/0000:08:00.0/product_name",
        "Updated GPU",
    );
    // In-place metadata writes do not change a directory timestamp: bounded fallback audit.
    assert_ne!(
        discovery
            .get_at(&fixture.0, now + Duration::from_secs(6))
            .gpus[1]
            .name,
        "Updated GPU"
    );
    assert_eq!(
        discovery
            .get_at(&fixture.0, now + Duration::from_secs(65))
            .gpus[1]
            .name,
        "Updated GPU"
    );
}
#[test]
#[ignore = "Headless I/O probe; run alone with GAMESHIFT_DISCOVERY_BASELINE=1 for the old rediscovery pattern"]
fn discovery_io_probe() {
    let fixture = Fixture::new();
    let mut cpu = TemperatureSampler::default();
    let mut home = home_sensors::Sampler::default();
    let mut gpu = Discovery::default();
    let baseline = std::env::var_os("GAMESHIFT_DISCOVERY_BASELINE").is_some();
    let now = Instant::now();
    for cycle in 0..6 {
        if baseline {
            cpu = TemperatureSampler::default();
            home = home_sensors::Sampler::default();
            gpu = Discovery::default();
        }
        let time = now + Duration::from_millis(cycle * 500);
        assert_eq!(cpu.sample_at(&fixture.0, time)[0].celsius, 44.125);
        assert_eq!(home.sample_at(&fixture.0, time).len(), 4);
        assert_eq!(values(&mut gpu, &fixture.0, time).len(), 14);
    }
    println!(
        "discovery probe: 6 cycles, 3 GPUs, 1 CPU, mode={}",
        if baseline { "rediscover" } else { "cached" }
    );
}
