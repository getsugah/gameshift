use super::{gpus, processes::*};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("gameshift-fdinfo-{}-{n}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn gpu(&self, pci: &str, vendor: &str, driver: &str) {
        let path = self.0.join("sys/bus/pci/devices").join(pci);
        fs::create_dir_all(path.join("drm/renderD140")).unwrap();
        fs::write(path.join("vendor"), vendor).unwrap();
        fs::write(path.join("device"), "0x1234").unwrap();
        fs::write(path.join("class"), "0x030000").unwrap();
        std::os::unix::fs::symlink(
            format!("/sys/bus/pci/drivers/{driver}"),
            path.join("driver"),
        )
        .unwrap();
    }
    fn process(&self, pid: u32, start: u64) {
        let path = self.0.join("proc").join(pid.to_string());
        fs::create_dir_all(path.join("fdinfo")).unwrap();
        fs::create_dir_all(path.join("fd")).unwrap();
        let mut fields = vec!["0".to_owned(); 20];
        fields[0] = "R".into();
        fields[19] = start.to_string();
        fs::write(
            path.join("stat"),
            format!("{pid} (a process with ) parentheses) {}", fields.join(" ")),
        )
        .unwrap();
        fs::write(path.join("comm"), "application").unwrap();
    }
    fn info(&self, pid: u32, fd: u32, pci: &str, id: u32, ns: u64) {
        fs::write(self.0.join(format!("proc/{pid}/fdinfo/{fd}")),format!("drm-driver: amdgpu\ndrm-pdev: {pci}\ndrm-client-id: {id}\ndrm-engine-gfx: {ns} ns\ndrm-engine-capacity-gfx: 2\ndrm-memory-vram: 10 KiB\ndrm-memory-gtt: 2 MiB\n")).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn parser_respects_units_resident_precedence_and_unknown_counters() {
    let c=parse_fdinfo("drm-driver: xe\ndrm-client-id: 7\ndrm-memory-vram: 9 KiB\ndrm-resident-vram: 2 MiB\ndrm-resident-system: 512\ndrm-engine-render: 100000 ns\ndrm-engine-capacity-render: 2\ndrm-engine-video: 20 bananas\ndrm-cycles-compute: 12\n").unwrap();
    assert_eq!(c.memory_bytes["vram"], 2 * 1048576);
    assert_eq!(c.memory_bytes["system"], 512);
    assert_eq!(c.engines_ns["render"], 100000);
    assert!(!c.engines_ns.contains_key("video"));
    assert!(!c.engines_ns.contains_key("compute"));
    assert!(parse_fdinfo("pos: 0\nflags: 0100\n").is_none());
}
#[test]
fn discovery_is_vendor_neutral_and_never_assumes_indices() {
    let f = Fixture::new();
    for (pci, vendor, driver) in [
        ("0000:04:00.0", "0x10de", "nvidia"),
        ("0000:02:00.0", "0x8086", "xe"),
        ("0000:09:00.0", "0x1002", "amdgpu"),
    ] {
        f.gpu(pci, vendor, driver);
    }
    let devices = gpus::discover_at(&f.0).unwrap();
    assert_eq!(devices.len(), 3);
    assert_eq!(devices[0].driver.as_deref(), Some("xe"));
    assert_eq!(devices[1].driver.as_deref(), Some("nvidia"));
    assert!(devices.iter().all(|g| g.nodes == ["/dev/dri/renderD140"]));
}
#[test]
fn duplicated_fds_shared_clients_and_same_ids_on_other_gpus_are_separate() {
    let f = Fixture::new();
    f.gpu("0000:09:00.0", "0x1002", "amdgpu");
    f.gpu("0000:04:00.0", "0x8086", "i915");
    for pid in [42, 43] {
        f.process(pid, 100);
        f.info(pid, 3, "0000:09:00.0", 7, 0);
    }
    f.info(42, 4, "0000:09:00.0", 7, 0);
    f.info(42, 5, "0000:04:00.0", 7, 0);
    let snapshot = Tracker::default().sample_at(&f.0, Instant::now());
    assert_eq!(snapshot.rows.len(), 3);
    let row = snapshot
        .rows
        .iter()
        .find(|r| r.pid == 42 && r.pci == "0000:09:00.0")
        .unwrap();
    assert_eq!(row.clients, 1);
    assert_eq!(row.memory_bytes["vram"], 10 * 1024);
    assert!(row.shared_clients);
    assert_eq!(row.activity(), None);
}
#[test]
fn deltas_use_capacity_keep_high_water_marks_and_reset_after_pause() {
    let f = Fixture::new();
    f.gpu("0000:09:00.0", "0x1002", "amdgpu");
    f.process(42, 100);
    f.info(42, 3, "0000:09:00.0", 7, 0);
    let mut tracker = Tracker::default();
    let now = Instant::now();
    assert_eq!(tracker.sample_at(&f.0, now).rows[0].activity(), None);
    f.info(42, 3, "0000:09:00.0", 7, 1_000_000_000);
    assert_eq!(
        tracker.sample_at(&f.0, now + Duration::from_secs(2)).rows[0].activity(),
        Some(25.)
    );
    f.info(42, 3, "0000:09:00.0", 7, 500_000_000);
    assert_eq!(
        tracker.sample_at(&f.0, now + Duration::from_secs(4)).rows[0].activity(),
        None
    );
    f.info(42, 3, "0000:09:00.0", 7, 1_400_000_000);
    assert_eq!(
        tracker.sample_at(&f.0, now + Duration::from_secs(6)).rows[0].activity(),
        Some(10.)
    );
    tracker.pause();
    assert_eq!(
        tracker.sample_at(&f.0, now + Duration::from_secs(10)).rows[0].activity(),
        None
    );
}
#[test]
fn exited_processes_and_pid_reuse_do_not_inherit_counters() {
    let f = Fixture::new();
    f.gpu("0000:09:00.0", "0x1002", "amdgpu");
    f.process(42, 100);
    f.info(42, 3, "0000:09:00.0", 7, 100);
    let mut tracker = Tracker::default();
    let now = Instant::now();
    tracker.sample_at(&f.0, now);
    f.process(42, 200);
    assert_eq!(
        tracker.sample_at(&f.0, now + Duration::from_secs(1)).rows[0].activity(),
        None
    );
    fs::remove_dir_all(f.0.join("proc/42")).unwrap();
    assert!(
        tracker
            .sample_at(&f.0, now + Duration::from_secs(2))
            .rows
            .is_empty()
    );
}

#[test]
fn engine_discovery_uses_drm_cards_and_validates_individual_sysfs_counters() {
    let f = Fixture::new();
    for (index, pci, driver) in [
        (17, "0000:05:00.0", "amdgpu"),
        (42, "0000:08:00.0", "amdgpu"),
        (4, "0000:01:00.0", "xe"),
    ] {
        f.gpu(pci, "0x1002", driver);
        let path = f.0.join(format!("sys/class/drm/card{index}"));
        fs::create_dir_all(&path).unwrap();
        std::os::unix::fs::symlink(
            f.0.join("sys/bus/pci/devices").join(pci),
            path.join("device"),
        )
        .unwrap();
    }
    let discovered = crate::engines::discover_at(&f.0).unwrap();
    assert_eq!(
        discovered
            .iter()
            .map(|g| g.pci.as_str())
            .collect::<Vec<_>>(),
        ["0000:05:00.0", "0000:08:00.0"]
    );
    let counter =
        f.0.join("sys/bus/pci/devices/0000:05:00.0/vcn_busy_percent");
    for value in ["0", "100", "42.5"] {
        fs::write(&counter, value).unwrap();
        assert_eq!(
            crate::engines::hardware_percent(&f.0, "0000:05:00.0", "vcn_busy_percent").unwrap(),
            value.parse::<f64>().unwrap()
        );
    }
    for value in ["-1", "101", "NaN", "inf", "broken"] {
        fs::write(&counter, value).unwrap();
        assert!(
            crate::engines::hardware_percent(&f.0, "0000:05:00.0", "vcn_busy_percent").is_err()
        );
    }
    for file in ["gpu_busy_percent", "mem_busy_percent"] {
        fs::write(
            f.0.join("sys/bus/pci/devices/0000:05:00.0").join(file),
            "37",
        )
        .unwrap();
        assert_eq!(
            crate::engines::hardware_percent(&f.0, "0000:05:00.0", file).unwrap(),
            37.
        );
    }
    let readings = crate::engines::readings(&f.0, "0000:08:00.0", &Snapshot::default());
    assert!(readings.iter().all(|r| !r.detected && r.value.is_none()));
    assert!(!readings.iter().any(|r| r.title.contains("décodage")));
    assert!(crate::engines::discover_at(&Fixture::new().0).is_err());
}
#[test]
fn engine_tracker_uses_same_fdinfo_pass_and_keeps_absent_engines_unavailable() {
    let f = Fixture::new();
    let pci = "0000:05:00.0";
    f.gpu(pci, "0x1002", "amdgpu");
    f.process(42, 100);
    f.info(42, 3, pci, 7, 0);
    f.info(42, 4, pci, 7, 0);
    let now = Instant::now();
    let mut tracker = Tracker::default();
    tracker.sample_at(&f.0, now);
    f.info(42, 3, pci, 7, 2_000_000_000);
    f.info(42, 4, pci, 7, 2_000_000_000);
    let snapshot = tracker.sample_at(&f.0, now + Duration::from_secs(1));
    assert_eq!(
        snapshot.engine_activity[pci]["gfx"].milliseconds_per_second,
        Some(2000.)
    );
    let readings = crate::engines::readings(&f.0, pci, &snapshot);
    assert_eq!(readings[0].value, Some(2000.));
    assert!(readings[1..].iter().all(|r| r.value.is_none()));
    tracker.pause();
    assert_eq!(
        tracker
            .sample_at(&f.0, now + Duration::from_secs(2))
            .engine_activity[pci]["gfx"]
            .milliseconds_per_second,
        None
    );
}

#[test]
fn hardware_video_percentage_has_distinct_zero_missing_and_impossible_states() {
    use crate::engines::{State, readings};
    let f = Fixture::new();
    let pci = "0000:05:00.0";
    f.gpu(pci, "0x1002", "amdgpu");
    let path =
        f.0.join("sys/bus/pci/devices")
            .join(pci)
            .join("vcn_busy_percent");
    let read = || readings(&f.0, pci, &Snapshot::default()).pop().unwrap();
    assert_eq!(read().state, State::Unavailable);
    for (text, expected) in [("0", 0.), ("8.2", 8.2), ("100", 100.)] {
        fs::write(&path, text).unwrap();
        let value = read();
        assert_eq!(value.state, State::Available);
        assert!(value.percent);
        assert_eq!(value.value, Some(expected));
    }
    for value in ["100.1", "200", "-0.1", "NaN", "inf", "not a number"] {
        fs::write(&path, value).unwrap();
        let value = read();
        assert_eq!(value.state, State::Impossible);
        assert_eq!(value.value, None);
    }
    fs::remove_file(&path).unwrap();
    assert_eq!(read().state, State::Unavailable);
    // The global GPU busy source must never be relabelled as independent GFX or compute.
    fs::write(
        f.0.join("sys/bus/pci/devices")
            .join(pci)
            .join("gpu_busy_percent"),
        "83",
    )
    .unwrap();
    let missing = readings(&f.0, pci, &Snapshot::default());
    assert!(missing[..3].iter().all(|r| !r.percent && r.value.is_none()));
}
